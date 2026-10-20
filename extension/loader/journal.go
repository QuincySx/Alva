// INPUT: Full AEP I/O, canonical session redaction and the actual owner-bound native log
// OUTPUT: Bounded memory/durable journal receipts with explicit untrusted-source provenance
// POS: [L4 extension] Plugin product policy above protocol; never truncates silently or injects transcripts into the model
package loader

import (
	"context"
	"encoding/json"
	"errors"
	"sync"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/redaction"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// Storage is the capability contract supplied by the outer assembly.
type Storage interface {
	storage.Journal
	storage.Records
	storage.References
}

type pluginJournal struct {
	mu                sync.Mutex
	log               storage.Session
	repository        Storage
	owner, id, plugin string
	cache             []json.RawMessage
	bytes             int
	failed            bool
	secrets           []string
}

func (j *pluginJournal) record(ctx context.Context, runID, direction string, value any) (wire.Reference, error) {
	j.mu.Lock()
	defer j.mu.Unlock()
	if j.failed {
		return wire.Reference{}, errors.New("AEP journal already failed; no replay permitted")
	}
	fail := func(err error) (wire.Reference, error) {
		j.failed = true
		return wire.Reference{}, errors.Join(errors.New("AEP journal failed; interaction stopped"), err)
	}
	frame, err := json.Marshal(value)
	if err != nil {
		return fail(err)
	}
	if direction == "stderr" || direction == "invalid_incoming" || direction == "incomplete_incoming" {
		frame, err = redaction.RedactStreamFrame(frame, j.secrets...)
	} else {
		frame, err = redaction.RedactJSON(frame, j.secrets...)
	}
	if err != nil {
		return fail(err)
	}
	payload, err := json.Marshal(struct {
		RunID     string          `json:"run_id"`
		Plugin    string          `json:"plugin"`
		Direction string          `json:"direction"`
		Frame     json.RawMessage `json:"frame"`
		Source    string          `json:"source_notice"`
	}{runID, j.plugin, direction, frame, pluginSource})
	if err != nil {
		return fail(err)
	}
	record := abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "alva.aep", Type: "plugin_io", SchemaVersion: 1, Payload: payload}
	data, err := json.Marshal(record)
	if err != nil {
		return fail(err)
	}
	if len(j.cache) >= 4096 || len(data)+1024 > (64<<20)-j.bytes {
		return fail(errors.New("AEP memory/durable interaction budget exceeded"))
	}
	committed, err := j.log.AppendRecords(ctx, []abi.SessionRecord{record})
	if err != nil {
		return fail(&abi.SessionError{Operation: "append AEP interaction", SessionID: j.id, Err: err})
	}
	if len(committed) != 1 {
		return fail(errors.New("AEP journal commit receipt invalid"))
	}
	data, err = json.Marshal(committed[0])
	if err != nil {
		return fail(err)
	}
	j.cache = append(j.cache, data)
	j.bytes += len(data)
	page, err := j.repository.Page(ctx, j.owner, j.id, committed[0].Sequence-1, 1, committed[0].Sequence)
	if err != nil {
		return fail(err)
	}
	if len(page.Entries) != 1 || page.Entries[0].ID != committed[0].ID {
		return fail(errors.New("AEP committed reference mismatch"))
	}
	return page.Entries[0].Value, nil
}

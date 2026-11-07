// INPUT: context, encoding/json, errors, maps/sort, kernel/abi and shared storage latest-record/page/artifact ports.
// OUTPUT: Atomic shell ownership facts plus alva.shell/pending_jobs snapshot, snapshot load and one-time paged legacy rebuild.
// POS: [L4 extension] Interruption recovery reads one latest snapshot instead of the whole native session; never adopts PIDs or replays.
package shell

import (
	"context"
	"encoding/json"
	"errors"
	"maps"
	"sort"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

const (
	shellNamespace  = "alva.shell"
	pendingJobsKind = "pending_jobs"
	// Ownership facts and the snapshot are small metadata (IDs/status), never
	// captured output; one record above this budget is corrupt, not history.
	ownershipRecordBytes = 1 << 20
	ownershipPageSize    = 100
)

func ownershipKind(kind string) bool {
	switch kind {
	case "job_intent", "job_outcome", "job_start_failed", "job_abandoned":
		return true
	}
	return false
}

// ownershipChange reports how one ownership fact changes the unfinished-job
// set: job_intent adds its task ID, the other three kinds remove their job.
func ownershipChange(kind string, payload json.RawMessage) (id string, pending bool, err error) {
	var value struct {
		Task struct {
			ID string `json:"id"`
		} `json:"task"`
		Status json.RawMessage `json:"status"`
		JobID  string          `json:"job_id"`
	}
	if json.Unmarshal(payload, &value) != nil {
		return "", false, errors.New("invalid saved shell ownership fact")
	}
	switch kind {
	case "job_intent":
		return value.Task.ID, true, nil
	case "job_outcome":
		var status struct {
			Task struct {
				ID string `json:"id"`
			} `json:"task"`
		}
		if json.Unmarshal(value.Status, &status) != nil {
			return "", false, errors.New("invalid saved shell outcome")
		}
		return status.Task.ID, false, nil
	case "job_start_failed", "job_abandoned":
		return value.JobID, false, nil
	}
	return "", false, errors.New("not a shell ownership fact")
}

func sortedJobIDs(set map[string]bool) []string {
	ids := make([]string, 0, len(set))
	for id := range set {
		ids = append(ids, id)
	}
	sort.Strings(ids)
	return ids
}

// readOwnershipRecord restores one exact alva.shell record body named by
// bounded metadata, verifying that body and metadata identify the same fact.
func (p *Plugin) readOwnershipRecord(ctx context.Context, entry storage.RecordEntry) (abi.SessionRecord, error) {
	_, data, err := p.config.Repository.ReadArtifactFull(ctx, p.config.Owner, p.config.SessionID, entry.Value.URI, ownershipRecordBytes)
	if err != nil {
		return abi.SessionRecord{}, err
	}
	var record abi.SessionRecord
	if json.Unmarshal(data, &record) != nil {
		return abi.SessionRecord{}, errors.New("stored shell ownership record decoding failed")
	}
	if err = record.Validate(); err != nil {
		return abi.SessionRecord{}, err
	}
	if record.Kind != abi.RecordExtension || record.Namespace != shellNamespace || record.Type != entry.Type || record.ID != entry.ID || record.Sequence != entry.Sequence {
		return abi.SessionRecord{}, errors.New("shell ownership record metadata/body differ")
	}
	return record, nil
}

// loadPendingLocked reads the latest pending_jobs snapshot. A session written
// before snapshots existed is paged once (100 metadata entries per page, only
// ownership bodies opened), retaining only the pending set; the caller's next
// commit then writes the first snapshot. Caller holds pendingMu.
func (p *Plugin) loadPendingLocked(ctx context.Context) (found bool, err error) {
	p.pendingLoaded = false
	repository := p.config.Repository
	entry, found, err := repository.LatestRecord(ctx, p.config.Owner, p.config.SessionID, shellNamespace, pendingJobsKind)
	if err != nil {
		return false, err
	}
	pending := map[string]bool{}
	var revision uint64
	if found {
		if entry.SchemaVersion != 1 {
			return false, errors.New("unsupported shell pending_jobs snapshot schema")
		}
		record, err := p.readOwnershipRecord(ctx, entry)
		if err != nil {
			return false, err
		}
		var ids []string
		if json.Unmarshal(record.Payload, &ids) != nil || ids == nil {
			return false, errors.New("invalid shell pending_jobs snapshot")
		}
		for _, id := range ids {
			if id == "" || pending[id] {
				return false, errors.New("invalid shell pending_jobs snapshot")
			}
			pending[id] = true
		}
		revision = entry.Sequence
	} else {
		var after, highWater uint64
		for {
			page, err := repository.Page(ctx, p.config.Owner, p.config.SessionID, after, ownershipPageSize, highWater)
			if err != nil {
				return false, err
			}
			highWater = page.HighWater
			for _, entry := range page.Entries {
				if entry.Kind != abi.RecordExtension || entry.Namespace != shellNamespace || !ownershipKind(entry.Type) {
					continue
				}
				record, err := p.readOwnershipRecord(ctx, entry)
				if err != nil {
					return false, err
				}
				id, add, err := ownershipChange(record.Type, record.Payload)
				if err != nil {
					return false, err
				}
				if add && id != "" {
					pending[id] = true
				} else if !add {
					delete(pending, id)
				}
			}
			if !page.HasMore {
				break
			}
			if page.Next <= after {
				return false, errors.New("shell ownership record page did not advance")
			}
			after = page.Next
		}
	}
	p.pending, p.pendingRevision, p.pendingLoaded = pending, revision, true
	return found, nil
}

// commitOwnershipLocked appends ownership facts and the resulting pending_jobs
// snapshot in one compare-and-append: both commit or neither does. Any error,
// including an ambiguous one, forces the next commit to reload the latest
// snapshot instead of reusing a stale revision; a revision conflict means
// another writer owns this session's shell facts and is never auto-retried.
// Caller holds pendingMu.
func (p *Plugin) commitOwnershipLocked(ctx context.Context, records []abi.SessionRecord) error {
	if !p.pendingLoaded {
		if _, err := p.loadPendingLocked(ctx); err != nil {
			return err
		}
	}
	next := maps.Clone(p.pending)
	for _, record := range records {
		if record.Kind != abi.RecordExtension || record.Namespace != shellNamespace || !ownershipKind(record.Type) {
			return errors.New("only shell ownership facts accompany the pending_jobs snapshot")
		}
		id, add, err := ownershipChange(record.Type, record.Payload)
		if err != nil {
			return err
		}
		if add && id != "" {
			next[id] = true
		} else if !add {
			delete(next, id)
		}
	}
	payload, err := json.Marshal(sortedJobIDs(next))
	if err != nil {
		return err
	}
	committed, err := p.log.AppendExtensionSnapshot(ctx, shellNamespace, pendingJobsKind, p.pendingRevision, payload, records...)
	if err != nil {
		p.pendingLoaded = false
		return err
	}
	p.pending, p.pendingRevision = next, committed.Sequence
	return nil
}

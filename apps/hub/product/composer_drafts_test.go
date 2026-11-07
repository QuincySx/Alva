// INPUT: Controlled scoped draft storage, frozen preparations and actual cancellation lifetimes.
// OUTPUT: Shared snapshot isolation, obsolete-ack preservation, strict receipts and joined I/O regressions.
// POS: [L7 product tests] Exercises the durable composer seam without a UI or model execution.
package product

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/protocol/skill"
	"github.com/QuincySx/Alva/platform/storage"
)

type composerDraftPort struct {
	read func(context.Context, hub.ComposerDraftScope) (hub.ComposerDraft, error)
	save func(context.Context, hub.ComposerDraft, uint64) (hub.ComposerDraft, error)
}

func (p *composerDraftPort) ComposerDraft(ctx context.Context, scope hub.ComposerDraftScope) (hub.ComposerDraft, error) {
	return p.read(ctx, scope)
}
func (p *composerDraftPort) SaveComposerDraft(ctx context.Context, value hub.ComposerDraft, expected uint64) (hub.ComposerDraft, error) {
	return p.save(ctx, value, expected)
}

func composerMemoryDraftPort() *composerDraftPort {
	var mu sync.Mutex
	rows := map[hub.ComposerDraftScope]hub.ComposerDraft{}
	return &composerDraftPort{
		read: func(ctx context.Context, scope hub.ComposerDraftScope) (hub.ComposerDraft, error) {
			mu.Lock()
			defer mu.Unlock()
			if err := ctx.Err(); err != nil {
				return hub.ComposerDraft{}, err
			}
			value, ok := rows[scope]
			if !ok {
				value.Scope = scope
			}
			return hub.CloneComposerDraft(value), nil
		},
		save: func(ctx context.Context, value hub.ComposerDraft, expected uint64) (hub.ComposerDraft, error) {
			mu.Lock()
			defer mu.Unlock()
			if err := ctx.Err(); err != nil {
				return hub.ComposerDraft{}, err
			}
			if rows[value.Scope].Revision != expected {
				return hub.ComposerDraft{}, hub.ErrComposerDraftConflict
			}
			value.Revision = expected + 1
			rows[value.Scope] = hub.CloneComposerDraft(value)
			return hub.CloneComposerDraft(value), nil
		},
	}
}

func TestComposerDraftsSharedSnapshotsAndObsoleteAcknowledgements(t *testing.T) {
	owner, err := NewComposerDraftsService(composerMemoryDraftPort())
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Close(t.Context())
	scope := hub.ComposerDraftScope{Kind: "home", ID: "shared-new-task"}
	if _, known := owner.Snapshot(scope); known {
		t.Fatal("unknown cache was presented as a successful empty read")
	}
	baseline, err := owner.Read(t.Context(), scope)
	if err != nil || baseline.Revision != 0 {
		t.Fatal(baseline, err)
	}
	prepared, err := (&HubService{}).PrepareComposerSelection(t.Context(), "", ComposerSelectionRequest{EntryID: "builtin:review"})
	if err != nil {
		t.Fatal(err)
	}
	commands, err := EncodeComposerDraftPreparations([]ComposerPreparation{prepared})
	if err != nil {
		t.Fatal(err)
	}
	first, err := owner.Save(t.Context(), hub.ComposerDraft{Scope: scope, Text: "same text", References: []hub.ComposerDraftReference{{Kind: "file", ID: "saved-original", SHA256: strings.Repeat("a", 64)}}, PreparedCommands: commands}, 0)
	if err != nil {
		t.Fatal(err)
	}
	// A second window sees a committed copy; changing it cannot mutate the owner.
	observer, known := owner.Snapshot(scope)
	if !known || !reflect.DeepEqual(first, observer) {
		t.Fatal("another observer missed the shared committed draft")
	}
	observer.References[0].ID = "mutated"
	observer.PreparedCommands[0] = ' '
	if current, _ := owner.Snapshot(scope); !reflect.DeepEqual(current, first) {
		t.Fatal("observer mutated the shared cache")
	}
	second := hub.CloneComposerDraft(first)
	second.Text = "edited"
	second, err = owner.Save(t.Context(), second, first.Revision)
	if err != nil {
		t.Fatal(err)
	}
	second.Text = first.Text
	third, err := owner.Save(t.Context(), second, second.Revision)
	if err != nil {
		t.Fatal(err)
	}
	if consumed, err := owner.Consume(t.Context(), scope, first.Revision); err != nil || consumed {
		t.Fatal("late Send acknowledgement erased edited-back input", consumed, err)
	}
	if current, _ := owner.Snapshot(scope); !reflect.DeepEqual(current, third) {
		t.Fatal("obsolete acknowledgement changed the newer manual draft")
	}
	if consumed, err := owner.Consume(t.Context(), scope, third.Revision); err != nil || !consumed {
		t.Fatal("matching admitted revision was not consumed", consumed, err)
	}
	if current, _ := owner.Snapshot(scope); current.Revision != third.Revision+1 || current.Text != "" || len(current.References) != 0 || len(current.PreparedCommands) != 0 {
		t.Fatal("consumption did not publish a durable empty revision", current)
	}
	if _, err = owner.Save(t.Context(), first, first.Revision); !errors.Is(err, hub.ErrComposerDraftConflict) {
		t.Fatal("stale second window overwrote consumption", err)
	}
}

func TestComposerDraftPreparationSchemaAndOwnershipAreStrict(t *testing.T) {
	scope := hub.ComposerDraftScope{Kind: "session", ID: "product-session"}
	skillPrepared := ComposerPreparation{EntryID: "skill:review", Kind: ComposerSkillEntry, Origin: "alva.project-skill", Scope: "workspace", Text: "请使用项目 Skill「review」处理下一次请求。", SkillRef: &ComposerSkillReference{SessionID: scope.ID, BranchID: "branch", Name: "review", RelativePath: ManagedSkillDirectory + "/review/SKILL.md", SHA256: strings.Repeat("b", 64), Policy: skill.Automatic}}
	data, err := EncodeComposerDraftPreparations([]ComposerPreparation{skillPrepared})
	if err != nil {
		t.Fatal(err)
	}
	draft := hub.ComposerDraft{Scope: scope, PreparedCommands: data}
	decoded, err := DecodeComposerDraftPreparations(draft)
	if err != nil || len(decoded) != 1 || !reflect.DeepEqual(decoded[0], skillPrepared) {
		t.Fatal(decoded, err)
	}
	for _, bad := range []json.RawMessage{
		json.RawMessage(`[ {"entry_id":"builtin:review","entry_id":"builtin:plan"} ]`),
		json.RawMessage(`[ {"Entry_ID":"builtin:review"} ]`),
		json.RawMessage(`[ {"entry_id":"builtin:review","unknown":true} ]`),
		json.RawMessage(`[ {"entry_id":"\ud800"} ]`),
		json.RawMessage(strings.Replace(string(data), `"auto"`, `"strict"`, 1)),
		json.RawMessage(strings.Replace(string(data), `"branch"`, `""`, 1)),
		json.RawMessage(strings.Replace(string(data), `review/SKILL.md`, `escape/SKILL.md`, 1)),
	} {
		draft.PreparedCommands = bad
		if _, err = DecodeComposerDraftPreparations(draft); err == nil {
			t.Fatalf("malformed or altered preparation accepted: %s", bad)
		}
	}
	draft.PreparedCommands = data
	draft.Scope.ID = "another-session"
	if _, err = DecodeComposerDraftPreparations(draft); err == nil {
		t.Fatal("foreign saved Skill preparation accepted")
	}
}

func TestComposerDraftsFailedOrFalseStoreReceiptsLeaveCommittedCache(t *testing.T) {
	scope := hub.ComposerDraftScope{Kind: "home", ID: "receipt"}
	baseline := hub.ComposerDraft{Scope: scope, Revision: 8, Text: "committed"}
	port := &composerDraftPort{read: func(context.Context, hub.ComposerDraftScope) (hub.ComposerDraft, error) { return baseline, nil }}
	owner, err := NewComposerDraftsService(port)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = owner.Read(t.Context(), scope); err != nil {
		t.Fatal(err)
	}
	failure := errors.New("selected storage failure")
	port.save = func(context.Context, hub.ComposerDraft, uint64) (hub.ComposerDraft, error) {
		return hub.ComposerDraft{}, failure
	}
	if _, err = owner.Save(t.Context(), baseline, baseline.Revision); !errors.Is(err, failure) {
		t.Fatal(err)
	}
	port.save = func(context.Context, hub.ComposerDraft, uint64) (hub.ComposerDraft, error) { return baseline, nil }
	if _, err = owner.Save(t.Context(), baseline, baseline.Revision); err == nil {
		t.Fatal("false committed revision accepted")
	}
	for _, bad := range []hub.ComposerDraft{{Scope: scope, Revision: 7, Text: "older"}, {Scope: scope, Revision: 8, Text: "different"}, {Scope: hub.ComposerDraftScope{Kind: "home", ID: "foreign"}, Revision: 9, Text: "foreign"}} {
		port.read = func(context.Context, hub.ComposerDraftScope) (hub.ComposerDraft, error) { return bad, nil }
		if _, err = owner.Read(t.Context(), scope); err == nil {
			t.Fatal("invalid stored read receipt accepted", bad)
		}
		if current, _ := owner.Snapshot(scope); !reflect.DeepEqual(current, baseline) {
			t.Fatal("failed read replaced the last committed observation")
		}
	}
	var nilPort *composerDraftPort
	if _, err = NewComposerDraftsService(nilPort); err == nil {
		t.Fatal("typed-nil storage accepted")
	}
}

func TestComposerDraftCloseRetainsAndJoinsActualLateCommit(t *testing.T) {
	scope := hub.ComposerDraftScope{Kind: "home", ID: "close"}
	baseline := hub.ComposerDraft{Scope: scope, Revision: 1, Text: "before"}
	committed := baseline
	committed.Revision, committed.Text = 2, "committed after cancellation"
	entered := make(chan context.Context, 1)
	release := make(chan struct{})
	defer close(release)
	var reads, writes atomic.Int32
	port := &composerDraftPort{
		read: func(context.Context, hub.ComposerDraftScope) (hub.ComposerDraft, error) {
			reads.Add(1)
			return baseline, nil
		},
		save: func(ctx context.Context, _ hub.ComposerDraft, _ uint64) (hub.ComposerDraft, error) {
			writes.Add(1)
			entered <- ctx
			<-release // Represents physical I/O whose actual return must be joined.
			return committed, nil
		},
	}
	owner, err := NewComposerDraftsService(port)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = owner.Read(t.Context(), scope); err != nil {
		t.Fatal(err)
	}
	results := make(chan error, 1)
	go func() { _, err := owner.Save(t.Context(), committed, 1); results <- err }()
	var admitted context.Context
	select {
	case admitted = <-entered:
	case <-time.After(2 * time.Second):
		t.Fatal("write never entered selected storage")
	}
	snapshot := make(chan hub.ComposerDraft, 1)
	go func() { value, _ := owner.Snapshot(scope); snapshot <- value }()
	select {
	case value := <-snapshot:
		if !reflect.DeepEqual(value, baseline) {
			t.Fatal("uncommitted data reached Snapshot")
		}
	case <-time.After(time.Second):
		t.Fatal("Snapshot waited for storage I/O")
	}
	queued, cancelQueued := context.WithCancel(t.Context())
	cancelQueued()
	if _, err = owner.Read(queued, scope); !errors.Is(err, context.Canceled) || reads.Load() != 1 {
		t.Fatal("canceled queued operation reached storage", err)
	}
	for range 2 {
		ctx, cancel := context.WithTimeout(t.Context(), 20*time.Millisecond)
		err = owner.Close(ctx)
		cancel()
		if !errors.Is(err, context.DeadlineExceeded) || !errors.Is(admitted.Err(), context.Canceled) {
			t.Fatal("unfinished write was not retained after Close timeout", err)
		}
	}
	if _, err = owner.Read(t.Context(), scope); !errors.Is(err, storage.ErrClosed) || writes.Load() != 1 {
		t.Fatal("closed owner admitted storage work", err)
	}
	release <- struct{}{}
	select {
	case err = <-results:
		if err != nil {
			t.Fatal("late actual commit was hidden", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("write did not return")
	}
	if err = owner.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	if current, _ := owner.Snapshot(scope); !reflect.DeepEqual(current, committed) || writes.Load() != 1 || reads.Load() != 1 {
		t.Fatal("late commit lost or operation replayed", current)
	}
}

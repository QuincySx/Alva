// INPUT: context, encoding/json, errors, reflect, strings, testing, kernel/abi, llm/wire
// OUTPUT: Explicit file import, backend memory, atomic rewrite and externalization regressions
// POS: [L3 capability library] Tests SDK capabilities without granting OS paths or mutating durable session facts.
package agentcontext

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type contextFileFixture func(context.Context, ContextFileRequest) (ContextFileResult, error)

func (reader contextFileFixture) Load(ctx context.Context, request ContextFileRequest) (ContextFileResult, error) {
	return reader(ctx, request)
}

func TestExplicitFileImportUsesBoundReaderAndTruthfulProjection(t *testing.T) {
	bound := contextFileFixture(func(_ context.Context, request ContextFileRequest) (ContextFileResult, error) {
		if request.AgentID != "actual-session" {
			return ContextFileResult{}, errors.New("foreign session denied")
		}
		if request.Path != "selected.log" || request.Lines == nil || *request.Lines != (LineRange{Start: 2, End: 3}) || request.MaxBytes != 32 {
			return ContextFileResult{}, errors.New("request binding lost")
		}
		return ContextFileResult{Path: "selected.log", Text: "line2\nline3", Source: wire.Reference{URI: "artifact://selected", Bytes: 100, Processing: "Original UTF-8 text stored by session"}, StartLine: 2, EndLine: 3, Truncated: true}, nil
	})
	store := NewStore(1000, 1000, nil, nil, StoreServices{Files: bound, MaxFileBytes: 32})
	if err := store.InjectFromFile(context.Background(), "foreign", "selected.log", &LineRange{Start: 2, End: 3}); err == nil || len(store.Entries("foreign")) != 0 {
		t.Fatal("path string granted foreign access")
	}
	if err := store.InjectFromFile(context.Background(), "actual-session", "selected.log", &LineRange{Start: 2, End: 3}); err != nil {
		t.Fatal(err)
	}
	entries := store.Entries("actual-session")
	if len(entries) != 1 || entries[0].Metadata.Layer != abi.OnDemand || entries[0].Message.Role != wire.RoleUser || !strings.Contains(entries[0].Message.Text(), `"truncated":true`) || !strings.Contains(entries[0].Message.Text(), "line2\nline3") || !strings.Contains(entries[0].Message.Content[0].Reference.Processing, "no additional media transformation") {
		t.Fatal("source/status lost", entries)
	}
	for _, text := range []string{string([]byte{0xff}), strings.Repeat("a", 33)} {
		invalid := NewStore(1000, 1000, nil, nil, StoreServices{Files: contextFileFixture(func(context.Context, ContextFileRequest) (ContextFileResult, error) {
			return ContextFileResult{Path: "x", Text: text, Source: wire.Reference{URI: "artifact://x"}, StartLine: 1, EndLine: 1}, nil
		}), MaxFileBytes: 32})
		if err := invalid.InjectFromFile(context.Background(), "a", "x", nil); err == nil || len(invalid.Entries("a")) != 0 {
			t.Fatal("invalid/unbounded import retained")
		}
	}
}

func TestContextRewriteBatchAtomicAndOwned(t *testing.T) {
	store := NewStore(1000, 1000, nil, nil)
	for _, id := range []string{"a", "b"} {
		message := wire.TextMessage(wire.RoleUser, id)
		message.ID = id
		_ = store.InjectMessage("session", abi.OnDemand, message)
	}
	before := store.Entries("session")
	rewrite := wire.TextMessage(wire.RoleUser, "changed")
	if err := store.RewriteBatch("session", []MessageRewrite{{ID: "a", Message: rewrite}, {ID: "missing", Message: rewrite}}); err == nil || !reflect.DeepEqual(before, store.Entries("session")) {
		t.Fatal("partial rewrite applied")
	}
	if err := store.RewriteBatch("session", []MessageRewrite{{ID: "a", Message: rewrite}, {ID: "b", Message: rewrite}}); err != nil {
		t.Fatal(err)
	}
	rewrite.Content[0].Text = "caller changed"
	for _, entry := range store.Entries("session") {
		if entry.Message.Text() != "changed" || entry.Message.ID != entry.ID {
			t.Fatal("ownership/identity lost")
		}
	}
}

type referenceStoreCallback struct {
	ReferenceBackend
	after func()
}

func (backend referenceStoreCallback) StoreText(ctx context.Context, text string) (wire.Reference, error) {
	reference, err := backend.ReferenceBackend.StoreText(ctx, text)
	if err == nil && backend.after != nil {
		backend.after()
	}
	return reference, err
}

func TestContextExternalizationExactBytesAndConcurrentConflict(t *testing.T) {
	for _, conflict := range []bool{false, true} {
		backend, _ := NewMemoryReferences("session", 1<<20)
		var store *Store
		callback := referenceStoreCallback{ReferenceBackend: backend}
		if conflict {
			callback.after = func() {
				_ = store.InjectMessage("session", abi.RuntimeInject, wire.TextMessage(wire.RoleUser, "concurrent"))
			}
		}
		store = NewStore(1000, 1000, nil, nil, StoreServices{References: callback})
		message := wire.TextMessage(wire.RoleUser, "original 日志")
		message.ID = "original"
		_ = store.InjectMessage("session", abi.RuntimeInject, message)
		before := store.Entries("session")
		reference, err := store.Externalize(context.Background(), "session", abi.MessageRange{From: abi.MessageSelector{Kind: abi.FromStart}, To: abi.MessageSelector{Kind: abi.ToEnd}}, "")
		if conflict != (err != nil) || reference.URI == "" {
			t.Fatal("bad settlement", reference, err)
		}
		page, readErr := backend.ReadText(context.Background(), reference.URI, 0, MaxTextPageBytes)
		var retained []abi.ContextEntry
		if readErr != nil || json.Unmarshal([]byte(page.Text), &retained) != nil || !reflect.DeepEqual(retained, before) {
			t.Fatal("artifact lost exact entries", readErr)
		}
		entries := store.Entries("session")
		if conflict {
			if len(entries) != 2 || entries[0].Message.Text() != "original 日志" || !strings.Contains(err.Error(), "artifact retained") {
				t.Fatal("conflict destroyed context", entries, err)
			}
		} else if len(entries) != 1 || entries[0].Message.Content[0].Reference == nil || entries[0].Message.Content[0].Reference.URI != reference.URI {
			t.Fatal("missing actual reference", entries)
		}
	}
}

type factsFixture struct {
	facts      []abi.MemoryFact
	stored     abi.MemoryFact
	deleted    string
	err        error
	queryCalls int
}

type cancellingFacts struct{ cancel context.CancelFunc }

func (backend cancellingFacts) Query(context.Context, string, int) ([]abi.MemoryFact, error) {
	backend.cancel()
	return []abi.MemoryFact{{ID: "late", Text: "late memory", Confidence: 1}}, nil
}
func (cancellingFacts) Store(context.Context, abi.MemoryFact) error { return nil }
func (cancellingFacts) Delete(context.Context, string) error        { return nil }

func TestContextMemoryRejectsLateBackendRead(t *testing.T) {
	for _, inject := range []bool{false, true} {
		t.Run(map[bool]string{false: "query", true: "inject"}[inject], func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			store := NewStore(1000, 1000, nil, nil, StoreServices{Memory: cancellingFacts{cancel: cancel}})
			var facts []abi.MemoryFact
			var err error
			if inject {
				facts, err = store.InjectMemory(ctx, "session", "", 100)
			} else {
				facts, err = store.QueryMemory(ctx, "", 20)
			}
			if !errors.Is(err, context.Canceled) || len(facts) != 0 || len(store.Entries("session")) != 0 {
				t.Fatal("cancelled backend read was consumed", facts, store.Entries("session"), err)
			}
		})
	}
}

func (backend *factsFixture) Query(context.Context, string, int) ([]abi.MemoryFact, error) {
	backend.queryCalls++
	return backend.facts, backend.err
}
func (backend *factsFixture) Store(_ context.Context, fact abi.MemoryFact) error {
	backend.stored = fact
	return backend.err
}
func (backend *factsFixture) Delete(_ context.Context, id string) error {
	backend.deleted = id
	return backend.err
}

func TestContextMemoryBindingBudgetAndFailure(t *testing.T) {
	backend := &factsFixture{facts: []abi.MemoryFact{{ID: "one", Text: "12345678", Confidence: 1}, {ID: "two", Text: "12345678", Confidence: 0.5}}}
	store := NewStore(1000, 1000, nil, nil, StoreServices{Memory: backend})
	if facts, err := store.InjectMemory(context.Background(), "a", "", 2); err != nil || len(facts) != 1 || facts[0].ID != "one" {
		t.Fatal("bad memory budget", facts, err)
	}
	if facts, err := store.InjectMemory(context.Background(), "a", "", 0); err != nil || len(facts) != 0 || backend.queryCalls != 1 {
		t.Fatal("zero budget queried backend", facts, err)
	}
	if err := store.StoreMemory(context.Background(), backend.facts[0]); err != nil || backend.stored.Fingerprint == "" {
		t.Fatal("store binding missing", err)
	}
	if err := store.DeleteMemory(context.Background(), "one"); err != nil || backend.deleted != "one" {
		t.Fatal("delete binding missing", err)
	}
	backend.err = errors.New("memory unavailable")
	if _, err := store.InjectMemory(context.Background(), "a", "", 2); !errors.Is(err, backend.err) {
		t.Fatal("backend failure concealed", err)
	}
}

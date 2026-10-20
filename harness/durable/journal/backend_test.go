// INPUT: Explicit local test subprocesses, private SQLite, actual SDK and security approval bindings.
// OUTPUT: Cross-process external/approval resume and crash-after-side-effect no-replay acceptance.
// POS: [L5 native harness testing] Real process exit/reopen proves durability separately from volatile rendezvous behavior.
package journal_test

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/harness/durable"
	native "github.com/QuincySx/Alva/harness/durable/journal"
	"github.com/QuincySx/Alva/platform/storage/blob"
	blobmemory "github.com/QuincySx/Alva/platform/storage/blob/memory"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (fn modelFunc) Stream(ctx context.Context, r abi.ModelRequest, e func(wire.StreamChunk) error) error {
	return fn(ctx, r, e)
}
func discard(abi.Event) error { return nil }

func TestNativeDurableRejectsVolatileArtifactRetention(t *testing.T) {
	objects, err := blobmemory.New(blobmemory.Limits{Bytes: 1 << 20, Objects: 32})
	if err != nil {
		t.Fatal(err)
	}
	repo, err := session.OpenWithBlobs(t.Context(), filepath.Join(t.TempDir(), "facts"), session.BlobConfig{ID: "temporary", Retention: blob.Volatile, Store: objects})
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	if _, err = repo.Create(t.Context(), "owner", "session", ""); err != nil {
		t.Fatal(err)
	}
	if _, err = native.Open(t.Context(), repo, "owner", "session"); err == nil {
		t.Fatal("volatile result bytes advertised as restart-durable waits")
	}
}

type fixtureTool struct {
	guard *security.Guard
	dir   string
	crash bool
}

func (*fixtureTool) Name() string                            { return "mutate" }
func (*fixtureTool) Description() string                     { return "approved local fixture mutation" }
func (*fixtureTool) ParametersSchema() json.RawMessage       { return json.RawMessage(`{"type":"object"}`) }
func (*fixtureTool) Scheduling(wire.ToolCall) abi.Scheduling { return abi.Scheduling{} }
func (*fixtureTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectUnknown, Description: "append fixture evidence"}
}
func (*fixtureTool) IsReadOnly(wire.ToolCall) bool { return false }
func (t *fixtureTool) Execute(ctx context.Context, e abi.ToolExecutionContext, c wire.ToolCall) (wire.ToolResult, error) {
	if err := t.guard.Authorize(ctx, e, c, t.DescribeEffect(c)); err != nil {
		return wire.ToolResult{}, err
	}
	f, err := os.OpenFile(filepath.Join(t.dir, "effects"), os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
	if err != nil {
		return wire.ToolResult{}, err
	}
	_, err = f.WriteString("executed\n")
	closeErr := f.Close()
	if err != nil || closeErr != nil {
		return wire.ToolResult{}, errors.Join(err, closeErr)
	}
	if t.crash {
		os.Exit(42)
	}
	return wire.ToolResult{}, nil
}

func TestDurableProcessHelper(t *testing.T) {
	stage, dir, kind := os.Getenv("ALVA_DURABLE_STAGE"), os.Getenv("ALVA_DURABLE_DIR"), os.Getenv("ALVA_DURABLE_KIND")
	if stage == "" {
		t.Skip("explicit subprocess fixture")
	}
	repo, err := session.Open(t.Context(), filepath.Join(dir, "facts"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	if stage == "start" {
		if _, err := repo.Create(t.Context(), "owner", "native", ""); err != nil {
			t.Fatal(err)
		}
	}
	backend, err := native.Open(t.Context(), repo, "owner", "native")
	if err != nil {
		t.Fatal(err)
	}
	c, err := durable.New(durable.Config{Backend: backend, TTL: time.Hour})
	if err != nil {
		t.Fatal(err)
	}
	if stage == "resume" || stage == "crash" {
		s, err := c.Snapshot(t.Context())
		if err != nil || len(s.State.Requests) != 1 {
			t.Fatal(s, err)
		}
		value := json.RawMessage(`{"answer":42}`)
		if kind == "approval" {
			value = json.RawMessage(`{"allow":true}`)
		}
		if err := c.Resolve(t.Context(), durable.Resolution{RequestID: s.State.Requests[0].ID, Binding: s.State.Requests[0].Binding, Result: value}); err != nil {
			t.Fatal(err)
		}
	}
	guard, err := security.NewGuard(dir, nil, nil, security.NewPermissionManager(security.Ask, c))
	if err != nil {
		t.Fatal(err)
	}
	tool := &fixtureTool{guard: guard, dir: dir, crash: stage == "crash"}
	called := false
	model := modelFunc(func(_ context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		if stage == "after-crash" {
			t.Fatal("interrupted mutation was replayed")
		}
		if !called && (stage == "start" || kind == "approval") {
			called = true
			name, args := "await_external", `{"request":{"job":"fixture"}}`
			if kind == "approval" {
				name, args = tool.Name(), `{"value":1}`
			}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "c-" + stage, Name: name, Arguments: json.RawMessage(args)}}); err != nil {
				return err
			}
		} else if kind == "external" {
			if !strings.Contains(r.Messages[len(r.Messages)-1].Text(), `"answer":42`) {
				t.Fatal("result not delivered", r.Messages)
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	// This Controller owns BeginRun. Views contribute refs/history/context and
	// deliberately do not acquire a second native execution claim.
	views, err := harness.SessionViews(t.Context(), repo, "owner", "native")
	if err != nil {
		t.Fatal(err)
	}
	builder := harness.NewBuilder(model, backend.Log()).Workspace(dir).Plugin(c).Tool(tool)
	for _, view := range views {
		builder.Plugin(view)
	}
	// Fixture tool performs its own real Guard authorization, so trust only
	// this actual object in the normal harness security contribution.
	builder.Plugin(harness.NewSecurityPlugin(guard, []abi.Tool{tool, c.Tool()}, views...))
	a, err := builder.Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	var outcome durable.Outcome
	if stage == "start" {
		outcome, err = c.Start(t.Context(), a, []wire.Message{wire.TextMessage(wire.RoleUser, "fixture")}, discard)
	} else {
		outcome, err = c.Resume(t.Context(), a, discard)
	}
	if stage == "after-crash" {
		if !errors.Is(err, durable.ErrInterrupted) {
			t.Fatal("crash did not block replay", err)
		}
		records, err := backend.Log().(abi.SessionStore).Records(t.Context(), 0)
		if err != nil {
			t.Fatal(err)
		}
		unknown := false
		for _, r := range records {
			if r.Namespace == "alva.native.recovery" {
				unknown = true
			}
		}
		if !unknown {
			t.Fatal("interrupted native tool lacks unknown execution evidence")
		}
		return
	}
	if err != nil || outcome.Suspended != (stage == "start") {
		t.Fatal(outcome, err)
	}
}

func process(t *testing.T, dir, kind, stage string, wantExit int) {
	t.Helper()
	command := exec.CommandContext(t.Context(), os.Args[0], "-test.run=^TestDurableProcessHelper$", "-test.timeout=30s")
	command.Env = []string{"ALVA_DURABLE_DIR=" + dir, "ALVA_DURABLE_KIND=" + kind, "ALVA_DURABLE_STAGE=" + stage}
	out, err := command.CombinedOutput()
	if wantExit != 0 {
		var exit *exec.ExitError
		if !errors.As(err, &exit) || exit.ExitCode() != wantExit {
			t.Fatalf("expected exit %d: %v\n%s", wantExit, err, out)
		}
		return
	}
	if err != nil {
		t.Fatalf("stage %s: %v\n%s", stage, err, out)
	}
}
func TestCrossProcessExternalAndApprovalResume(t *testing.T) {
	for _, kind := range []string{"external", "approval"} {
		t.Run(kind, func(t *testing.T) {
			dir := t.TempDir()
			process(t, dir, kind, "start", 0)
			if _, err := os.Stat(filepath.Join(dir, "effects")); !os.IsNotExist(err) {
				t.Fatal("approval executed before decision", err)
			}
			process(t, dir, kind, "resume", 0)
			if kind == "approval" {
				data, err := os.ReadFile(filepath.Join(dir, "effects"))
				if err != nil || string(data) != "executed\n" {
					t.Fatal("effect count", string(data), err)
				}
			}
		})
	}
}
func TestCrashAfterRealEffectNeverReplays(t *testing.T) {
	dir := t.TempDir()
	process(t, dir, "approval", "start", 0)
	process(t, dir, "approval", "crash", 42)
	process(t, dir, "approval", "after-crash", 0)
	data, err := os.ReadFile(filepath.Join(dir, "effects"))
	if err != nil || string(data) != "executed\n" {
		t.Fatal("crashed effect replayed", string(data), err)
	}
}

func TestLargeDeliveryIsReferencedWithoutDroppingOtherContent(t *testing.T) {
	repo, err := session.Open(t.Context(), filepath.Join(t.TempDir(), "facts"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	if _, err := repo.Create(t.Context(), "owner", "native", ""); err != nil {
		t.Fatal(err)
	}
	b, err := native.Open(t.Context(), repo, "owner", "native")
	if err != nil {
		t.Fatal(err)
	}
	refs, err := repo.BoundReferences(t.Context(), "owner", "native")
	if err != nil {
		t.Fatal(err)
	}
	other, err := refs.StoreText(t.Context(), "other explicit evidence")
	if err != nil {
		t.Fatal(err)
	}
	large := strings.Repeat("delivery-data", 600)
	message := wire.Message{Role: wire.RoleUser, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: large}, {Kind: wire.ContentReference, Reference: &other}}}
	if _, err := b.Commit(t.Context(), 0, durable.State{Version: 1, Status: durable.Running}, []wire.Message{message}); err != nil {
		t.Fatal(err)
	}
	messages, err := b.Log().Messages(t.Context())
	if err != nil || len(messages) != 1 || strings.Contains(messages[0].Text(), large) {
		t.Fatal("large body delivered inline", messages, err)
	}
	var retained *wire.Reference
	keptOther := false
	for _, block := range messages[0].Content {
		if block.Reference == nil {
			continue
		}
		if block.Reference.URI == other.URI {
			keptOther = true
		} else {
			retained = block.Reference
		}
	}
	if !keptOther || retained == nil {
		t.Fatal("projection dropped unrelated evidence", messages)
	}
	var restored strings.Builder
	for offset := int64(0); ; {
		page, err := refs.ReadText(t.Context(), retained.URI, offset, 4096)
		if err != nil {
			t.Fatal(err)
		}
		restored.WriteString(page.Text)
		if !page.Truncated {
			break
		}
		if page.NextOffset <= offset {
			t.Fatal("reference cursor did not advance")
		}
		offset = page.NextOffset
	}
	if restored.String() != large {
		t.Fatal("referenced delivery lost data")
	}
}

func TestRedactionCannotCommitAnUnusableRequestBinding(t *testing.T) {
	repo, err := session.Open(t.Context(), filepath.Join(t.TempDir(), "facts"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	if _, err := repo.Create(t.Context(), "owner", "native", ""); err != nil {
		t.Fatal(err)
	}
	b, err := native.Open(t.Context(), repo, "owner", "native")
	if err != nil {
		t.Fatal(err)
	}
	c, err := durable.New(durable.Config{Backend: b, TTL: time.Hour})
	if err != nil {
		t.Fatal(err)
	}
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "c", Name: "await_external", Arguments: json.RawMessage(`{"request":{"api_key":"fixture-private-value"}}`)}}); err != nil {
			return err
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	a, err := core.NewBuilder(model, b.Log()).Plugin(c).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if out, err := c.Start(t.Context(), a, nil, discard); err == nil || out.Suspended {
		t.Fatal("modified binding became a waiting checkpoint", out, err)
	}
	s, err := c.Snapshot(t.Context())
	if err != nil || s.State.Status != durable.Interrupted || len(s.State.Requests) != 0 {
		t.Fatal("unusable state was persisted", s, err)
	}
}

// INPUT: Actual SDK/native diagnostics seam, custom manager reports and same/foreign artifact evidence.
// OUTPUT: Replaceable manager evidence validation, typed-nil and late cancellation regressions.
// POS: [L4 extension] Custom diagnostic implementations must return real same-session sources rather than plausible metadata.
package tooling

import (
	"context"
	"errors"
	"testing"

	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type nilMapManager map[string]string

func (nilMapManager) Catalog() []ProfileInfo                       { return []ProfileInfo{{ID: "approved"}} }
func (nilMapManager) Run(context.Context, Request) (Report, error) { return Report{}, nil }
func TestDiagnosticRejectsAllTypedNilManagers(t *testing.T) {
	f := newFixture(t, true)
	var manager nilMapManager
	if _, err := New(t.Context(), Config{Repository: f.repo, Owner: "owner", SessionID: "native", Cwd: f.cwd, Guard: f.guard, Manager: manager, Report: func(context.Context, Activity) error { return nil }}); err == nil {
		t.Fatal("typed-nil map manager accepted")
	}
}
func TestDiagnosticManagerEvidenceMustBeActualAndCancellationJoined(t *testing.T) {
	for _, mode := range []string{"valid", "invalid_ref", "foreign", "forged_hash", "late_cancel"} {
		t.Run(mode, func(t *testing.T) {
			f := newFixture(t, true)
			id := "native"
			if mode == "foreign" {
				id = "other"
				if _, err := f.repo.Create(t.Context(), "owner", id, ""); err != nil {
					t.Fatal(err)
				}
			}
			info, err := f.repo.PutArtifact(t.Context(), "owner", id, session.ArtifactInput{Kind: "diagnostic-stdout", Name: "approved", MediaType: "text/plain", Processing: "actual custom diagnostic source", Data: []byte("verified source"), Text: true})
			if err != nil {
				t.Fatal(err)
			}
			ref := info.Reference()
			if mode == "invalid_ref" {
				ref = wire.Reference{URI: "fake://source"}
			}
			if mode == "forged_hash" {
				ref.SHA256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
			}
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			manager := &managerFixture{infos: []ProfileInfo{{ID: "approved"}}, run: func(r Request) (Report, error) {
				if mode == "late_cancel" {
					cancel()
				}
				return Report{SessionID: r.Execution.SessionID, RunID: r.Execution.RunID, CallID: r.Call.ID, ProfileID: r.ProfileID, Status: "completed", Started: true, Reaped: true, CapturedComplete: true, Stdout: ref, Stderr: ref, SourceNotice: "Actual custom source; no parsed diagnostics"}, nil
			}}
			p, err := New(t.Context(), Config{Repository: f.repo, Owner: "owner", SessionID: "native", Cwd: f.cwd, Guard: f.guard, Manager: manager, Report: func(context.Context, Activity) error { return nil }})
			if err != nil {
				t.Fatal(err)
			}
			state, _ := abi.NewAgentState(modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { return nil }), f.log, []abi.Tool{p.Tool()})
			runtime := abi.RuntimeContext{State: state, RunID: "run", Workspace: f.cwd}
			if err := p.OnRunStart(t.Context(), runtime); err != nil {
				t.Fatal(err)
			}
			defer p.OnRunEnd(context.Background(), runtime, nil)
			_, err = p.Tool().Execute(ctx, abi.ToolExecutionContext{Session: f.log, SessionID: "native", RunID: "run", CallID: "call", Workspace: f.cwd, ActualTool: p.Tool()}, wire.ToolCall{ID: "call", Name: p.Tool().Name(), Arguments: []byte(`{"action":"run","profile_id":"approved"}`)})
			if mode == "valid" {
				if err != nil {
					t.Fatal(err)
				}
			} else if err == nil {
				t.Fatal("unverified/late-cancel manager returned success")
			}
			if mode == "late_cancel" && !errors.Is(err, context.Canceled) {
				t.Fatal("lost cancellation", err)
			}
		})
	}
}

// INPUT: Committed Hub events carrying an already-redacted large Pi reply and the existing CLI finalAnswer reader
// OUTPUT: Exact full-answer reconstruction from honest bounded deltas after native completion
// POS: [L7 CLI entry] Verifies the consumer contract without inventing a process Inspector or treating preview references as readable originals
package main

import (
	"context"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/integration/engine"
)

type piCompletedAnswerFixture struct{ text string }

func (e piCompletedAnswerFixture) Capabilities() engine.Capabilities {
	return engine.Capabilities{Streaming: true}
}
func (e piCompletedAnswerFixture) Cancel(context.Context, string) error { return engine.ErrUnsupported }
func (e piCompletedAnswerFixture) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return engine.ErrUnsupported
}
func (e piCompletedAnswerFixture) Execute(ctx context.Context, _ engine.Request, sink func(engine.Event) error) error {
	if err := sink(engine.Event{Kind: engine.SessionStarted, SessionID: "pi-cli-long-answer"}); err != nil {
		return err
	}
	for offset := 0; offset < len(e.text); offset += 16 << 10 {
		if err := ctx.Err(); err != nil {
			return err
		}
		end := min(offset+(16<<10), len(e.text))
		if err := sink(engine.Event{Kind: engine.MessageDelta, ID: "assistant-completed", Text: e.text[offset:end]}); err != nil {
			return err
		}
	}
	return sink(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
}

func TestFinalAnswerReadsWholeLargeSafePiCompletionFromCommittedDeltas(t *testing.T) {
	text := strings.Repeat("x", (16<<10)-6) + "[REDACTED]" + strings.Repeat("y", 64<<10) + "[REDACTED]FINAL-TAIL"
	app, err := bootstrap.Open(t.Context(), bootstrap.Config{OpenStores: bootstrap.LocalStores, DataDirectory: filepath.Join(t.TempDir(), "data"), Demo: true})
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := app.Close(ctx); err != nil {
			t.Error(err)
		}
	}()
	if err := app.Runtime.RegisterDescriptor("pi", engine.DescribeRuntime(piCompletedAnswerFixture{text})); err != nil {
		t.Fatal(err)
	}
	session, err := app.Service.Create(t.Context(), "safe complete Pi answer", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	target := hub.Target{EngineID: "pi", ModelID: "engine-default", ProviderID: "engine-default", AccountSource: "local completed-answer fixture", BillingRoute: "local fixture"}
	branch, err := app.Store.Switch(t.Context(), session.ID, target, "explicit Pi final-answer consumer fixture")
	if err != nil {
		t.Fatal(err)
	}
	turn, done, err := app.Runtime.StartOnBranch(t.Context(), session.ID, branch.ID, "emit the completed safe answer")
	if err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("Pi final-answer fixture did not finish")
	}
	command := cli{app: app, session: session.ID}
	actual, err := command.finalAnswer(t.Context(), turn, 0)
	if err != nil || actual != text {
		t.Fatal("CLI lost or repeated a safe Pi completion larger than 64 KiB", len(actual), len(text), err)
	}
}

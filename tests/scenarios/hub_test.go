// INPUT: apps/hub/bootstrap real assembly with local stores, product HubService/Transcripts/Traces and a fakeModel endpoint
// OUTPUT: scenario harness: open/reopen one data directory, wait for settled Turns, read blocks, trace steps, history and pending interactions
// POS: [scenario harness] Drives the Hub only through the product entry points the desktop and HTTP server use.
package scenarios

import (
	"context"
	"encoding/json"
	"fmt"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

// hubFixture is one data directory and the assembly recipe reopened on it.
type hubFixture struct {
	t      *testing.T
	config bootstrap.Config
	app    *bootstrap.Application
}

func liveConfig(t *testing.T, model *fakeModel) bootstrap.Config {
	return bootstrap.Config{OpenStores: bootstrap.LocalStores, OpenMemory: bootstrap.LocalMemory, OpenBindings: bootstrap.LocalBindings,
		DataDirectory: filepath.Join(t.TempDir(), "data"), ProviderKind: model.kind,
		Provider: provider.Config{Endpoint: model.endpoint(), Model: "scenario-model", APIKey: "scenario-key"}}
}

func demoConfig(t *testing.T) bootstrap.Config {
	return bootstrap.Config{OpenStores: bootstrap.LocalStores, OpenMemory: bootstrap.LocalMemory, OpenBindings: bootstrap.LocalBindings,
		DataDirectory: filepath.Join(t.TempDir(), "data"), Demo: true}
}

func openHub(t *testing.T, config bootstrap.Config) *hubFixture {
	t.Helper()
	h := &hubFixture{t: t, config: config}
	h.open()
	t.Cleanup(func() {
		if h.app != nil {
			h.close()
		}
	})
	return h
}

func (h *hubFixture) open() {
	h.t.Helper()
	app, err := bootstrap.Open(context.Background(), h.config)
	if err != nil {
		h.t.Fatal("open Hub assembly:", err)
	}
	h.app = app
}

func (h *hubFixture) close() {
	h.t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := h.app.Close(ctx); err != nil {
		h.t.Fatal("close Hub assembly:", err)
	}
	h.app = nil
}

func (h *hubFixture) reopen() {
	h.t.Helper()
	h.close()
	h.open()
}

func (h *hubFixture) service() *product.HubService { return h.app.Service }

// createDefault creates a session exactly as the product does, so durable
// waits keep their default (on).
func (h *hubFixture) createDefault(title, workspace string) hub.Session {
	h.t.Helper()
	s, err := h.service().Create(context.Background(), title, workspace)
	if err != nil {
		h.t.Fatal("create session:", err)
	}
	return s
}

// create makes a session whose durable waits are turned off, so scenarios
// about live approvals and failures run the in-process path. Scenarios about
// waiting across a restart use createDefault or enable waits explicitly.
func (h *hubFixture) create(title, workspace string) hub.Session {
	h.t.Helper()
	s := h.createDefault(title, workspace)
	if err := h.service().EnableDurableWaits(context.Background(), s.ID, false); err != nil {
		h.t.Fatal("turn durable waits off:", err)
	}
	return s
}

func (h *hubFixture) send(id, prompt string) string {
	h.t.Helper()
	turn, err := h.service().Submit(context.Background(), id, prompt, nil)
	if err != nil {
		h.t.Fatal("send:", err)
	}
	return turn
}

func (h *hubFixture) detail(id string) hub.Detail {
	h.t.Helper()
	d, err := h.app.Store.Detail(context.Background(), id)
	if err != nil {
		h.t.Fatal("detail:", err)
	}
	return d
}

func (h *hubFixture) branch(id string) hub.Branch {
	h.t.Helper()
	b, err := product.ActiveBranch(h.detail(id))
	if err != nil {
		h.t.Fatal(err)
	}
	return b
}

// settle waits until the session's Turn and its physical owner both ended.
func (h *hubFixture) settle(id string) engine.Outcome {
	h.t.Helper()
	deadline := time.Now().Add(30 * time.Second)
	for time.Now().Before(deadline) {
		d := h.detail(id)
		if d.Session.Status != hub.Running && !h.app.Runtime.SessionActive(id) {
			return d.Session.Status
		}
		time.Sleep(5 * time.Millisecond)
	}
	h.t.Fatalf("session %s still running: %s", id, h.dump(id))
	return ""
}

func (h *hubFixture) expect(id string, want engine.Outcome) {
	h.t.Helper()
	if got := h.settle(id); got != want {
		h.t.Fatalf("session ended %s, want %s: %s", got, want, h.dump(id))
	}
}

// history reads committed product history page by page (test-only full read).
func (h *hubFixture) history(id string) []hub.HistoryEntry {
	h.t.Helper()
	var out []hub.HistoryEntry
	var after uint64
	for {
		page, err := h.app.Store.History(context.Background(), id, after, 128)
		if err != nil {
			h.t.Fatal(err)
		}
		out = append(out, page...)
		if len(page) < 128 {
			return out
		}
		after = page[len(page)-1].Sequence
	}
}

// dump renders committed history compactly: one line per event.
func (h *hubFixture) dump(id string) string {
	var out strings.Builder
	for _, e := range h.history(id) {
		line := fmt.Sprintf("\n%d %s %s", e.Sequence, e.Event.Kind, e.Event.Name)
		if e.Input != "" {
			line += " input=" + e.Input
		}
		if e.Event.Outcome != "" {
			line += " outcome=" + string(e.Event.Outcome)
		}
		if e.Event.Failure != "" {
			line += " failure=" + string(e.Event.Failure)
		}
		if e.Timeline != nil {
			data, _ := json.Marshal(e.Timeline)
			line += " timeline=" + string(data)
		}
		if text := e.Event.Text; text != "" {
			if len(text) > 300 {
				text = text[:300] + "..."
			}
			line += " text=" + text
		}
		out.WriteString(line)
	}
	return out.String()
}

// blocks projects the Message Stream tail exactly as the desktop does.
func (h *hubFixture) blocks(id string) []product.Block {
	h.t.Helper()
	chunk, err := h.service().Transcripts.Tail(context.Background(), id)
	if err != nil {
		h.t.Fatal("transcript tail:", err)
	}
	w := product.NewTranscriptWindow(0)
	w.Reset(chunk)
	return w.Blocks()
}

func blocksOf(blocks []product.Block, kind product.BlockKind) []product.Block {
	var out []product.Block
	for _, b := range blocks {
		if b.Kind == kind {
			out = append(out, b)
		}
	}
	return out
}

func toolBlock(t *testing.T, blocks []product.Block, callID string) product.Block {
	t.Helper()
	for _, b := range blocks {
		if b.Kind == product.BlockKindTool && (b.ID == b.AttemptID+"/"+callID) {
			return b
		}
	}
	t.Fatalf("no tool block for call %s in %d blocks", callID, len(blocks))
	return product.Block{}
}

// trace reads the execution trace tail of the session's active branch.
func (h *hubFixture) trace(id string) []product.TraceStep {
	h.t.Helper()
	b := h.branch(id)
	chunk, err := h.service().Traces.Tail(context.Background(), product.TraceTarget{SessionID: id, BranchID: b.ID})
	if err != nil {
		h.t.Fatal("trace tail:", err)
	}
	w := product.NewTraceWindow(0)
	w.Reset(chunk)
	return w.Steps()
}

// pending polls the unified interaction read until one item appears.
func (h *hubFixture) pending(id string) product.Interaction {
	h.t.Helper()
	deadline := time.Now().Add(10 * time.Second)
	for time.Now().Before(deadline) {
		items, err := h.service().PendingInteractions(context.Background(), id)
		if err != nil {
			h.t.Fatal(err)
		}
		if len(items) > 0 {
			return items[0]
		}
		if d := h.detail(id); d.Session.Status != hub.Running {
			h.t.Fatalf("session settled %s without a pending interaction: %s", d.Session.Status, h.dump(id))
		}
		time.Sleep(2 * time.Millisecond)
	}
	h.t.Fatalf("no pending interaction: %s", h.dump(id))
	return product.Interaction{}
}

// errorEvents returns the fatal engine errors committed for the session.
func errorEvents(history []hub.HistoryEntry) []engine.Event {
	var out []engine.Event
	for _, e := range history {
		if e.Event.Kind == engine.Error {
			out = append(out, e.Event)
		}
	}
	return out
}

func waitFor(t *testing.T, what string, ready func() bool) {
	t.Helper()
	deadline := time.Now().Add(15 * time.Second)
	for time.Now().Before(deadline) {
		if ready() {
			return
		}
		time.Sleep(2 * time.Millisecond)
	}
	t.Fatal("timed out waiting for", what)
}

func received(t *testing.T, ch <-chan struct{}, what string) {
	t.Helper()
	select {
	case <-ch:
	case <-time.After(15 * time.Second):
		t.Fatal("timed out waiting for", what)
	}
}

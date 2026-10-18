// INPUT: context, encoding/json, errors, http/httptest, os, filepath, strings, testing, time, SDK and native session storage.
// OUTPUT: Actual Chrome SDK/HITL, durable-risk admission, source references, identity and complete lifecycle regressions.
// POS: [L4 extension] Real local fixtures prove browser business behavior without paid services, external accounts or existing user browser profiles.
package browser

import (
	securityplugin "github.com/QuincySx/Alva/extension/security"
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	host "github.com/QuincySx/Alva/platform/host/browser"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const explicitChrome = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"

type fixture struct {
	repo      *session.Repository
	log       *session.Session
	cwd, temp string
	guard     *security.Guard
	approvals int
}

func browserFixture(t *testing.T, allow bool) *fixture {
	t.Helper()
	if _, err := os.Stat(explicitChrome); err != nil {
		t.Skip("explicit fixture Chrome unavailable; no discovery/install")
	}
	repo, err := session.Open(context.Background(), filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	log, err := repo.Create(context.Background(), "owner", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	f := &fixture{repo: repo, log: log, cwd: t.TempDir(), temp: t.TempDir()}
	os.Chmod(f.temp, 0700)
	f.guard, err = security.NewGuard(f.cwd, nil, nil, security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(context.Context, security.ApprovalRequest) (security.ApprovalDecision, error) {
		f.approvals++
		return security.ApprovalDecision{Allowed: allow, Scope: security.Once}, nil
	})))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := repo.Close(); err != nil {
			t.Error(err)
		}
	})
	return f
}
func (f *fixture) plugin(t *testing.T, report func(context.Context, Activity) error) *Plugin {
	t.Helper()
	if report == nil {
		report = func(context.Context, Activity) error { return nil }
	}
	p, err := New(context.Background(), Config{Repository: f.repo, Owner: "owner", SessionID: "native", Cwd: f.cwd, Guard: f.guard, ApprovedHostAccess: true, Launch: host.Config{Executable: explicitChrome, Cwd: f.cwd, TempRoot: f.temp, Headless: true}, Report: report})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := p.Close(ctx); err != nil {
			t.Error(err)
		}
	})
	return p
}

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (f modelFunc) Stream(ctx context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return f(ctx, r, emit)
}
func discard(abi.Event) error { return nil }

func TestRealBrowserRecipeRunsCatalogWithHITLAndReferenceOnlyEvidence(t *testing.T) {
	f := browserFixture(t, true)
	page := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		w.Write([]byte(`<html><head><title>Browser source</title></head><body><input id="input"><button id="button">Go</button><article>` + strings.Repeat("page-body-日志🙂 ", 10000) + `evidence-tail🙂</article></body></html>`))
	}))
	defer page.Close()
	reports := 0
	p := f.plugin(t, func(_ context.Context, a Activity) error {
		reports++
		if a.SessionID != "native" || a.RunID == "" || a.CallID == "" {
			return errors.New("wrong prior risk identity")
		}
		return nil
	})
	before, _ := os.ReadDir(f.temp)
	if len(before) != 0 {
		t.Fatal("constructor starts browser")
	}
	calls := []wire.ToolCall{{ID: "start", Name: "browser_start", Arguments: json.RawMessage(`{}`)}, {ID: "nav", Name: "browser_navigate", Arguments: json.RawMessage(`{"url":` + strconvJSON(page.URL) + `}`)}, {ID: "type", Name: "browser_action", Arguments: json.RawMessage(`{"action":"type","selector":"#input","text":"actual text🙂"}`)}, {ID: "click", Name: "browser_action", Arguments: json.RawMessage(`{"action":"click","selector":"#button"}`)}, {ID: "snapshot", Name: "browser_snapshot", Arguments: json.RawMessage(`{"mode":"text"}`)}, {ID: "screenshot", Name: "browser_screenshot", Arguments: json.RawMessage(`{"format":"png"}`)}, {ID: "status", Name: "browser_status", Arguments: json.RawMessage(`{"id":"*"}`)}}
	turn := 0
	model := modelFunc(func(_ context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		data, _ := json.Marshal(r.Messages)
		if strings.Contains(string(data), "page-body-日志") || strings.Contains(string(data), "evidence-tail") {
			return errors.New("raw browser page fed into model context")
		}
		for _, m := range r.Messages {
			for _, b := range m.Content {
				if b.Kind == wire.ContentImage {
					return errors.New("browser image automatically delivered")
				}
			}
		}
		if turn < len(calls) {
			call := calls[turn]
			turn++
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &call}); err != nil {
				return err
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, f.log).Workspace(f.cwd).MaxIterations(16).Plugin(p).Plugin(securityplugin.NewSecurityPlugin(f.guard, p.Tools())).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if len(agent.Snapshot().Tools) != 7 {
		t.Fatal("real browser catalog missing", agent.Snapshot())
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	if err = agent.Run(ctx, []wire.Message{wire.TextMessage(wire.RoleUser, "use explicit local fixture")}, discard); err != nil {
		t.Fatal(err)
	}
	if f.approvals != 6 || reports != 7 {
		t.Fatal("actual tool HITL duplicated/skipped or prior activity missing", f.approvals, reports)
	}
	after, _ := os.ReadDir(f.temp)
	if len(after) != 0 || p.manager != nil || !p.CleanupComplete() {
		t.Fatal("normal run left unowned browser/profile", after)
	}
	records, err := f.log.Records(context.Background(), 0)
	if err != nil {
		t.Fatal(err)
	}
	intents, outcomes, processIO, cleanupFacts := 0, 0, 0, 0
	var source wire.Reference
	imageFound := false
	for _, record := range records {
		if record.Namespace == "alva.browser" {
			switch record.Type {
			case "browser_intent":
				intents++
			case "browser_outcome":
				outcomes++
			case "browser_process_io":
				processIO++
			case "browser_cleanup":
				var receipt struct {
					Complete bool   `json:"cleanup_complete"`
					Stage    string `json:"stage"`
				}
				if json.Unmarshal(record.Payload, &receipt) != nil || !receipt.Complete || receipt.Stage != "run_end" {
					t.Fatal("false/missing durable cleanup evidence", string(record.Payload))
				}
				cleanupFacts++
			}
		}
		if record.ToolOutcome == nil {
			continue
		}
		if len(record.ToolOutcome.Result.Content) != 1 || len(record.ToolOutcome.Result.Content[0].Text) > 4096 {
			t.Fatal("tool returned unbounded body")
		}
		var body struct {
			Source wire.Reference `json:"source"`
			Status string         `json:"status"`
			Format string         `json:"format"`
			Width  int            `json:"width"`
		}
		json.Unmarshal([]byte(record.ToolOutcome.Result.Content[0].Text), &body)
		if record.ToolOutcome.CallID == "snapshot" {
			source = body.Source
		}
		if record.ToolOutcome.CallID == "screenshot" {
			imageFound = body.Status == "image_retained_not_delivered" && body.Format == "png" && body.Width > 0 && body.Source.URI != ""
		}
	}
	if intents != 7 || outcomes != 7 || processIO == 0 || source.URI == "" || !imageFound || cleanupFacts != 1 {
		t.Fatal("missing actual operation/source evidence", intents, outcomes, processIO, source, imageFound)
	}
	refs, _ := f.repo.BoundReferences(context.Background(), "owner", "native")
	matches, err := refs.SearchText(context.Background(), source.URI, "evidence-tail🙂", 1)
	if err != nil || len(matches) != 1 {
		t.Fatal("full original source tail lost", matches, err)
	}
	other, _ := f.repo.Create(context.Background(), "owner", "sibling", "")
	if _, err = f.repo.ReadArtifact(context.Background(), "owner", other.ID(), source.URI, 0, 2048); !errors.Is(err, session.ErrNotFound) {
		t.Fatal("sibling source grant", err)
	}
}
func strconvJSON(text string) string { data, _ := json.Marshal(text); return string(data) }

func TestBrowserDenialRiskFailureAndForgedIdentityCannotStart(t *testing.T) {
	for _, kind := range []string{"deny", "risk", "forged"} {
		t.Run(kind, func(t *testing.T) {
			f := browserFixture(t, kind != "deny")
			failure := errors.New("durable risk rejected")
			p := f.plugin(t, func(context.Context, Activity) error {
				if kind == "risk" {
					return failure
				}
				return nil
			})
			state, _ := abi.NewAgentState(modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { return nil }), f.log, p.Tools())
			runtime := abi.RuntimeContext{RunID: "run", State: state, Workspace: f.cwd}
			if err := p.OnRunStart(context.Background(), runtime); err != nil {
				t.Fatal(err)
			}
			defer p.OnRunEnd(context.Background(), runtime, nil)
			tool := p.Tools()[0]
			actual := tool
			if kind == "forged" {
				actual = &browserTool{plugin: p, name: tool.Name()}
			}
			execution := abi.ToolExecutionContext{Session: f.log, SessionID: "native", RunID: "run", CallID: "call", Workspace: f.cwd, ActualTool: actual}
			_, err := tool.Execute(context.Background(), execution, wire.ToolCall{ID: "call", Name: tool.Name(), Arguments: json.RawMessage(`{}`)})
			if err == nil {
				t.Fatal("unapproved browser started")
			}
			if kind == "risk" && !errors.Is(err, failure) {
				t.Fatal("risk failure hidden", err)
			}
			entries, _ := os.ReadDir(f.temp)
			if len(entries) != 0 {
				t.Fatal("failed admission leaked profile/process", entries)
			}
		})
	}
}

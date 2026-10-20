// INPUT: context, encoding/json, errors, io, os, filepath, strings, testing, time, agent/core/security and native SQLite.
// OUTPUT: Real diagnostic command, HITL, source/exit, bounds, cancellation and manager capability regressions.
// POS: [L4 extension] Actual SDK plus temporary Go subprocess acceptance never fabricates zero diagnostics or starts ambient language servers.
package diagnostics

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	securityplugin "github.com/QuincySx/Alva/extension/security"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/platform/system/process"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestDiagnosticsHelper(t *testing.T) {
	mode := ""
	for _, arg := range os.Args {
		if strings.HasPrefix(arg, "--diagnostics-helper=") {
			mode = strings.TrimPrefix(arg, "--diagnostics-helper=")
		}
	}
	if mode == "" {
		return
	}
	if os.Getenv("ALVA_HOST_SECRET") != "" {
		os.Exit(90)
	}
	io.ReadAll(os.Stdin)
	if marker := os.Getenv("DIAGNOSTIC_MARKER"); marker != "" {
		if os.WriteFile(marker, []byte("started"), 0600) != nil {
			os.Exit(91)
		}
	}
	switch mode {
	case "issues":
		io.WriteString(os.Stdout, strings.Repeat("file.go:1:1: diagnostic🙂\n", 10000)+"END_EVIDENCE🙂")
		io.WriteString(os.Stderr, "compiler completed with actual issues")
		os.Exit(1)
	case "zero":
		os.Exit(0)
	case "slow":
		for {
			time.Sleep(time.Hour)
		}
	case "flood":
		io.WriteString(os.Stdout, strings.Repeat("X", 2<<20))
		os.Exit(0)
	case "secrets":
		io.WriteString(os.Stdout, `{"api_key":"fixture-secret","safe":"kept"}`)
		os.Exit(0)
	case "environment":
		io.WriteString(os.Stdout, os.Getenv("API_KEY"))
		io.WriteString(os.Stderr, os.Getenv("API_KEY"))
		os.Exit(0)
	default:
		os.Exit(92)
	}
}

func TestDiagnosticConfiguredCredentialIsAbsentFromBothActualStreams(t *testing.T) {
	f := newFixture(t, true)
	profile := f.profile(t, "environment")
	const secret = "diagnostic-opaque-environment-credential"
	profile.Process.Env = map[string]string{"API_KEY": secret}
	p := f.plugin(t, profile, nil)
	if err := build(t, f, p, model(`{"action":"run","profile_id":"approved"}`, nil)).Run(context.Background(), nil, discard); err != nil {
		t.Fatal(err)
	}
	report := outcome(t, f)
	refs, _ := f.repo.BoundReferences(context.Background(), "owner", "native")
	for _, ref := range []wire.Reference{report.Stdout, report.Stderr} {
		page, err := refs.ReadText(context.Background(), ref.URI, 0, 4096)
		if err != nil || strings.Contains(page.Text, secret) || page.Text != "[REDACTED]" {
			t.Fatal("unlabelled configured credential retained", page, err)
		}
	}
	partial, err := p.retain(context.Background(), "approved", "stderr", []byte("public line\n"+secret[:10]), false)
	if err != nil {
		t.Fatal(err)
	}
	page, err := refs.ReadText(context.Background(), partial.URI, 0, 4096)
	if err != nil || page.Text != "public line\n" || !strings.Contains(partial.Processing, "incomplete_tail_omitted_bytes=10") {
		t.Fatal("incomplete credential prefix retained", partial, page, err)
	}
}

type fixture struct {
	repo      storage.Repository
	log       storage.Session
	cwd       string
	guard     *security.Guard
	approvals int
}

func newFixture(t *testing.T, allow bool) *fixture {
	t.Helper()
	ctx := context.Background()
	repo, err := session.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	log, err := repo.Create(ctx, "owner", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	f := &fixture{repo: repo, log: log, cwd: t.TempDir()}
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
func (f *fixture) profile(t *testing.T, mode string) Profile {
	t.Helper()
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	return Profile{ID: "approved", Description: "Explicit local diagnostic fixture", ApprovedHostAccess: true, Timeout: 3 * time.Second, Process: process.Config{Command: exe, Args: []string{"-test.run=^TestDiagnosticsHelper$", "--", "--diagnostics-helper=" + mode}, Cwd: f.cwd}}
}
func (f *fixture) plugin(t *testing.T, profile Profile, report func(context.Context, Activity) error) *Plugin {
	t.Helper()
	if report == nil {
		report = func(context.Context, Activity) error { return nil }
	}
	p, err := New(context.Background(), Config{Repository: f.repo, Owner: "owner", SessionID: "native", Cwd: f.cwd, Guard: f.guard, Profiles: []Profile{profile}, Report: report})
	if err != nil {
		t.Fatal(err)
	}
	return p
}

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (fn modelFunc) Stream(ctx context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return fn(ctx, r, emit)
}
func model(args string, verify func(abi.ModelRequest) error) abi.LanguageModel {
	turn := 0
	return modelFunc(func(_ context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		if verify != nil {
			if err := verify(r); err != nil {
				return err
			}
		}
		if turn == 1 {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "call", Name: "project_diagnostics", Arguments: json.RawMessage(args)}}); err != nil {
				return err
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
}
func build(t *testing.T, f *fixture, p *Plugin, m abi.LanguageModel) *core.Agent {
	t.Helper()
	agent, err := core.NewBuilder(m, f.log).Workspace(f.cwd).Plugin(p).Plugin(securityplugin.NewSecurityPlugin(f.guard, []abi.Tool{p.Tool()})).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	return agent
}
func discard(abi.Event) error { return nil }
func outcome(t *testing.T, f *fixture) Report {
	t.Helper()
	records, err := f.log.Records(context.Background(), 0)
	if err != nil {
		t.Fatal(err)
	}
	for _, record := range records {
		if record.Namespace == "alva.tooling" && record.Type == "diagnostic_outcome" {
			var report Report
			if json.Unmarshal(record.Payload, &report) != nil {
				t.Fatal("bad report")
			}
			return report
		}
	}
	t.Fatal("no actual diagnostic outcome")
	return Report{}
}

func TestDiagnosticsActuallyRunsWithApprovalAndRetainedCompleteSource(t *testing.T) {
	t.Setenv("ALVA_HOST_SECRET", "must-not-inherit")
	f := newFixture(t, true)
	profile := f.profile(t, "issues")
	marker := filepath.Join(f.cwd, "marker")
	profile.Process.Env = map[string]string{"DIAGNOSTIC_MARKER": marker}
	reports := 0
	p := f.plugin(t, profile, func(_ context.Context, a Activity) error {
		reports++
		if a.Effect != abi.EffectUnknown || a.SessionID != "native" || a.CallID != "call" {
			return errors.New("wrong pre-start activity")
		}
		if _, err := os.Stat(marker); !errors.Is(err, os.ErrNotExist) {
			return errors.New("activity arrived after start")
		}
		return nil
	})
	if _, err := os.Stat(marker); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("constructor started process")
	}
	agent := build(t, f, p, model(`{"action":"run","profile_id":"approved"}`, func(r abi.ModelRequest) error {
		data, _ := json.Marshal(r.Messages)
		if strings.Contains(string(data), "file.go:1:1") {
			return errors.New("raw diagnostic body entered model context")
		}
		return nil
	}))
	manager, ok := bus.Resolve[Manager](agent.Bus())
	if !ok || manager != p.manager {
		t.Fatal("actual Tool manager capability missing")
	}
	if err := agent.Run(context.Background(), nil, discard); err != nil {
		t.Fatal(err)
	}
	report := outcome(t, f)
	if f.approvals != 1 || reports != 1 || report.ExitCode != 1 || report.Status != "nonzero_exit" || !report.Started || !report.Reaped || !report.CapturedComplete || report.DiagnosticsParsed {
		t.Fatal("fake/no execution or wrong evidence", report, f.approvals, reports)
	}
	refs, _ := f.repo.BoundReferences(context.Background(), "owner", "native")
	matches, err := refs.SearchText(context.Background(), report.Stdout.URI, "END_EVIDENCE🙂", 1)
	if err != nil || len(matches) != 1 {
		t.Fatal("captured source tail missing", matches, err)
	}
	other, _ := f.repo.Create(context.Background(), "owner", "sibling", "")
	if _, err = f.repo.ReadArtifact(context.Background(), "owner", other.ID(), report.Stdout.URI, 0, 1024); !errors.Is(err, storage.ErrNotFound) {
		t.Fatal("sibling diagnostic source access", err)
	}
}
func TestDiagnosticZeroExitDoesNotClaimParsedZeroIssuesAndSecretsRedact(t *testing.T) {
	for _, mode := range []string{"zero", "secrets"} {
		t.Run(mode, func(t *testing.T) {
			f := newFixture(t, true)
			p := f.plugin(t, f.profile(t, mode), nil)
			if err := build(t, f, p, model(`{"action":"run","profile_id":"approved"}`, nil)).Run(context.Background(), nil, discard); err != nil {
				t.Fatal(err)
			}
			report := outcome(t, f)
			if report.DiagnosticsParsed || report.ExitCode != 0 || !report.CapturedComplete || !strings.Contains(report.SourceNotice, "not proof of zero issues") {
				t.Fatal("zero exit falsely claimed clean diagnostics", report)
			}
			if mode == "secrets" {
				refs, _ := f.repo.BoundReferences(context.Background(), "owner", "native")
				page, err := refs.ReadText(context.Background(), report.Stdout.URI, 0, 4096)
				if err != nil || strings.Contains(page.Text, "fixture-secret") || !strings.Contains(page.Text, "kept") {
					t.Fatal("known structured credential leaked", page, err)
				}
			}
		})
	}
}
func TestDiagnosticTimeoutAndOverflowAreFatalNotSuccessfulPartialResults(t *testing.T) {
	for _, mode := range []string{"slow", "flood"} {
		t.Run(mode, func(t *testing.T) {
			f := newFixture(t, true)
			profile := f.profile(t, mode)
			if mode == "slow" {
				profile.Timeout = 100 * time.Millisecond
			}
			p := f.plugin(t, profile, nil)
			err := build(t, f, p, model(`{"action":"run","profile_id":"approved"}`, nil)).Run(context.Background(), nil, discard)
			if err == nil || abi.OnlyCancellation(err) {
				t.Fatal("own command failure hidden as success/user cancellation", err)
			}
			report := outcome(t, f)
			if report.Status != "failed" || !report.Reaped || report.CapturedComplete {
				t.Fatal("failed/partial capture looked complete", report)
			}
		})
	}
}
func TestDiagnosticsDenialOrActivityFailureStartsNoProcess(t *testing.T) {
	for _, deny := range []bool{true, false} {
		f := newFixture(t, !deny)
		profile := f.profile(t, "zero")
		marker := filepath.Join(f.cwd, "marker")
		profile.Process.Env = map[string]string{"DIAGNOSTIC_MARKER": marker}
		failure := errors.New("activity store failed")
		p := f.plugin(t, profile, func(context.Context, Activity) error { return failure })
		err := build(t, f, p, model(`{"action":"run","profile_id":"approved"}`, nil)).Run(context.Background(), nil, discard)
		if err == nil {
			t.Fatal("denial/report failure ignored")
		}
		if !deny && !errors.Is(err, failure) {
			t.Fatal(err)
		}
		if _, err = os.Stat(marker); !errors.Is(err, os.ErrNotExist) {
			t.Fatal("unauthorized process started")
		}
	}
}
func TestManagerCapabilityCannotTurnReadOnlyCatalogCallIntoExecution(t *testing.T) {
	f := newFixture(t, true)
	p := f.plugin(t, f.profile(t, "zero"), nil)
	state, _ := abi.NewAgentState(model(`{"action":"list"}`, nil), f.log, []abi.Tool{p.Tool()})
	if err := p.OnRunStart(context.Background(), abi.RuntimeContext{RunID: "run", Workspace: f.cwd, State: state}); err != nil {
		t.Fatal(err)
	}
	defer p.OnRunEnd(context.Background(), abi.RuntimeContext{RunID: "run"}, nil)
	execution := abi.ToolExecutionContext{SessionID: "native", Session: f.log, RunID: "run", CallID: "call", Workspace: f.cwd, ActualTool: p.Tool()}
	call := wire.ToolCall{ID: "call", Name: "project_diagnostics", Arguments: json.RawMessage(`{"action":"list"}`)}
	if _, err := p.manager.Run(context.Background(), Request{ProfileID: "approved", Execution: execution, Call: call}); err == nil {
		t.Fatal("manager ran command under read-only list authorization")
	}
	if f.approvals != 0 {
		t.Fatal("invalid request reached authorization")
	}
	call.Arguments = json.RawMessage(`{"action":"run","profile_id":"approved"}`)
	execution.ActualTool = &diagnosticTool{plugin: p}
	if _, err := p.manager.Run(context.Background(), Request{ProfileID: "approved", Execution: execution, Call: call}); err == nil {
		t.Fatal("copied actual-tool identity accepted")
	}
}

type managerFixture struct {
	infos  []ProfileInfo
	called int
	run    func(Request) (Report, error)
}

func (m *managerFixture) Catalog() []ProfileInfo { return m.infos }
func (m *managerFixture) Run(_ context.Context, r Request) (Report, error) {
	m.called++
	return m.run(r)
}

func TestReplaceableManagerIsReallyConsumedAndCannotReturnFakeEmptySuccess(t *testing.T) {
	for _, foreign := range []bool{false, true} {
		t.Run(map[bool]string{false: "empty", true: "foreign"}[foreign], func(t *testing.T) {
			f := newFixture(t, true)
			manager := &managerFixture{infos: []ProfileInfo{{ID: "approved", Description: "explicit host implementation"}}, run: func(r Request) (Report, error) {
				report := Report{SessionID: r.Execution.SessionID, RunID: r.Execution.RunID, CallID: r.Call.ID, ProfileID: r.ProfileID}
				if foreign {
					report.SessionID = "other-session"
				}
				return report, nil
			}}
			p, err := New(context.Background(), Config{Repository: f.repo, Owner: "owner", SessionID: "native", Cwd: f.cwd, Guard: f.guard, Manager: manager, Report: func(context.Context, Activity) error { return nil }})
			if err != nil {
				t.Fatal(err)
			}
			err = build(t, f, p, model(`{"action":"run","profile_id":"approved"}`, nil)).Run(context.Background(), nil, discard)
			if err == nil || manager.called != 1 {
				t.Fatal("manager seam was ignored or fabricated success", err, manager.called)
			}
		})
	}
}

func TestDiagnosticCatalogPaginatesWithoutStartingOrDroppingEntries(t *testing.T) {
	f := newFixture(t, true)
	var infos []ProfileInfo
	for index := range 16 {
		infos = append(infos, ProfileInfo{ID: "profile-" + string(rune('a'+index)), Description: strings.Repeat("\n", 512)})
	}
	manager := &managerFixture{infos: infos, run: func(Request) (Report, error) { t.Fatal("catalog started diagnostics"); return Report{}, nil }}
	p, err := New(context.Background(), Config{Repository: f.repo, Owner: "owner", SessionID: "native", Cwd: f.cwd, Guard: f.guard, Manager: manager, Report: func(context.Context, Activity) error { t.Fatal("catalog reported a start"); return nil }})
	if err != nil {
		t.Fatal(err)
	}
	manager.infos[0].ID = "changed-after-construction"
	state, _ := abi.NewAgentState(model(`{"action":"list"}`, nil), f.log, []abi.Tool{p.Tool()})
	if err = p.OnRunStart(context.Background(), abi.RuntimeContext{RunID: "run", Workspace: f.cwd, State: state}); err != nil {
		t.Fatal(err)
	}
	defer p.OnRunEnd(context.Background(), abi.RuntimeContext{RunID: "run"}, nil)
	execution := abi.ToolExecutionContext{SessionID: "native", Session: f.log, RunID: "run", CallID: "call", Workspace: f.cwd, ActualTool: p.Tool()}
	after := ""
	seen := map[string]bool{}
	for pages := 0; ; pages++ {
		if pages > 16 {
			t.Fatal("nonadvancing catalog cursor")
		}
		data, _ := json.Marshal(map[string]string{"action": "list", "after": after})
		result, err := p.Tool().Execute(context.Background(), execution, wire.ToolCall{ID: "call", Name: "project_diagnostics", Arguments: data})
		if err != nil {
			t.Fatal(err)
		}
		body := result.Content[0].Text
		var page struct {
			Profiles []ProfileInfo `json:"profiles"`
			Next     string        `json:"next"`
			HasMore  bool          `json:"has_more"`
		}
		if len(body) > 4096 || json.Unmarshal([]byte(body), &page) != nil || len(page.Profiles) < 1 || len(page.Profiles) > 5 {
			t.Fatal("unbounded or empty catalog page", body)
		}
		for _, info := range page.Profiles {
			if seen[info.ID] || info.ID == "changed-after-construction" || len(info.Description) != 512 {
				t.Fatal("catalog snapshot mutated, duplicated or truncated", info)
			}
			seen[info.ID] = true
		}
		if !page.HasMore {
			break
		}
		after = page.Next
	}
	if len(seen) != 16 || manager.called != 0 || f.approvals != 0 {
		t.Fatal("catalog lost entries or executed", seen, manager.called, f.approvals)
	}
}

func TestQueuedManagerCannotLaunchAfterItsRunEnds(t *testing.T) {
	f := newFixture(t, true)
	profile := f.profile(t, "zero")
	marker := filepath.Join(f.cwd, "stale-command-marker")
	profile.Process.Env = map[string]string{"DIAGNOSTIC_MARKER": marker}
	p := f.plugin(t, profile, func(context.Context, Activity) error {
		t.Error("stale waiter reached command activity")
		return errors.New("stale command admission")
	})
	state, _ := abi.NewAgentState(model(`{"action":"list"}`, nil), f.log, []abi.Tool{p.Tool()})
	runtime := abi.RuntimeContext{RunID: "old-run", Workspace: f.cwd, State: state}
	if err := p.OnRunStart(context.Background(), runtime); err != nil {
		t.Fatal(err)
	}
	p.gate <- struct{}{}
	execution := abi.ToolExecutionContext{SessionID: "native", Session: f.log, RunID: "old-run", CallID: "call", Workspace: f.cwd, ActualTool: p.Tool()}
	call := wire.ToolCall{ID: "call", Name: "project_diagnostics", Arguments: json.RawMessage(`{"action":"run","profile_id":"approved"}`)}
	entered, approved := make(chan struct{}), make(chan struct{})
	guard, err := security.NewGuard(f.cwd, nil, nil, security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(context.Context, security.ApprovalRequest) (security.ApprovalDecision, error) {
		close(entered)
		<-approved
		return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
	})))
	if err != nil {
		t.Fatal(err)
	}
	p.config.Guard = guard
	done := make(chan error, 1)
	go func() {
		_, err := p.manager.Run(context.Background(), Request{ProfileID: "approved", Execution: execution, Call: call})
		done <- err
	}()
	select {
	case <-entered: // Initial p.check definitely passed before the run changes.
	case <-time.After(5 * time.Second):
		t.Fatal("manager never entered old-run authorization")
	}
	if err := p.OnRunEnd(context.Background(), runtime, nil); err != nil {
		t.Fatal(err)
	}
	runtime.RunID = "new-run"
	if err := p.OnRunStart(context.Background(), runtime); err != nil {
		t.Fatal(err)
	}
	defer p.OnRunEnd(context.Background(), runtime, nil)
	close(approved)
	<-p.gate
	select {
	case err := <-done:
		if err == nil {
			t.Fatal("old-run queued manager launch succeeded")
		}
	case <-time.After(5 * time.Second):
		t.Fatal("stale waiter did not settle")
	}
	if _, err := os.Stat(marker); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("old-run queued command actually started", err)
	}
}

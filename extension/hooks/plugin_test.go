// INPUT: context, encoding/json, errors, io, os, filepath, strings, testing, time, SDK core/security and native session SQLite.
// OUTPUT: Actual approved Hook phases, effective authorization, reference-only context, errors and real process regressions.
// POS: [L4 extension] Temporary Go helper processes and actual SDK loops prove behavior without existing scripts, credentials or paid models.
package hooks

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
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestHooksHelper(t *testing.T) {
	mode := ""
	for _, arg := range os.Args {
		if strings.HasPrefix(arg, "--hooks-helper=") {
			mode = strings.TrimPrefix(arg, "--hooks-helper=")
		}
	}
	if mode == "" {
		return
	}
	if os.Getenv("ALVA_HOST_SECRET") != "" {
		os.Exit(90)
	}
	data, err := io.ReadAll(os.Stdin)
	if err != nil {
		os.Exit(91)
	}
	var input Input
	if json.Unmarshal(data, &input) != nil {
		os.Exit(92)
	}
	if marker := os.Getenv("HOOK_MARKER"); marker != "" {
		if os.WriteFile(marker, []byte(string(input.Event)), 0600) != nil {
			os.Exit(93)
		}
	}
	switch mode {
	case "context":
		json.NewEncoder(os.Stdout).Encode(Output{AdditionalContext: strings.Repeat("untrusted-hook-context🙂", 2000)})
	case "rewrite":
		json.NewEncoder(os.Stdout).Encode(Output{PermissionDecision: "allow", UpdatedInput: json.RawMessage(`{"path":"effective"}`)})
	case "block":
		io.WriteString(os.Stderr, strings.Repeat("blocking source🙂", 1000))
		os.Exit(2)
	case "failure":
		io.WriteString(os.Stderr, "dedicated-failure-hook")
		io.WriteString(os.Stdout, `{}`)
	case "success":
		io.WriteString(os.Stderr, "dedicated-success-hook")
		io.WriteString(os.Stdout, `{}`)
	case "nonblocking":
		io.WriteString(os.Stderr, "reported nonblocking error")
		os.Exit(3)
	case "malformed":
		io.WriteString(os.Stdout, `{"continue":`)
	case "null":
		io.WriteString(os.Stdout, `{"continue":null}`)
	case "unknown":
		io.WriteString(os.Stdout, `{"grant_all_permissions":true}`)
	case "flood":
		io.WriteString(os.Stdout, strings.Repeat("X", 2<<20))
	case "sleep":
		for {
			time.Sleep(time.Hour)
		}
	case "redact":
		io.WriteString(os.Stdout, `{"additional_context":"public","stop_reason":"API_KEY=private"}`)
		io.WriteString(os.Stderr, "Authorization: Bearer private")
	case "environment":
		json.NewEncoder(os.Stdout).Encode(Output{AdditionalContext: os.Getenv("API_KEY")})
		io.WriteString(os.Stderr, os.Getenv("API_KEY"))
	default:
		io.WriteString(os.Stdout, `{}`)
	}
	os.Exit(0)
}

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (fn modelFunc) Stream(ctx context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return fn(ctx, r, emit)
}

type fixtureTool struct {
	execute func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error)
	write   bool
}

func (*fixtureTool) Name() string                      { return "fixture" }
func (*fixtureTool) Description() string               { return "local fixture" }
func (*fixtureTool) ParametersSchema() json.RawMessage { return json.RawMessage(`{"type":"object"}`) }
func (t *fixtureTool) Execute(ctx context.Context, e abi.ToolExecutionContext, c wire.ToolCall) (wire.ToolResult, error) {
	return t.execute(ctx, e, c)
}
func (*fixtureTool) Scheduling(c wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true, Resources: []abi.ResourceClaim{{Resource: string(c.Arguments), Access: abi.AccessWrite}}}
}
func (t *fixtureTool) DescribeEffect(wire.ToolCall) abi.Effect {
	kind := abi.EffectRead
	if t.write {
		kind = abi.EffectWrite
	}
	return abi.Effect{Kind: kind, Description: "actual fixture effect"}
}
func (t *fixtureTool) IsReadOnly(wire.ToolCall) bool { return !t.write }

type fixture struct {
	repo storage.Repository
	log  storage.Session
	cwd  string
}

func newFixture(t *testing.T) *fixture {
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
	t.Cleanup(func() {
		if err := repo.Close(); err != nil {
			t.Error(err)
		}
	})
	return &fixture{repo: repo, log: log, cwd: t.TempDir()}
}
func (f *fixture) entry(t *testing.T, id string, event Event, mode string) Entry {
	t.Helper()
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	return Entry{ID: id, Event: event, ApprovedHostAccess: true, Process: process.Config{Command: exe, Args: []string{"-test.run=^TestHooksHelper$", "--", "--hooks-helper=" + mode}, Cwd: f.cwd}, Timeout: 3 * time.Second}
}
func (f *fixture) plugin(t *testing.T, entries []Entry, report func(context.Context, Activity) error) *Plugin {
	t.Helper()
	if report == nil {
		report = func(context.Context, Activity) error { return nil }
	}
	p, err := New(context.Background(), Config{Repository: f.repo, Owner: "owner", SessionID: f.log.ID(), Cwd: f.cwd, Entries: entries, Report: report})
	if err != nil {
		t.Fatal(err)
	}
	return p
}
func toolModel(verify func(abi.ModelRequest) error) abi.LanguageModel {
	turn := 0
	return modelFunc(func(_ context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		if verify != nil {
			if err := verify(r); err != nil {
				return err
			}
		}
		if turn == 1 {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "call", Name: "fixture", Arguments: json.RawMessage(`{"path":"declared"}`)}}); err != nil {
				return err
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
}
func build(t *testing.T, f *fixture, p *Plugin, model abi.LanguageModel, tool abi.Tool, extra ...core.Plugin) *core.Agent {
	t.Helper()
	b := core.NewBuilder(model, f.log).Workspace(f.cwd).Plugin(p)
	if tool != nil {
		b.Tool(tool)
	}
	for _, plugin := range extra {
		b.Plugin(plugin)
	}
	agent, err := b.Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	return agent
}
func discard(abi.Event) error { return nil }
func receipts(t *testing.T, f *fixture) []Receipt {
	t.Helper()
	records, err := f.log.Records(context.Background(), 0)
	if err != nil {
		t.Fatal(err)
	}
	var receipts []Receipt
	for _, record := range records {
		if record.Namespace == "alva.hooks" && record.Type == "hook_outcome" {
			var receipt Receipt
			if json.Unmarshal(record.Payload, &receipt) != nil {
				t.Fatal("bad receipt")
			}
			receipts = append(receipts, receipt)
		}
	}
	return receipts
}

func TestHookPhasesActuallyExecuteAndReauthorizeModifiedArguments(t *testing.T) {
	t.Setenv("ALVA_HOST_SECRET", "not-in-child")
	f := newFixture(t)
	marker := filepath.Join(f.cwd, "start-marker")
	start := f.entry(t, "start", SessionStart, "context")
	start.Process.Env = map[string]string{"HOOK_MARKER": marker}
	var reports int
	p := f.plugin(t, []Entry{start, f.entry(t, "rewrite", PreToolUse, "rewrite"), f.entry(t, "post", PostToolUse, "success")}, func(_ context.Context, a Activity) error {
		reports++
		if a.SessionID != "native" || a.RunID == "" || a.Effect != abi.EffectUnknown {
			return errors.New("wrong pre-start identity")
		}
		if reports == 1 {
			if _, err := os.Stat(marker); !errors.Is(err, os.ErrNotExist) {
				return errors.New("activity report arrived after process start")
			}
		}
		return nil
	})
	if _, err := os.Stat(marker); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("constructor starts hooks")
	}
	approvals, calls := 0, 0
	guard, err := security.NewGuard(f.cwd, nil, nil, security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(_ context.Context, a security.ApprovalRequest) (security.ApprovalDecision, error) {
		approvals++
		if !strings.Contains(a.InputSummary, "effective") {
			return security.ApprovalDecision{}, errors.New("approval saw original not effective args")
		}
		return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
	})))
	if err != nil {
		t.Fatal(err)
	}
	tool := &fixtureTool{write: true, execute: func(_ context.Context, e abi.ToolExecutionContext, c wire.ToolCall) (wire.ToolResult, error) {
		calls++
		if string(c.Arguments) != `{"path":"effective"}` || e.Scheduling.Resources[0].Resource != `{"path":"effective"}` {
			return wire.ToolResult{}, errors.New("modified input bypassed descriptor recompute")
		}
		return wire.ToolResult{CallID: c.ID}, nil
	}}
	model := toolModel(func(r abi.ModelRequest) error {
		data, _ := json.Marshal(r.Messages)
		if strings.Contains(string(data), "untrusted-hook-context") {
			return errors.New("raw additional_context fed to model")
		}
		return nil
	})
	agent := build(t, f, p, model, tool, securityplugin.NewSecurityPlugin(guard, []abi.Tool{tool}))
	if _, err := os.Stat(marker); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("registration starts hooks")
	}
	if err = agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "test")}, discard); err != nil {
		t.Fatal(err)
	}
	if reports != 3 || calls != 1 || approvals != 1 {
		t.Fatal("missing actual phase or permission allow bypass", reports, calls, approvals)
	}
	all := receipts(t, f)
	if len(all) != 3 || all[0].Context == nil || !all[0].Complete || !all[0].Reaped {
		t.Fatal("missing real context/process evidence", all)
	}
	refs, _ := f.repo.BoundReferences(context.Background(), "owner", "native")
	page, err := refs.ReadText(context.Background(), all[0].Context.URI, 0, 4096)
	if err != nil || !strings.Contains(page.Text, "untrusted-hook-context") {
		t.Fatal("source context not retained", page, err)
	}
	records, _ := f.log.Records(context.Background(), 0)
	for _, record := range records {
		if record.ToolIntent != nil {
			if string(record.ToolIntent.Call.Arguments) != `{"path":"effective"}` || record.ToolIntent.DeclaredCall == nil || string(record.ToolIntent.DeclaredCall.Arguments) != `{"path":"declared"}` {
				t.Fatal("original/effective facts conflated")
			}
		}
	}
	snapshot := agent.Snapshot()
	if len(snapshot.Plugins[0].Middleware) != 7 {
		t.Fatal("hooks did not use ordinary typed middleware contributions", snapshot)
	}
}
func TestPreToolBlockIsReferenceOnlyAndRunsDedicatedFailureHook(t *testing.T) {
	f := newFixture(t)
	p := f.plugin(t, []Entry{f.entry(t, "pre", PreToolUse, "block"), f.entry(t, "failure", PostToolUseFailure, "failure"), f.entry(t, "success", PostToolUse, "success")}, nil)
	calls := 0
	tool := &fixtureTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		calls++
		return wire.ToolResult{}, nil
	}}
	if err := build(t, f, p, toolModel(nil), tool).Run(context.Background(), nil, discard); err != nil {
		t.Fatal(err)
	}
	all := receipts(t, f)
	if calls != 0 || len(all) != 2 || all[0].Status != "blocked" || all[1].Event != PostToolUseFailure {
		t.Fatal("block falsely executed tool/success hook", calls, all)
	}
	records, _ := f.log.Records(context.Background(), 0)
	for _, record := range records {
		if record.ToolOutcome != nil {
			data, _ := json.Marshal(record.ToolOutcome.Result)
			if !record.ToolOutcome.Result.IsError || len(data) > 4096 || strings.Contains(string(data), "blocking source") {
				t.Fatal("block body entered context")
			}
		}
	}
}
func TestHookFailureKindsNeverBecomeFakeSuccess(t *testing.T) {
	for _, test := range []struct {
		mode, status string
		fatal        bool
	}{{"nonblocking", "nonblocking_error", false}, {"malformed", "failed", true}, {"null", "failed", true}, {"unknown", "failed", true}, {"flood", "failed", true}, {"sleep", "failed", true}} {
		t.Run(test.mode, func(t *testing.T) {
			f := newFixture(t)
			entry := f.entry(t, "start", SessionStart, test.mode)
			if test.mode == "sleep" {
				entry.Timeout = 100 * time.Millisecond
			}
			p := f.plugin(t, []Entry{entry}, nil)
			modelCalls := 0
			model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
				modelCalls++
				return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
			})
			err := build(t, f, p, model, nil).Run(context.Background(), nil, discard)
			if (err != nil) != test.fatal {
				t.Fatal("hook error incorrectly reported", err)
			}
			all := receipts(t, f)
			if len(all) != 1 || all[0].Status != test.status || !all[0].Reaped {
				t.Fatal("incorrect evidence", all)
			}
			if test.fatal && modelCalls != 0 {
				t.Fatal("failed lifecycle still invoked model")
			}
			if test.mode == "sleep" && abi.OnlyCancellation(err) {
				t.Fatal("own hook timeout relabelled user cancellation")
			}
		})
	}
}

func TestTrueToolErrorPreservesFailureAndNeverRunsSuccessHook(t *testing.T) {
	f := newFixture(t)
	p := f.plugin(t, []Entry{f.entry(t, "failed", PostToolUseFailure, "failure"), f.entry(t, "success", PostToolUse, "success"), f.entry(t, "stop", Stop, "success"), f.entry(t, "end", SessionEnd, "success")}, nil)
	failure := errors.New("actual tool infrastructure failure")
	tool := &fixtureTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		return wire.ToolResult{}, failure
	}}
	err := build(t, f, p, toolModel(nil), tool).Run(context.Background(), nil, discard)
	if !errors.Is(err, failure) {
		t.Fatal("true tool error swallowed by hook wrapper", err)
	}
	all := receipts(t, f)
	if len(all) != 3 || all[0].Event != PostToolUseFailure || all[1].Event != Stop || all[2].Event != SessionEnd {
		t.Fatal("error did not get distinct failure/finalization events", all)
	}
	for _, receipt := range all {
		if !receipt.Started || !receipt.Reaped || !receipt.Complete {
			t.Fatal("claimed finalization without complete execution", receipt)
		}
	}
}

func TestParentCancellationReapsActiveHookAndRunsBoundedFinalization(t *testing.T) {
	f := newFixture(t)
	start := f.entry(t, "active", SessionStart, "sleep")
	marker := filepath.Join(f.cwd, "active-marker")
	start.Process.Env = map[string]string{"HOOK_MARKER": marker}
	p := f.plugin(t, []Entry{start, f.entry(t, "stop", Stop, "success"), f.entry(t, "end", SessionEnd, "success")}, nil)
	agent := build(t, f, p, toolModel(nil), nil)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan error, 1)
	go func() { done <- agent.Run(ctx, nil, discard) }()
	deadline := time.NewTimer(5 * time.Second)
	defer deadline.Stop()
	ticker := time.NewTicker(10 * time.Millisecond)
	defer ticker.Stop()
waitStarted:
	for {
		select {
		case <-ticker.C:
			if _, err := os.Stat(marker); err == nil {
				break waitStarted
			}
		case err := <-done:
			t.Fatal("hook exited before cancellation", err)
		case <-deadline.C:
			t.Fatal("hook never actually started")
		}
	}
	cancel()
	select {
	case err := <-done:
		if !abi.OnlyCancellation(err) {
			t.Fatal("pure user cancel lost or finalization failed", err)
		}
	case <-time.After(8 * time.Second):
		t.Fatal("canceled hook/finalization did not really finish")
	}
	all := receipts(t, f)
	if len(all) != 3 || !all[0].Reaped || all[0].Status != "failed" || all[0].Complete || all[1].Event != Stop || all[2].Event != SessionEnd || !all[1].Complete || !all[2].Complete {
		t.Fatal("cancel cleanup evidence is incomplete or false", all)
	}
}

func TestHookSourcesDeclareProcessingAndRedactKnownCredentials(t *testing.T) {
	f := newFixture(t)
	p := f.plugin(t, []Entry{f.entry(t, "start", SessionStart, "redact")}, nil)
	if err := build(t, f, p, modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	}), nil).Run(context.Background(), nil, discard); err != nil {
		t.Fatal(err)
	}
	all := receipts(t, f)
	if len(all) != 1 || all[0].Context == nil {
		t.Fatal("missing complete source receipt", all)
	}
	refs, _ := f.repo.BoundReferences(context.Background(), "owner", "native")
	for _, ref := range []wire.Reference{all[0].Stdout, all[0].Stderr, *all[0].Context} {
		page, err := refs.ReadText(context.Background(), ref.URI, 0, 4096)
		if err != nil || strings.Contains(page.Text, "private") || !strings.Contains(ref.Processing, "JSON normalized") {
			t.Fatal("known credentials or processing ambiguity in source", ref, page, err)
		}
	}
}

func TestHookConfiguredCredentialIsAbsentFromStdoutStderrAndContextArtifacts(t *testing.T) {
	f := newFixture(t)
	entry := f.entry(t, "start", SessionStart, "environment")
	const secret = "hook-opaque-environment-credential"
	entry.Process.Env = map[string]string{"API_KEY": secret}
	p := f.plugin(t, []Entry{entry}, nil)
	if err := build(t, f, p, toolModel(nil), nil).Run(context.Background(), nil, discard); err != nil {
		t.Fatal(err)
	}
	all := receipts(t, f)
	if len(all) != 1 || all[0].Context == nil {
		t.Fatal("missing actual source evidence", all)
	}
	refs, _ := f.repo.BoundReferences(context.Background(), "owner", "native")
	for _, ref := range []wire.Reference{all[0].Stdout, all[0].Stderr, *all[0].Context} {
		page, err := refs.ReadText(context.Background(), ref.URI, 0, 4096)
		if err != nil || strings.Contains(page.Text, secret) || !strings.Contains(page.Text, "[REDACTED]") {
			t.Fatal("unlabelled configured credential retained", ref, page, err)
		}
	}
	partial, err := p.retain(context.Background(), "start", "stderr", []byte("public line\n"+secret[:10]), false)
	if err != nil {
		t.Fatal(err)
	}
	page, err := refs.ReadText(context.Background(), partial.URI, 0, 4096)
	if err != nil || page.Text != "public line\n" || !strings.Contains(partial.Processing, "incomplete_tail_omitted_bytes=10") {
		t.Fatal("incomplete credential prefix retained", partial, page, err)
	}
}
func TestFailedActivityAdmissionStartsNoHookAndRejectsForeignBinding(t *testing.T) {
	f := newFixture(t)
	marker := filepath.Join(f.cwd, "marker")
	entry := f.entry(t, "start", SessionStart, "success")
	entry.Process.Env = map[string]string{"HOOK_MARKER": marker}
	failure := errors.New("activity commit failed")
	p := f.plugin(t, []Entry{entry}, func(context.Context, Activity) error { return failure })
	if err := build(t, f, p, toolModel(nil), nil).Run(context.Background(), nil, discard); !errors.Is(err, failure) {
		t.Fatal(err)
	}
	if _, err := os.Stat(marker); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("started after failed durable activity")
	}
	state, _ := abi.NewAgentState(toolModel(nil), core.NewMemorySession("native"), nil)
	if err := p.OnRunStart(context.Background(), abi.RuntimeContext{RunID: "foreign", Workspace: f.cwd, State: state}); err == nil {
		t.Fatal("same-ID foreign authority accepted")
	}
}

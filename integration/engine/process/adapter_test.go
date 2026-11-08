// INPUT: testing, the test executable as a local JSONL process and private temporary directories
// OUTPUT: real subprocess protocol, exit, cancellation, budget, provenance and vendor usage cache-count regressions
// POS: [integration / native external-engine adapter] External engine conformance without executing user CLIs or reading accounts
package process

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/bindings"
)

func TestVendorFixtureProcess(t *testing.T) {
	mode := os.Getenv("ALVA_ENGINE_FIXTURE")
	if mode == "" {
		return
	}
	recordFixtureArgv()
	input, _ := io.ReadAll(os.Stdin)
	fmt.Fprintln(os.Stderr, "fixture diagnostic marker-secret-12345")
	if mode == "before-header" {
		if err := os.WriteFile(os.Getenv("ALVA_ENGINE_READY_FILE"), nil, 0600); err != nil {
			os.Exit(24)
		}
		for {
			time.Sleep(time.Second)
		}
	}
	if mode == "split-secret" {
		fmt.Fprint(os.Stderr, "split marker-")
		time.Sleep(50 * time.Millisecond)
		fmt.Fprintln(os.Stderr, "secret-12345")
		fmt.Fprintln(os.Stderr, `{"api_key":"field-secret-not-in-env","ordinary":"retained"}`)
	}
	if mode == "budget" {
		fmt.Fprint(os.Stderr, strings.Repeat("x", 128<<10))
		os.Exit(0)
	}
	switch os.Getenv("ALVA_ENGINE_VENDOR") {
	case "codex":
		fmt.Println(`{"type":"thread.started","thread_id":"fixture-native"}`)
		if mode == "tool-secret" {
			fmt.Println(`{"type":"item.started","item":{"id":"c1","type":"command_execution","command":"echo marker-secret-12345","arguments":{"cmd":"echo marker-secret-12345"}}}`)
			fmt.Println(`{"type":"item.completed","item":{"id":"c1","type":"command_execution","command":"echo marker-secret-12345","exit_code":0,"status":"completed"}}`)
		}
		if mode == "fatal" {
			fmt.Println(`{"type":"turn.failed","error":{"message":"fixture fatal"}}`)
		}
		fmt.Println(`{"type":"item.completed","item":{"type":"agent_message","id":"answer","text":"hello fixture"}}`)
		if mode != "eof" {
			fmt.Println(`{"type":"turn.completed","usage":{"input_tokens":4,"output_tokens":2}}`)
		}
	case "claude":
		if mode == "max-turns" {
			present := func(flag, value string) bool {
				for index, arg := range os.Args {
					if arg == flag && index+1 < len(os.Args) && os.Args[index+1] == value {
						return true
					}
				}
				return false
			}
			if !present("--append-system-prompt", "explicit system instruction") || !present("--max-turns", "1") || !present("--permission-mode", "dontAsk") {
				os.Exit(91)
			}
			fmt.Println(`{"type":"system","subtype":"init","session_id":"fixture-native"}`)
			fmt.Println(`{"type":"result","subtype":"error_max_turns","session_id":"fixture-native","result":null}`)
			os.Exit(0)
		}
		fmt.Println(`{"type":"system","subtype":"init","session_id":"fixture-native"}`)
		switch mode {
		case "result-is-error":
			fmt.Println(`{"type":"result","subtype":"success","session_id":"fixture-native","is_error":true,"result":"fixture API failure"}`)
			os.Exit(0)
		case "result-foreign":
			fmt.Println(`{"type":"result","subtype":"success","session_id":"foreign-native"}`)
			os.Exit(0)
		}
		if mode == "tool-secret" {
			fmt.Println(`{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"echo marker-secret-12345"}}]}}`)
			fmt.Println(`{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}`)
		}
		fmt.Println(`{"type":"assistant","message":{"content":[{"type":"text","text":"hello fixture"}]}}`)
		if mode != "eof" {
			fmt.Println(`{"type":"result","subtype":"success","session_id":"fixture-native"}`)
		}
		if mode == "after-result" {
			fmt.Println(`{"type":"assistant","message":{"content":[{"type":"text","text":"must not replace completed turn"}]}}`)
		}
		if mode == "result-notice" {
			fmt.Println(`{"type":"system","subtype":"prompt_suggestion","session_id":"fixture-native","suggestion":"fixture suggestion"}`)
		}
	case "pi":
		if mode == "pi-files" {
			image, original, text := os.Getenv("ALVA_PI_IMAGE"), os.Getenv("ALVA_PI_ORIGINAL"), os.Getenv("ALVA_PI_TEXT")
			var images int
			for _, arg := range os.Args {
				if strings.HasPrefix(arg, "@") {
					if arg != "@"+image {
						os.Exit(81)
					}
					images++
				}
			}
			if images != 1 || !strings.Contains(string(input), original) || !strings.Contains(string(input), text) || !strings.Contains(string(input), "artifact://pi-original") || !strings.Contains(string(input), "artifact://pi-text") {
				os.Exit(82)
			}
			data, err := os.ReadFile(text)
			if err != nil || string(data) != "selected derivative fixture" {
				os.Exit(83)
			}
		}
		for i, arg := range os.Args {
			if arg == "--session" && i+1 < len(os.Args) {
				cwd, _ := os.Getwd()
				header, _ := json.Marshal(map[string]string{"type": "session", "id": "fixture-native", "cwd": cwd})
				if err := os.WriteFile(os.Args[i+1], append(header, '\n'), 0600); err != nil {
					os.Exit(23)
				}
			}
		}
		fmt.Println(`{"type":"session","id":"fixture-native"}`)
		if mode == "tool-secret" {
			// The configured secret straddles the 512-byte command bound.
			fmt.Printf("{\"type\":\"tool_execution_start\",\"toolCallId\":\"p1\",\"toolName\":\"bash\",\"args\":{\"command\":\"%s marker-secret-12345\"}}\n", strings.Repeat("a", 505))
			fmt.Println(`{"type":"tool_execution_end","toolCallId":"p1","toolName":"bash","isError":false,"result":{}}`)
		}
		if mode == "pi-files" {
			fmt.Println(`{"type":"tool_execution_start","toolCallId":"read-selected","toolName":"read","args":{"path":"selected captured derivative"}}`)
			fmt.Println(`{"type":"tool_execution_end","toolCallId":"read-selected","toolName":"read","isError":false,"result":{"content":[{"type":"text","text":"selected derivative fixture"}]}}`)
		}
		fmt.Println(`{"type":"message_start","message":{"role":"assistant"}}`)
		if mode == "pi-aborted" || mode == "pi-length" {
			reason := strings.TrimPrefix(mode, "pi-")
			fmt.Printf("{\"type\":\"message_end\",\"message\":{\"role\":\"assistant\",\"stopReason\":%q}}\n", reason)
			fmt.Println(`{"type":"agent_end","willRetry":false}`)
			fmt.Printf("{\"type\":\"agent_settled\",\"aborted\":%t}\n", reason == "aborted")
			os.Exit(0)
		}
		fmt.Println(`{"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"hello fixture"}]}}`)
		if mode != "eof" {
			fmt.Println(`{"type":"agent_end","willRetry":false}`)
			if mode != "pi-unsettled" {
				fmt.Println(`{"type":"agent_settled","aborted":false}`)
			}
		}
	}
	if mode == "nonzero" {
		os.Exit(9)
	}
	if mode == "hang" {
		_ = os.Stdout.Close()
		_ = os.Stderr.Close()
		for {
			time.Sleep(time.Second)
		}
	}
	os.Exit(0)
}
func TestClaudeRequestOptionsReachActualChildAndLimitIsFailure(t *testing.T) {
	a, _ := fixtureAdapter(t, "claude", "max-turns")
	d := a.Descriptor()
	if !d.Config.SupportsMaxTurns || !d.Config.SupportsSystemPrompt {
		t.Fatal("actual options missing in descriptor")
	}
	var started bool
	var terminal engine.Outcome
	err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "ordinary user prompt", SystemPrompt: "explicit system instruction", MaxTurns: 1}, func(event engine.Event) error {
		if event.Kind == engine.SessionStarted {
			started = true
		}
		if event.Kind == engine.Completed {
			terminal = event.Outcome
		}
		return nil
	})
	if err != nil || !started || terminal != engine.Failed {
		t.Fatal("limit/options did not reach actual child or were falsely successful", started, terminal, err)
	}
	for _, vendor := range []string{"codex", "pi"} {
		adapter, _ := fixtureAdapter(t, vendor, "ok")
		if adapter.Descriptor().Config.SupportsSystemPrompt || adapter.Descriptor().Config.SupportsMaxTurns {
			t.Fatal("invented option support", vendor)
		}
		if _, err := adapter.args(engine.Request{SystemPrompt: "must not become user text"}, "fixture"); !errors.Is(err, engine.ErrUnsupported) {
			t.Fatal("unsupported system role silently demoted", vendor, err)
		}
	}
}
func TestPiPrivateNativeResumeSurvivesAdapterRestartAndRejectsForeignOwner(t *testing.T) {
	a, _ := fixtureAdapter(t, "pi", "ok")
	ctx := context.Background()
	workspace := t.TempDir()
	request := engine.Request{OwnerID: "product-a", Prompt: "first", Workspace: workspace}
	sink := func(engine.Event) error { return nil }
	if err := a.Execute(ctx, request, sink); err != nil {
		t.Fatal(err)
	}
	config := a.config
	if err := a.Close(ctx); err != nil {
		t.Fatal(err)
	}
	store, err := bindings.Open(t.Context(), config.StateDirectory)
	if err != nil {
		t.Fatal(err)
	}
	config.Bindings = bindingPorts{store}
	reopened, err := New(config)
	if err != nil {
		store.Close()
		t.Fatal(err)
	}
	defer reopened.Close(ctx)
	request.ResumeSession = "fixture-native"
	request.Prompt = "second"
	if err = reopened.Execute(ctx, request, sink); err != nil {
		t.Fatal("exact native resume failed", err)
	}
	request.OwnerID = "product-b"
	if err = reopened.Execute(ctx, request, sink); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal("foreign owner resumed another native conversation", err)
	}
	request.OwnerID = "product-a"
	request.ResumeSession = "unknown"
	if err = reopened.Execute(ctx, request, sink); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal("unknown native id silently became a fresh session", err)
	}
	request.ResumeSession = "fixture-native"
	request.Workspace = t.TempDir()
	if err = reopened.Execute(ctx, request, sink); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal("native session rebound to foreign workspace", err)
	}
}

func TestPiResumeIgnoresUnrelatedIncompleteSessionFiles(t *testing.T) {
	a, dir := fixtureAdapter(t, "pi", "ok")
	ctx := context.Background()
	request := engine.Request{OwnerID: "product-a", Prompt: "first", Workspace: t.TempDir()}
	sink := func(engine.Event) error { return nil }
	if err := a.Execute(ctx, request, sink); err != nil {
		t.Fatal(err)
	}
	for name, contents := range map[string]string{"aborted.jsonl": "", "partial.jsonl": `{"type":"session"`} {
		if err := os.WriteFile(filepath.Join(dir, "sessions", name), []byte(contents), 0600); err != nil {
			t.Fatal(err)
		}
	}
	request.ResumeSession = "fixture-native"
	if err := a.Execute(ctx, request, sink); err != nil {
		t.Fatalf("unrelated aborted startup blocked a valid exact resume: %v", err)
	}
}

func TestExternalJournalRedactsSplitStderrAndCredentialFields(t *testing.T) {
	a, dir := fixtureAdapter(t, "codex", "split-secret")
	if err := a.Execute(context.Background(), engine.Request{OwnerID: "product-a", Prompt: "first", Workspace: t.TempDir()}, func(engine.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	files, err := os.ReadDir(filepath.Join(dir, "journals"))
	if err != nil || len(files) != 1 {
		t.Fatal(files, err)
	}
	data, err := os.ReadFile(filepath.Join(dir, "journals", files[0].Name()))
	if err != nil {
		t.Fatal(err)
	}
	for _, leaked := range []string{"marker-", "secret-12345", "field-secret-not-in-env"} {
		if strings.Contains(string(data), leaked) {
			t.Fatalf("private journal retained credential fragment %q", leaked)
		}
	}
	if !strings.Contains(string(data), "retained") {
		t.Fatal("ordinary diagnostic field was lost")
	}
}

type bindingPorts struct{ storage.BindingStore }

type bindingReadFailure struct {
	storage.BindingStore
	failure error
}

func (s bindingReadFailure) ReadBinding(context.Context, string) (storage.SessionBinding, error) {
	return storage.SessionBinding{}, s.failure
}
func TestResumePreservesSelectedProviderFailureWithoutStartingProcess(t *testing.T) {
	a, directory := fixtureAdapter(t, "codex", "ok")
	failure := errors.New("selected identity provider unavailable")
	a.config.Bindings = bindingReadFailure{BindingStore: a.config.Bindings, failure: failure}
	err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), ResumeSession: "actual-native", Prompt: "resume"}, func(engine.Event) error { t.Fatal("failed storage started runtime observations"); return nil })
	if !errors.Is(err, failure) || errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal("storage failure was hidden as an unknown session", err)
	}
	if _, err := os.Stat(filepath.Join(directory, "journals")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("failed lookup started process IO", err)
	}
}

func TestMissingBindingSlotDoesNotSelectDatabase(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "unopened")
	if a, err := New(Config{Vendor: "codex", StateDirectory: directory, ApprovedHostAccess: true}); err == nil {
		a.Close(t.Context())
		t.Fatal("missing binding component selected a backend")
	}
	if _, err := os.Stat(directory); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("missing slot performed storage IO", err)
	}
}

type bindingCloseFailure struct {
	storage.BindingStore
	failure error
	calls   int
}

func (s *bindingCloseFailure) Close() error {
	s.calls++
	if s.calls == 1 {
		return s.failure
	}
	return s.BindingStore.Close()
}
func TestAdapterRetainsSelectedBindingUntilCloseSucceeds(t *testing.T) {
	a, _ := fixtureAdapter(t, "codex", "ok")
	failure := errors.New("selected binding cleanup incomplete")
	selected := &bindingCloseFailure{BindingStore: a.config.Bindings, failure: failure}
	a.config.Bindings = selected
	if err := a.Close(t.Context()); !errors.Is(err, failure) || a.storeClosed {
		t.Fatal("lost failed provider owner", err)
	}
	if err := a.Close(t.Context()); err != nil || !a.storeClosed || selected.calls != 2 {
		t.Fatal("selected component was not closed again", err)
	}
}

func fixtureAdapter(t *testing.T, vendor, mode string) (*Adapter, string) {
	t.Helper()
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	dir := filepath.Join(t.TempDir(), "private")
	store, err := bindings.Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	a, err := New(Config{Bindings: bindingPorts{store}, Vendor: vendor, Command: binary, PrefixArgs: []string{"-test.run=^TestVendorFixtureProcess$", "--"}, StateDirectory: dir, ApprovedHostAccess: true, Env: map[string]string{"ALVA_ENGINE_FIXTURE": mode, "ALVA_ENGINE_VENDOR": vendor, "FIXTURE_TOKEN": "marker-secret-12345"}})
	if err != nil {
		store.Close()
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = a.Close(ctx)
	})
	return a, dir
}
func TestActualVendorJSONLProtocolsAndPrivateRedactedJournal(t *testing.T) {
	for _, vendor := range []string{"codex", "claude", "pi"} {
		t.Run(vendor, func(t *testing.T) {
			a, dir := fixtureAdapter(t, vendor, "ok")
			var events []engine.Event
			var contract engine.StreamContract
			err := a.Execute(context.Background(), engine.Request{OwnerID: "fixture-owner", Prompt: "fixture prompt", Workspace: t.TempDir()}, func(event engine.Event) error { events = append(events, event); return contract.Accept(event) })
			if err != nil {
				t.Fatal(err)
			}
			if !contract.Terminal || events[len(events)-1].Outcome != engine.Succeeded {
				t.Fatal("native result/exit not completed", events)
			}
			files, err := os.ReadDir(filepath.Join(dir, "journals"))
			if err != nil || len(files) != 1 {
				t.Fatal("journal missing", err)
			}
			data, err := os.ReadFile(filepath.Join(dir, "journals", files[0].Name()))
			if err != nil || strings.Contains(string(data), "marker-secret-12345") || !strings.Contains(string(data), "fixture prompt") || !strings.Contains(string(data), "fixture diagnostic") {
				t.Fatal("I/O evidence missing or secret leaked", err)
			}
		})
	}
}
func TestProcessFailuresNeverInventSuccessfulCompletion(t *testing.T) {
	for _, mode := range []string{"eof", "fatal", "nonzero", "budget", "hang"} {
		t.Run(mode, func(t *testing.T) {
			a, _ := fixtureAdapter(t, "codex", mode)
			if mode == "budget" {
				a.config.MaxJournalBytes = 4096
			}
			ctx := context.Background()
			var cancel context.CancelFunc
			if mode == "hang" {
				ctx, cancel = context.WithTimeout(ctx, 100*time.Millisecond)
				defer cancel()
			}
			var events []engine.Event
			var contract engine.StreamContract
			err := a.Execute(ctx, engine.Request{OwnerID: "fixture-owner", Prompt: "fixture", Workspace: t.TempDir()}, func(event engine.Event) error { events = append(events, event); return contract.Accept(event) })
			if err != nil {
				t.Fatal("adapter broke terminal contract", err)
			}
			if len(events) == 0 || !contract.Terminal || events[len(events)-1].Outcome == engine.Succeeded {
				t.Fatal("failure became success", events)
			}
			if mode == "hang" && events[len(events)-1].Outcome != engine.Cancelled || mode == "budget" && events[len(events)-1].Outcome != engine.Failed {
				t.Fatal("cancellation and strong journal failure lost their distinct outcomes", mode, events)
			}
			seenFatal := false
			for _, event := range events {
				if seenFatal && event.Kind != engine.Completed {
					t.Fatal("output after fatal", events)
				}
				if event.Kind == engine.Error && !event.Recoverable {
					seenFatal = true
				}
			}
		})
	}
}

func TestEarlyCancellationReferencesOnlyAuthorizedResumeIdentity(t *testing.T) {
	for _, resume := range []bool{false, true} {
		t.Run(fmt.Sprintf("resume=%t", resume), func(t *testing.T) {
			a, _ := fixtureAdapter(t, "codex", "ok")
			request := engine.Request{OwnerID: "fixture-owner", Workspace: t.TempDir(), Prompt: "early cancellation fixture"}
			if resume {
				if err := a.Execute(t.Context(), request, func(engine.Event) error { return nil }); err != nil {
					t.Fatal(err)
				}
				request.ResumeSession = "fixture-native"
			}
			ready := filepath.Join(t.TempDir(), "before-header")
			a.config.Env["ALVA_ENGINE_FIXTURE"] = "before-header"
			a.config.Env["ALVA_ENGINE_READY_FILE"] = ready
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			var events []engine.Event
			done := make(chan error, 1)
			go func() {
				done <- a.Execute(ctx, request, func(event engine.Event) error { events = append(events, event); return nil })
			}()
			deadline := time.Now().Add(5 * time.Second)
			for {
				if _, err := os.Stat(ready); err == nil {
					break
				} else if !os.IsNotExist(err) {
					t.Fatal(err)
				}
				if time.Now().After(deadline) {
					t.Fatal("child did not reach its pre-header barrier")
				}
				time.Sleep(time.Millisecond)
			}
			cancel()
			select {
			case err := <-done:
				if err != nil {
					t.Fatal("early cancellation violated the terminal contract", err)
				}
			case <-time.After(5 * time.Second):
				t.Fatal("early cancellation did not join its process")
			}
			if len(events) != 2 || events[0].Kind != engine.Error || events[0].Failure != engine.FailureCancelled || events[1].Kind != engine.Completed || events[1].Outcome != engine.Cancelled {
				t.Fatal("early cancellation was not a clean cancelled terminal", events)
			}
			for _, event := range events {
				if event.SessionID != "" || event.Reference == nil || event.Reference.SessionID != request.ResumeSession {
					t.Fatal("journal reference fabricated or lost native confirmation", event)
				}
			}
		})
	}
}

func TestRetainedJournalReferenceCannotBypassResumeBindingOrNativeConfirmation(t *testing.T) {
	a, directory := fixtureAdapter(t, "codex", "ok")
	request := engine.Request{OwnerID: "fixture-owner", Workspace: t.TempDir(), Prompt: "establish native fixture"}
	if err := a.Execute(t.Context(), request, func(engine.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	for _, mutate := range []func(*engine.Request){
		func(r *engine.Request) { r.ResumeSession = "missing" },
		func(r *engine.Request) { r.OwnerID = "foreign-owner" },
		func(r *engine.Request) { r.Workspace = t.TempDir() },
	} {
		invalid := request
		invalid.ResumeSession = "fixture-native"
		mutate(&invalid)
		if err := a.Execute(t.Context(), invalid, func(engine.Event) error { t.Fatal("unbound resume emitted an event"); return nil }); !errors.Is(err, engine.ErrSessionNotFound) {
			t.Fatal("journal fallback bypassed exact native binding", err)
		}
	}
	files, err := os.ReadDir(filepath.Join(directory, "journals"))
	if err != nil || len(files) != 1 {
		t.Fatal("rejected resumes created execution journals", len(files), err)
	}
	m := newMapper("codex", "engine-journal://retained-fixture")
	m.referenceSession = "fixture-native"
	if m.session != "" || m.reference().SessionID != "fixture-native" {
		t.Fatal("retained journal identity fabricated native confirmation")
	}
	events, err := m.mapLine([]byte(`{"type":"thread.started","thread_id":"fixture-native"}`))
	if err != nil || len(events) != 1 || events[0].Kind != engine.SessionStarted {
		t.Fatal("journal fallback suppressed the actual native confirmation", events, err)
	}
	request.ResumeSession = "fixture-native"
	if err := a.bindNative(t.Context(), request, "different-native"); err == nil {
		t.Fatal("retained journal fallback accepted a different actual native identity")
	}
}
func TestPiPendingErrorCanRecoverBeforeRealEOF(t *testing.T) {
	m := newMapper("pi", "engine-journal://fixture")
	for _, line := range []string{`{"type":"session","id":"fixture-native"}`, `{"type":"message_update","assistantMessageEvent":{"type":"error","error":{"errorMessage":"context pressure"}}}`, `{"type":"agent_end","willRetry":false}`, `{"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"recovered"}]}}`, `{"type":"agent_settled","aborted":false}`} {
		if _, err := m.mapLine([]byte(line)); err != nil {
			t.Fatal(err)
		}
	}
	if m.pending != "" || m.failure != "" || !m.done {
		t.Fatal("transient error poisoned recovered answer")
	}
}
func TestExplicitApprovalAndUnsafeResumeDenied(t *testing.T) {
	if _, err := New(Config{Vendor: "codex", StateDirectory: t.TempDir()}); err == nil {
		t.Fatal("unapproved external access accepted")
	}
	a, _ := fixtureAdapter(t, "codex", "ok")
	if _, err := a.args(engine.Request{ResumeSession: "--dangerously-bypass-approvals-and-sandbox"}, "run"); err == nil {
		t.Fatal("opaque native ID became CLI flags")
	}
}

func TestJournalSanitizationFailureRemainsLatched(t *testing.T) {
	file, err := os.CreateTemp(t.TempDir(), "journal-")
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	j := &journal{file: file, limit: 1 << 20}
	for i := 0; i < 600; i++ {
		j.secrets = append(j.secrets, fmt.Sprintf("explicit-secret-value-%d", i))
	}
	if err := j.write("stdout", "ordinary frame"); err == nil {
		t.Fatal("redaction work limit was not enforced")
	}
	j.secrets = nil
	if err := j.write("stdout", "later frame"); err == nil {
		t.Fatal("journal resumed after its sanitization failed")
	}
	info, err := file.Stat()
	if err != nil || info.Size() != 0 {
		t.Fatal("failed journal retained successful suffix", info, err)
	}
}

func TestAllExternalEnginesRejectGenerationBeforeFilesystemOrProcess(t *testing.T) {
	temperature := 0.7
	for _, vendor := range []string{"claude", "codex", "pi"} {
		t.Run(vendor, func(t *testing.T) {
			adapter, _ := fixtureAdapter(t, vendor, "ok")
			if adapter.Descriptor().Config.SupportsGeneration {
				t.Fatal("external adapter claimed unimplemented generation flags")
			}
			request := engine.Request{OwnerID: "fixture-owner", Workspace: filepath.Join(t.TempDir(), "does-not-exist"), Prompt: "must not start", Generation: engine.GenerationOptions{Temperature: &temperature}}
			if _, err := adapter.args(request, "fixture"); !errors.Is(err, engine.ErrUnsupported) {
				t.Fatal("argv preparation silently ignored generation", err)
			}
			if err := adapter.Execute(t.Context(), request, func(engine.Event) error { t.Fatal("unsupported request emitted process events"); return nil }); !errors.Is(err, engine.ErrUnsupported) {
				t.Fatal("generation reached filesystem or process admission", err)
			}
		})
	}
}

func TestPrintTransportsRedactToolInputAndDisplay(t *testing.T) {
	for _, vendor := range []string{"codex", "claude", "pi"} {
		t.Run(vendor, func(t *testing.T) {
			a, _ := fixtureAdapter(t, vendor, "tool-secret")
			var tools []engine.Event
			err := a.Execute(context.Background(), engine.Request{OwnerID: "fixture-owner", Prompt: "fixture prompt", Workspace: t.TempDir()}, func(event engine.Event) error {
				if event.Kind == engine.ToolStart || event.Kind == engine.ToolEnd {
					tools = append(tools, event)
				}
				return nil
			})
			if err != nil || len(tools) != 2 {
				t.Fatal("tool fixture did not run", err, len(tools))
			}
			for _, event := range tools {
				data, _ := json.Marshal(struct {
					Input   json.RawMessage
					Display *engine.ToolDisplay
				}{event.Input, event.Display})
				if strings.Contains(string(data), "marker-secret-12345") || strings.Contains(event.Display.Command, "marker") {
					t.Fatalf("print transport leaked a configured secret or its clipped prefix: %s", data)
				}
				if event.Display == nil || event.Display.Kind != engine.ToolCommand || !strings.Contains(event.Display.Command, "[REDAC") {
					t.Fatalf("redacted display missing: %+v", event.Display)
				}
			}
		})
	}
}

func TestVendorUsageCarriesInclusiveCacheCounts(t *testing.T) {
	n := func(v uint64) *uint64 { return &v }
	same := func(a, b *uint64) bool { return a == nil && b == nil || a != nil && b != nil && *a == *b }
	for _, test := range []struct {
		name, vendor   string
		lines          []string
		input, output  uint64
		cached, writes *uint64
	}{
		{"codex-reported", "codex", []string{`{"type":"turn.completed","usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":7}}`}, 100, 7, n(80), nil},
		{"codex-zero", "codex", []string{`{"type":"turn.completed","usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":7}}`}, 100, 7, n(0), nil},
		{"codex-absent", "codex", []string{`{"type":"turn.completed","usage":{"input_tokens":4,"output_tokens":2}}`}, 4, 2, nil, nil},
		{"codex-inconsistent", "codex", []string{`{"type":"turn.completed","usage":{"input_tokens":4,"cached_input_tokens":9,"output_tokens":2}}`}, 4, 2, nil, nil},
		{"claude-exclusive", "claude", []string{`{"type":"system","subtype":"init","session_id":"claude-native"}`, `{"type":"result","subtype":"success","usage":{"input_tokens":10,"cache_read_input_tokens":60,"cache_creation_input_tokens":30,"output_tokens":5}}`}, 100, 5, n(60), n(30)},
		{"claude-absent", "claude", []string{`{"type":"system","subtype":"init","session_id":"claude-native"}`, `{"type":"result","subtype":"success","usage":{"input_tokens":10,"output_tokens":5}}`}, 10, 5, nil, nil},
		{"pi-accumulated", "pi", []string{
			`{"type":"message_start","message":{"role":"assistant"}}`,
			`{"type":"message_end","message":{"role":"assistant","stopReason":"toolUse","content":[],"usage":{"input":5,"output":1,"cacheRead":15,"cacheWrite":0}}}`,
			`{"type":"message_start","message":{"role":"assistant"}}`,
			`{"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"done"}],"usage":{"input":2,"output":3,"cacheRead":18,"cacheWrite":0}}}`,
		}, 40, 4, n(33), n(0)},
		{"pi-zero-is-unreported", "pi", []string{
			`{"type":"message_start","message":{"role":"assistant"}}`,
			`{"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"done"}],"usage":{"input":7,"output":3,"cacheRead":0,"cacheWrite":0}}}`,
		}, 7, 3, nil, nil},
		{"pi-mixed-is-unreported", "pi", []string{
			`{"type":"message_start","message":{"role":"assistant"}}`,
			`{"type":"message_end","message":{"role":"assistant","stopReason":"toolUse","content":[],"usage":{"input":5,"output":1,"cacheRead":15,"cacheWrite":0}}}`,
			`{"type":"message_start","message":{"role":"assistant"}}`,
			`{"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"done"}],"usage":{"input":2,"output":3,"cacheRead":0,"cacheWrite":0}}}`,
		}, 22, 4, nil, nil},
	} {
		t.Run(test.name, func(t *testing.T) {
			m := newMapper(test.vendor, "engine-journal://run")
			for _, line := range test.lines {
				if _, err := m.mapLine([]byte(line)); err != nil {
					t.Fatal(err)
				}
			}
			if m.usage.InputTokens != test.input || m.usage.OutputTokens != test.output || !same(m.usage.CachedInputTokens, test.cached) || !same(m.usage.CacheWriteInputTokens, test.writes) || m.usage.Validate() != nil {
				t.Fatalf("usage %+v", m.usage)
			}
		})
	}
	run := codexRun{session: "bound-thread", turnID: "current-turn"}
	frame := codexFrame{Method: "thread/tokenUsage/updated", Params: json.RawMessage(`{"threadId":"bound-thread","turnId":"current-turn","tokenUsage":{"last":{"inputTokens":50,"cachedInputTokens":20,"outputTokens":4}}}`)}
	if err := run.handle(context.Background(), frame); err != nil {
		t.Fatal(err)
	}
	if run.usage.InputTokens != 50 || run.usage.OutputTokens != 4 || run.usage.CachedInputTokens == nil || *run.usage.CachedInputTokens != 20 {
		t.Fatalf("app-server usage %+v", run.usage)
	}
}

func TestResolvedAnswersKeepOnlyOfferedChoices(t *testing.T) {
	request := engine.UserInputRequest{Questions: []engine.UserInputQuestion{
		{ID: "pick", Question: "A or B?", Options: []engine.UserInputOption{{Label: "A"}, {Label: "B"}}, IsOther: true},
		{ID: "password", Question: "Which vault?", Options: []engine.UserInputOption{{Label: "Team vault"}}},
		{ID: "token", Question: "Token?", IsSecret: true, IsOther: true, Options: []engine.UserInputOption{{Label: "Use saved"}}},
		{ID: "note", Question: "Anything else?"},
	}}
	response := engine.UserInputResponse{Answers: map[string][]string{"pick": {"B"}, "password": {"Team vault"}, "token": {"Use saved"}, "note": {`{"pin":"4821"}`}}}
	relayed := relayedAnswers(request, response)
	if strings.Join(relayed["pick"], ",") != "B" || strings.Join(relayed["password"], ",") != "Team vault" {
		t.Fatal("offered choices were not kept", relayed)
	}
	for _, id := range []string{"token", "note"} {
		if relayed[id] == nil || len(relayed[id]) != 0 {
			t.Fatal("a secret or free-text answer kept its body", id, relayed[id])
		}
	}
	if typed := relayedAnswers(request, engine.UserInputResponse{Answers: map[string][]string{"pick": {"my own words"}}}); typed["pick"] == nil || len(typed["pick"]) != 0 {
		t.Fatal("free text beside offered options kept its body", typed)
	}
}

func TestCodexResolvedAnswersNeverAbortTheRun(t *testing.T) {
	for _, id := range []string{"password", "api_key", "db_password", "color"} {
		for _, test := range []struct {
			name, label, secret string
			kept                bool
		}{
			{"plain-label", "Team vault", "", true},
			{"label-holds-secret", "vault hunter2-value", "hunter2-value", false},
			{"redaction-would-grow", strings.Repeat("ab ", 1300), "ab", false},
		} {
			t.Run(id+"/"+test.name, func(t *testing.T) {
				j := &journal{}
				if test.secret != "" {
					if err := j.registerPrivateAnswers([]string{test.secret}); err != nil {
						t.Fatal(err)
					}
				}
				var got []engine.Event
				r := &codexRun{rpc: &codexRPC{journal: j}, session: "thread", emit: func(e engine.Event) error { got = append(got, e); return nil }}
				err := r.event(engine.Event{Kind: engine.InteractionResolved, ID: "req", Name: "user_input", Resolution: &engine.InteractionResolution{Answers: map[string][]string{id: {test.label}}}})
				if err != nil || len(got) != 1 || got[0].Resolution == nil {
					t.Fatal("a relayed answer ended the run", err, got)
				}
				contract := engine.StreamContract{}
				if err := contract.Accept(got[0]); err != nil {
					t.Fatal("published resolution broke the stream contract", err)
				}
				answers := got[0].Resolution.Answers[id]
				if test.kept != (len(answers) == 1 && answers[0] == test.label) || !test.kept && (answers == nil || len(answers) != 0) {
					t.Fatalf("published %q", answers)
				}
				if test.secret != "" && strings.Contains(fmt.Sprint(got[0].Resolution.Answers), test.secret) {
					t.Fatal("secret published")
				}
			})
		}
	}
}

func TestPiResolvedAnswersNeverCarryABody(t *testing.T) {
	for _, answer := range []string{`hunter2`, `"hunter2"`, `["tok-123"]`, `{"pin":"4821"}`, `123456`} {
		j := &journal{}
		if err := j.registerPrivateAnswers([]string{answer}); err != nil {
			t.Fatal(err)
		}
		var got []engine.Event
		r := &piRun{connection: &piConnection{journal: j}, session: "pi-native", emit: func(e engine.Event) error { got = append(got, e); return nil }}
		err := r.event(engine.Event{Kind: engine.InteractionResolved, ID: "pi:x", Name: "user_input", Resolution: &engine.InteractionResolution{Answers: map[string][]string{"answer": {answer}}}})
		if err != nil || len(got) != 1 || got[0].Resolution == nil {
			t.Fatal(answer, err, got)
		}
		data, _ := json.Marshal(got[0])
		if published := got[0].Resolution.Answers["answer"]; published == nil || len(published) != 0 || strings.Contains(string(data), "hunter2") || strings.Contains(string(data), "tok-123") || strings.Contains(string(data), "4821") || strings.Contains(string(data), "123456") {
			t.Fatalf("answer %q published %s", answer, data)
		}
	}
}

// Codex reports its own window beside the measured last request; the context
// meter shows only that, and nothing when the window is absent.
func TestCodexContextReportNeedsItsDeclaredWindow(t *testing.T) {
	var events []engine.Event
	run := codexRun{session: "bound-thread", turnID: "current-turn", rpc: &codexRPC{journal: &journal{}}, emit: func(event engine.Event) error { events = append(events, event); return nil }}
	without := codexFrame{Method: "thread/tokenUsage/updated", Params: json.RawMessage(`{"threadId":"bound-thread","turnId":"current-turn","tokenUsage":{"last":{"inputTokens":50,"cachedInputTokens":20,"outputTokens":4},"total":{"inputTokens":50,"cachedInputTokens":20,"outputTokens":4},"modelContextWindow":null}}`)}
	if err := run.handle(context.Background(), without); err != nil || len(events) != 0 {
		t.Fatal("context reported without a declared window", err, events)
	}
	with := codexFrame{Method: "thread/tokenUsage/updated", Params: json.RawMessage(`{"threadId":"bound-thread","turnId":"current-turn","tokenUsage":{"last":{"inputTokens":5000,"cachedInputTokens":2000,"outputTokens":40},"total":{"inputTokens":9000,"cachedInputTokens":2000,"outputTokens":90},"modelContextWindow":258400}}`)}
	if err := run.handle(context.Background(), with); err != nil || len(events) != 1 {
		t.Fatal("declared window not reported", err, events)
	}
	report := events[0].Context
	if events[0].Kind != engine.ContextUpdated || report == nil || report.Usage == nil || report.Usage.InputTokens != 5000 || report.Usage.OutputTokens != 40 || report.Usage.WindowTokens != 258400 || report.Usage.Parts != nil || report.Validate() != nil {
		t.Fatalf("codex context %+v", events[0])
	}
}

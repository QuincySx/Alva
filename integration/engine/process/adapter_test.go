// INPUT: testing, the test executable as a local JSONL process and private temporary directories
// OUTPUT: real subprocess protocol, exit, cancellation, budget and provenance regressions
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
	_, _ = io.ReadAll(os.Stdin)
	fmt.Fprintln(os.Stderr, "fixture diagnostic marker-secret-12345")
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
		fmt.Println(`{"type":"assistant","message":{"content":[{"type":"text","text":"hello fixture"}]}}`)
		if mode != "eof" {
			fmt.Println(`{"type":"result","subtype":"success","session_id":"fixture-native"}`)
		}
	case "pi":
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
		fmt.Println(`{"type":"message_start","message":{"role":"assistant"}}`)
		fmt.Println(`{"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"hello fixture"}]}}`)
		if mode != "eof" {
			fmt.Println(`{"type":"agent_end","willRetry":false}`)
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
func TestPiPendingErrorCanRecoverBeforeRealEOF(t *testing.T) {
	m := newMapper("pi", "engine-journal://fixture")
	for _, line := range []string{`{"type":"session","id":"fixture-native"}`, `{"type":"message_update","assistantMessageEvent":{"type":"error","error":{"errorMessage":"context pressure"}}}`, `{"type":"agent_end","willRetry":false}`, `{"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"recovered"}]}}`} {
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

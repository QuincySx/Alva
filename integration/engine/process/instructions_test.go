// INPUT: Owned helper child processes for each vendor transport and their recorded argv or first thread frame
// OUTPUT: Product instructions reach only Claude's append-only system prompt after the request SystemPrompt; Codex/Pi receive no extra flag or field
// POS: [integration / native external-engine adapter tests] Proves the delivery channel without installed CLIs, accounts or models
package process

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/bindings"
)

// A fixture with quotes, a backtick fence and newlines: the text must reach the
// child byte for byte, never re-read as flags, TOML keys or a file path.
const instructionsFixture = "Charts: put JSON in a ```chart block.\n" + `{"type":"bar","x":["a"]}` + "\nkey = \"not a key\""

// recordFixtureArgv lets a helper child prove the exact argv it received.
func recordFixtureArgv() {
	if path := os.Getenv("ALVA_FIXTURE_ARGV_FILE"); path != "" {
		data, _ := json.Marshal(os.Args)
		_ = os.WriteFile(path, data, 0600)
	}
}

func recordedArgv(t *testing.T, path string) []string {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal("child never recorded its argv", err)
	}
	var argv []string
	if err := json.Unmarshal(data, &argv); err != nil {
		t.Fatal(err)
	}
	return argv
}

func flagValue(argv []string, flag string) (string, int) {
	values, last := 0, ""
	for i, arg := range argv {
		if arg == flag && i+1 < len(argv) {
			values++
			last = argv[i+1]
		}
	}
	return last, values
}

func runSucceeds(t *testing.T, a *Adapter, request engine.Request) {
	t.Helper()
	var outcome engine.Outcome
	var contract engine.StreamContract
	var errorsSeen []string
	err := a.Execute(t.Context(), request, func(event engine.Event) error {
		if event.Kind == engine.Error {
			errorsSeen = append(errorsSeen, event.Text)
		}
		if event.Kind == engine.Completed {
			outcome = event.Outcome
		}
		return contract.Accept(event)
	})
	if err != nil || outcome != engine.Succeeded {
		t.Fatal("instructed run did not succeed", outcome, errorsSeen, err)
	}
}

func TestProductInstructionsFollowRequestSystemPromptInClaudeArgv(t *testing.T) {
	for system, want := range map[string]string{"USER-SYSTEM-FIXTURE": "USER-SYSTEM-FIXTURE\n\n" + instructionsFixture, "": instructionsFixture} {
		// The fixture always reports one native identity, so each case gets
		// its own adapter and binding store.
		a, _ := fixtureAdapter(t, "claude", "ok")
		a.config.ProductInstructions = instructionsFixture
		argvFile := filepath.Join(t.TempDir(), "argv.json")
		a.config.Env["ALVA_FIXTURE_ARGV_FILE"] = argvFile
		runSucceeds(t, a, engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "prompt", SystemPrompt: system})
		if value, count := flagValue(recordedArgv(t, argvFile), "--append-system-prompt"); count != 1 || value != want {
			t.Fatalf("request system prompt must stay first and intact, then the instructions: %d %q", count, value)
		}
	}
}

// noInstructions fails when the child received the instruction text or any
// instruction-carrying flag at all.
func noInstructions(t *testing.T, argv []string) {
	t.Helper()
	for _, arg := range argv {
		if arg == "--append-system-prompt" || arg == "-c" || strings.Contains(arg, "developer_instructions") || strings.Contains(arg, instructionsFixture) {
			t.Fatalf("vendor whose channel replaces user config still received instructions: %q", argv)
		}
	}
}

// Codex developer instructions (-c and thread start/resume params) replace the
// user's config.toml developer_instructions, and Pi's --append-system-prompt
// replaces its APPEND_SYSTEM.md discovery. Those vendors therefore get nothing
// extra and still run normally; they simply do not learn the chart format.
func TestProductInstructionsSkipCodexAndPiWhoseChannelsReplaceUserConfig(t *testing.T) {
	t.Run("codex exec", func(t *testing.T) {
		a, _ := fixtureAdapter(t, "codex", "ok")
		a.config.ProductInstructions = instructionsFixture
		argvFile := filepath.Join(t.TempDir(), "argv.json")
		a.config.Env["ALVA_FIXTURE_ARGV_FILE"] = argvFile
		runSucceeds(t, a, engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "prompt"})
		noInstructions(t, recordedArgv(t, argvFile))
	})
	t.Run("codex app-server", func(t *testing.T) {
		a, _ := codexAppServerFixture(t, "ok")
		a.config.ProductInstructions = instructionsFixture
		paramsFile := filepath.Join(t.TempDir(), "thread.json")
		a.config.Env["ALVA_CODEX_THREAD_PARAMS_FILE"] = paramsFile
		request := engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "first"}
		for _, resume := range []string{"", "rpc-native"} {
			request.ResumeSession = resume
			runSucceeds(t, a, request)
			data, err := os.ReadFile(paramsFile)
			var params map[string]any
			if err != nil || json.Unmarshal(data, &params) != nil {
				t.Fatal("thread frame not recorded", resume, err)
			}
			if _, present := params["developerInstructions"]; present || strings.Contains(string(data), "chart") {
				t.Fatalf("thread start/resume carried developerInstructions (resume=%q): %s", resume, data)
			}
			if resume != "" && params["threadId"] != resume {
				t.Fatal("resume frame changed", params)
			}
		}
	})
	for _, transport := range []string{"exec", "rpc"} {
		t.Run("pi "+transport, func(t *testing.T) {
			var a *Adapter
			if transport == "rpc" {
				a, _, _ = piRPCFixture(t, "long-public")
			} else {
				a, _ = fixtureAdapter(t, "pi", "ok")
			}
			a.config.ProductInstructions = instructionsFixture
			argvFile := filepath.Join(t.TempDir(), "argv.json")
			a.config.Env["ALVA_FIXTURE_ARGV_FILE"] = argvFile
			runSucceeds(t, a, engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "prompt"})
			noInstructions(t, recordedArgv(t, argvFile))
		})
	}
	// The request SystemPrompt keeps its explicit unsupported answer.
	for _, vendor := range []string{"codex", "pi"} {
		a, _ := fixtureAdapter(t, vendor, "ok")
		a.config.ProductInstructions = instructionsFixture
		if _, err := a.args(engine.Request{SystemPrompt: "session system"}, "fixture"); !errors.Is(err, engine.ErrUnsupported) {
			t.Fatal("product instructions relaxed the unsupported request SystemPrompt", vendor, err)
		}
	}
}

func TestProductInstructionsAbsentLeaveArgvUnchanged(t *testing.T) {
	for _, vendor := range []string{"claude", "codex", "pi"} {
		a, _ := fixtureAdapter(t, vendor, "ok")
		argv, err := a.args(engine.Request{Workspace: t.TempDir()}, "fixture")
		if err != nil {
			t.Fatal(err)
		}
		if slices.Contains(argv, "--append-system-prompt") || slices.Contains(argv, "-c") {
			t.Fatal("no configured instructions still changed argv", vendor, argv)
		}
	}
}

func TestProductInstructionsAreBoundedText(t *testing.T) {
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	for _, text := range []string{strings.Repeat("a", 4<<10+1), "nul\x00byte", "bad\xffutf8"} {
		dir := filepath.Join(t.TempDir(), "private")
		store, err := bindings.Open(t.Context(), dir)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := New(Config{Bindings: bindingPorts{store}, Vendor: "claude", Command: binary, StateDirectory: dir, ApprovedHostAccess: true, ProductInstructions: text}); err == nil {
			t.Fatal("unbounded product instructions accepted", len(text))
		}
		store.Close()
	}
}

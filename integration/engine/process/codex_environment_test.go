// INPUT: Controlled native environment snapshots and owned Codex protocol fixture
// OUTPUT: Cindy-compatible inheritance/defaults, credential-only redaction, safe streaming and recoverable large results
// POS: [integration / native external-engine adapter tests] No OS environment dump, real CLI, account or trust/credential file access
package process

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/integration/engine"
)

func TestCodexNativeEnvironmentMatchesPinnedCindy(t *testing.T) {
	for _, selected := range []bool{false, true} {
		t.Run(fmt.Sprint(selected), func(t *testing.T) {
			directory, err := filepath.EvalSymlinks(t.TempDir())
			if err != nil {
				t.Fatal(err)
			}
			a := &Adapter{config: Config{Transport: "app-server", StateDirectory: directory, Env: map[string]string{"PATH": "/fixture/override-bin", "OMP_NUM_THREADS": "2"}}, nativeEnvironment: func() []string {
				return []string{"HOME=/fixture/native-home", "USERPROFILE=/fixture/userprofile", "XDG_CONFIG_HOME=/fixture/xdg", "MISE_DATA_DIR=/fixture/mise", "PATH=/fixture/native-bin", "CODEX_HOME=/fixture/unselected-profile", "NO_PROXY= corp.internal ,localhost", "no_proxy=lower.internal,corp.internal", "PYTHONUTF8=0", "TERM=xterm-fixture", "NO_COLOR="}
			}}
			if selected {
				a.config.CLIConfigDirectory = t.TempDir()
			}
			env, err := a.codexEnvironment()
			if err != nil {
				t.Fatal(err)
			}
			for key, value := range map[string]string{"HOME": "/fixture/native-home", "USERPROFILE": "/fixture/userprofile", "XDG_CONFIG_HOME": "/fixture/xdg", "MISE_DATA_DIR": "/fixture/mise", "PATH": "/fixture/override-bin", "OMP_NUM_THREADS": "2", "PYTHONUTF8": "0", "PYTHONIOENCODING": "utf-8", "TERM": "xterm-fixture", "NO_COLOR": "", "CLICOLOR": "0", "FORCE_COLOR": "0", "PSStyle__OutputRendering": "PlainText", "NO_PROXY": "corp.internal,localhost,lower.internal,127.0.0.1,::1"} {
				if env[key] != value {
					t.Fatal("controlled native environment field changed", key)
				}
			}
			if _, exists := env["no_proxy"]; exists {
				t.Fatal("lowercase proxy duplicate was retained")
			}
			want := a.config.CLIConfigDirectory
			if want == "" {
				want = filepath.Join(a.config.StateDirectory, "home", "codex")
				if a.AccountSource() != "explicit executable; native environment; private CODEX_HOME" {
					t.Fatal("managed source label misstates native environment")
				}
			} else if !strings.HasPrefix(a.AccountSource(), "explicit CLI configuration sha256:") || strings.Contains(a.AccountSource(), want) {
				t.Fatal("explicit profile fingerprint changed or exposes its path")
			}
			if env["CODEX_HOME"] != want {
				t.Fatal("inherited profile displaced explicit CODEX_HOME selection")
			}
		})
	}
}

func TestCodexNativeWindowsEnvironmentPreservesMixedCaseValues(t *testing.T) {
	env := composeCodexEnvironment([]string{"term=xterm-custom", "no_color=0", "pythonutf8=0", "No_Proxy=corp.internal", "Path=C:\\native-tools", "UserProfile=C:\\fixture-profile"}, map[string]string{"PATH": "C:\\override-tools", "no_proxy": "additional.internal"}, "C:\\managed-codex", true)
	for key, value := range map[string]string{"term": "xterm-custom", "no_color": "0", "pythonutf8": "0", "PATH": "C:\\override-tools", "UserProfile": "C:\\fixture-profile", "CODEX_HOME": "C:\\managed-codex"} {
		if env[key] != value {
			t.Fatal("Windows-aware composition replaced an explicit mixed-case value", key)
		}
	}
	if env["NO_PROXY"] != "additional.internal,corp.internal,127.0.0.1,localhost,::1" {
		t.Fatal("Windows-aware proxy merge lost a native mixed-case entry")
	}
	for key := range env {
		if strings.EqualFold(key, "NO_PROXY") && key != "NO_PROXY" {
			t.Fatal("Windows-aware proxy merge retained a case duplicate")
		}
	}
}

func TestCodexInheritedSecretsSelectCredentialKeysOnly(t *testing.T) {
	effective := map[string]string{"HOME": "/public/native-home", "PATH": "/public/toolchain", "LANG": "en_US.UTF-8", "OPENAI_API_KEY": "controlled-native-key", "GITHUB_TOKEN": "short", "SERVICE_SECRET": "controlled-secret", "AUTHORIZATION": "fixture bearer"}
	values := codexEnvironmentSecrets(effective, map[string]string{"PRIVATE_OVERRIDE": "/explicit-private-path"})
	for _, expected := range []string{"controlled-native-key", "short", "controlled-secret", "fixture bearer", "/explicit-private-path"} {
		if !slices.Contains(values, expected) {
			t.Fatal("known credential or explicit override missing from snapshot")
		}
	}
	for _, public := range []string{"/public/native-home", "/public/toolchain", "en_US.UTF-8"} {
		if slices.Contains(values, public) {
			t.Fatal("public native context was classified as a credential")
		}
	}
	effective["OPENAI_API_KEY"] = "replacement"
	if slices.Contains(values, "replacement") {
		t.Fatal("credential snapshot retained mutable environment ownership")
	}
}

func TestCodexNativeEnvironmentReachesOwnedChildAndProtectsBothSinks(t *testing.T) {
	for _, selected := range []bool{false, true} {
		t.Run(fmt.Sprint(selected), func(t *testing.T) {
			a, dir := codexAppServerFixture(t, "environment")
			a.nativeEnvironment = func() []string {
				return []string{"HOME=/fixture/native-home", "USERPROFILE=/fixture/userprofile", "XDG_CONFIG_HOME=/fixture/xdg", "MISE_DATA_DIR=/fixture/mise", "PATH=/fixture/native-bin", "CODEX_HOME=/fixture/unselected-profile", "NO_PROXY=corp.internal,localhost", "no_proxy= lower.internal,localhost ", "INHERITED_TEST_TOKEN=native-env-secret-ABCD"}
			}
			if selected {
				var err error
				a.config.CLIConfigDirectory, err = filepath.EvalSymlinks(t.TempDir())
				if err != nil {
					t.Fatal(err)
				}
			}
			expected := a.config.CLIConfigDirectory
			if expected == "" {
				expected = filepath.Join(a.config.StateDirectory, "home", "codex")
			}
			a.config.Env["PATH"], a.config.Env["ALVA_EXPECTED_CODEX_HOME"] = "/fixture/override-bin", expected
			var observed []engine.Event
			var owner *codexRun
			if err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "controlled environment fixture"}, func(event engine.Event) error {
				observed = append(observed, event)
				if event.Kind == engine.SessionStarted {
					owner = a.codexRun(event.SessionID)
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
			if len(observed) == 0 || observed[len(observed)-1].Outcome != engine.Succeeded {
				t.Fatal("controlled native environment fixture failed")
			}
			messageStreaming, reasoningStreaming, completed, largeReference := false, false, false, false
			for _, event := range observed {
				messageStreaming = messageStreaming || event.Kind == engine.MessageDelta && strings.Contains(event.Text, "benign streaming prefix")
				reasoningStreaming = reasoningStreaming || event.Kind == engine.ReasoningDelta && strings.Contains(event.Text, "benign reasoning prefix")
				completed = completed || event.Kind == engine.Message && strings.Contains(event.Text, "hello app-server") && strings.Contains(event.Text, "benign tail retained") && strings.Contains(event.Text, "[REDACTED]")
				largeReference = largeReference || event.Kind == engine.ToolEnd && event.ID == "large-tool" && event.Reference != nil && event.Text == "" && len(event.Input) == 0 && !event.IsError
			}
			if !messageStreaming || !reasoningStreaming || !completed || !largeReference {
				t.Fatal("safe streaming, useful aggregate or large-result reference was lost", messageStreaming, reasoningStreaming, completed, largeReference)
			}
			if owner == nil || !owner.rpc.proc.CleanupComplete() || owner.streamBytes != 0 || len(owner.streamTails) != 0 {
				t.Fatal("native process or stream tails survived terminal cleanup")
			}
			files, err := os.ReadDir(filepath.Join(dir, "journals"))
			if err != nil || len(files) != 1 {
				t.Fatal("native journal missing", err)
			}
			journalData, err := os.ReadFile(filepath.Join(dir, "journals", files[0].Name()))
			if err != nil {
				t.Fatal(err)
			}
			productData, _ := json.Marshal(observed)
			for _, forbidden := range []string{"native-env-secret-ABCD", "native-env-", "secret-ABCD"} {
				if strings.Contains(string(journalData), forbidden) || strings.Contains(string(productData), forbidden) {
					t.Fatal("inherited credential or split fragment reached a persistent sink")
				}
			}
			recovered := false
			for _, line := range strings.Split(strings.TrimSpace(string(journalData)), "\n") {
				var envelope struct{ Direction, Text string }
				if json.Unmarshal([]byte(line), &envelope) != nil || envelope.Direction != "stdout" {
					continue
				}
				var frame codexFrame
				if json.Unmarshal([]byte(envelope.Text), &frame) != nil || frame.Method != "item/completed" {
					continue
				}
				var params struct {
					Item struct{ ID, AggregatedOutput string }
				}
				if json.Unmarshal(frame.Params, &params) == nil && params.Item.ID == "large-tool" {
					recovered = len(params.Item.AggregatedOutput) > 64<<10 && strings.HasPrefix(params.Item.AggregatedOutput, "large-before ") && strings.HasSuffix(params.Item.AggregatedOutput, "[REDACTED] large-after")
				}
			}
			if !recovered || !strings.Contains(string(journalData), "omitted_source_bytes") {
				t.Fatal("redacted complete large tool result was not recoverable alongside stream omission facts")
			}
		})
	}
}

func TestCodexCredentialStreamBoundsAndFlush(t *testing.T) {
	if _, err := codexStreamKeep([]string{strings.Repeat("x", (16<<10)+1)}); err == nil {
		t.Fatal("unbounded credential tail accepted")
	}
	secrets := []string{"controlled-credential-value"}
	var events []engine.Event
	run := codexRun{session: "native", rpc: &codexRPC{journal: &journal{secrets: secrets}}, emit: func(event engine.Event) error { events = append(events, event); return nil }}
	if err := run.event(engine.Event{Kind: engine.ReasoningDelta, ID: "reason", Text: "benign tail"}); err != nil {
		t.Fatal(err)
	}
	if len(events) != 0 {
		t.Fatal("credential-width tail was published prematurely")
	}
	if err := run.flushStreams("reason", false); err != nil || len(events) != 1 || events[0].Text != "benign tail" {
		t.Fatal("completed benign tail was not flushed", err)
	}
	for i := 0; i < 65; i++ {
		_, err := run.filterDelta(engine.Event{Kind: engine.MessageDelta, ID: fmt.Sprint(i), Text: "x"}, secrets)
		if (err != nil) != (i == 64) {
			t.Fatal("stream ownership exceeded bounded active items", i, err)
		}
	}
	run.clearStreams()
	if run.streamBytes != 0 || len(run.streamTails) != 0 {
		t.Fatal("stream cleanup retained private tails")
	}
}

func TestCodexRedactedMarkersNeverReenterStreamCarry(t *testing.T) {
	secrets := []string{"unrelated-long-credential-value", "E"}
	var output strings.Builder
	run := codexRun{session: "native", rpc: &codexRPC{journal: &journal{secrets: secrets}}, emit: func(event engine.Event) error { output.WriteString(event.Text); return nil }}
	for _, chunk := range []string{"E", " benign", " output"} {
		if err := run.event(engine.Event{Kind: engine.ReasoningDelta, ID: "reason", Text: chunk}); err != nil {
			t.Fatal(err)
		}
	}
	if err := run.flushStreams("reason", false); err != nil {
		t.Fatal(err)
	}
	if output.String() != "[REDACTED] benign output" {
		t.Fatal("redaction markers reentered the undecided carry")
	}
}

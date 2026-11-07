// INPUT: Explicit temporary CLI directories, isolated process environment and the local test executable
// OUTPUT: Approved configuration validation, canonical account identity and real child profile isolation regressions
// POS: [integration / native external-engine adapter] Verify complete CLI configuration routing without reading any account material.
package process

import (
	"context"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/bindings"
)

func TestCLIProfileFixtureProcess(t *testing.T) {
	vendor := os.Getenv("ALVA_PROFILE_VENDOR")
	if vendor == "" {
		return
	}
	home, selected := os.Getenv("ALVA_EXPECTED_HOME"), os.Getenv("ALVA_EXPECTED_PROFILE")
	expected := map[string]string{
		"HOME": home, "CODEX_HOME": filepath.Join(home, "codex"),
		"CLAUDE_CONFIG_DIR": filepath.Join(home, "claude"), "PI_CODING_AGENT_DIR": filepath.Join(home, "pi"),
	}
	if selected != "" {
		switch vendor {
		case "codex":
			expected["CODEX_HOME"] = selected
		case "claude":
			expected["CLAUDE_CONFIG_DIR"] = selected
		case "pi":
			expected["PI_CODING_AGENT_DIR"] = selected
		}
	}
	for key, value := range expected {
		if os.Getenv(key) != value {
			os.Exit(91)
		}
	}
	if os.Getenv("ALVA_AMBIENT_PROFILE_MARKER") != "" {
		os.Exit(92)
	}
	_, _ = io.ReadAll(os.Stdin)
	if vendor == "codex" {
		fmt.Println(`{"type":"thread.started","thread_id":"explicit-profile-fixture"}`)
		fmt.Println(`{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":0}}`)
	} else if vendor == "claude" {
		fmt.Println(`{"type":"system","subtype":"init","session_id":"explicit-profile-fixture"}`)
		fmt.Println(`{"type":"result","subtype":"success","session_id":"explicit-profile-fixture"}`)
	} else {
		fmt.Println(`{"type":"session","id":"explicit-profile-fixture"}`)
		fmt.Println(`{"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"profile fixture"}]}}`)
		fmt.Println(`{"type":"agent_end","willRetry":false}`)
		fmt.Println(`{"type":"agent_settled","aborted":false}`)
	}
	os.Exit(0)
}

func TestExplicitCLIConfigurationReachesOnlySelectedChildProfile(t *testing.T) {
	for _, key := range []string{"HOME", "CODEX_HOME", "CLAUDE_CONFIG_DIR", "PI_CODING_AGENT_DIR", "ALVA_AMBIENT_PROFILE_MARKER"} {
		t.Setenv(key, "ambient-must-not-be-inherited")
	}
	for _, vendor := range []string{"codex", "claude", "pi"} {
		for _, explicit := range []bool{false, true} {
			t.Run(fmt.Sprintf("%s/explicit=%t", vendor, explicit), func(t *testing.T) {
				binary, err := os.Executable()
				if err != nil {
					t.Fatal(err)
				}
				directory := filepath.Join(t.TempDir(), "private")
				store, err := bindings.Open(t.Context(), directory)
				if err != nil {
					t.Fatal(err)
				}
				directory, err = filepath.EvalSymlinks(directory)
				if err != nil {
					t.Fatal(err)
				}
				var selected string
				if explicit {
					selected, err = filepath.EvalSymlinks(t.TempDir())
					if err != nil {
						t.Fatal(err)
					}
				}
				a, err := New(Config{Bindings: bindingPorts{store}, Vendor: vendor, Command: binary,
					PrefixArgs: []string{"-test.run=^TestCLIProfileFixtureProcess$", "--"}, StateDirectory: directory,
					CLIConfigDirectory: selected, ApprovedHostAccess: true,
					Env: map[string]string{"ALVA_PROFILE_VENDOR": vendor, "ALVA_EXPECTED_HOME": filepath.Join(directory, "home"), "ALVA_EXPECTED_PROFILE": selected}})
				if err != nil {
					store.Close()
					t.Fatal(err)
				}
				defer a.Close(context.Background())
				var outcome engine.Outcome
				err = a.Execute(t.Context(), engine.Request{OwnerID: "fixture-owner", Prompt: "profile routing fixture", Workspace: t.TempDir(), AccountSource: a.AccountSource()}, func(event engine.Event) error {
					if event.Kind == engine.Completed {
						outcome = event.Outcome
					}
					return nil
				})
				if err != nil || outcome != engine.Succeeded {
					t.Fatal("profile did not reach the actual child with private HOME and other profiles", outcome, err)
				}
				if strings.Contains(a.AccountSource(), directory) || selected != "" && strings.Contains(a.AccountSource(), selected) {
					t.Fatal("account source exposes a host path")
				}
			})
		}
	}
}

func TestCLIConfigurationValidationAndEnvironmentCannotEscapeProfile(t *testing.T) {
	store, err := bindings.Open(t.Context(), filepath.Join(t.TempDir(), "bindings"))
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	regular := filepath.Join(t.TempDir(), "file")
	if err := os.WriteFile(regular, nil, 0600); err != nil {
		t.Fatal(err)
	}
	base := Config{Bindings: bindingPorts{store}, Vendor: "codex", Command: "not-started", ApprovedHostAccess: true, CLIConfigDirectory: t.TempDir()}
	for _, mutate := range []func(*Config){
		func(c *Config) { c.ApprovedHostAccess = false },
		func(c *Config) { c.CLIConfigDirectory = "relative" },
		func(c *Config) { c.CLIConfigDirectory = filepath.Join(t.TempDir(), "missing") },
		func(c *Config) { c.CLIConfigDirectory = regular },
	} {
		config := base
		config.StateDirectory = filepath.Join(t.TempDir(), "not-created")
		mutate(&config)
		if _, err := New(config); err == nil {
			t.Fatal("invalid explicit profile accepted")
		}
		if _, err := os.Stat(config.StateDirectory); !os.IsNotExist(err) {
			t.Fatal("invalid profile created private engine data", err)
		}
	}
	for _, key := range []string{"HOME", "CODEX_HOME", "CLAUDE_CONFIG_DIR", "PI_CODING_AGENT_DIR", "codex_home", "XDG_CONFIG_HOME"} {
		config := base
		config.StateDirectory = filepath.Join(t.TempDir(), "not-created")
		config.Env = map[string]string{key: t.TempDir()}
		if _, err := New(config); err == nil {
			t.Fatal("profile environment override accepted", key)
		}
	}
}

func TestCLIAccountSourceRejectsDifferentSavedProfileBeforeProcessIO(t *testing.T) {
	a, directory := fixtureAdapter(t, "codex", "ok")
	request := engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "must not start", AccountSource: "different profile"}
	if err := a.Execute(t.Context(), request, func(engine.Event) error { t.Fatal("unexpected process event"); return nil }); err == nil {
		t.Fatal("saved account source silently used another CLI profile")
	}
	if _, err := os.Stat(filepath.Join(directory, "journals")); !os.IsNotExist(err) {
		t.Fatal("mismatched account created execution journal", err)
	}
}

func TestCodexFreshAndResumeBothKeepExplicitReadOnlySandbox(t *testing.T) {
	a, _ := fixtureAdapter(t, "codex", "ok")
	for _, resume := range []string{"", "fixture-native"} {
		args, err := a.args(engine.Request{Workspace: "/explicit/workspace", ResumeSession: resume}, "fixture")
		if err != nil {
			t.Fatal(err)
		}
		if len(args) < 5 || args[2] != "exec" || args[3] != "--sandbox" || args[4] != "read-only" {
			t.Fatal("Codex exec parent lost the explicit read-only sandbox", args)
		}
		if resume != "" && (args[5] != "resume" || args[6] != resume) {
			t.Fatal("read-only sandbox was not applied before native resume", args)
		}
	}
}

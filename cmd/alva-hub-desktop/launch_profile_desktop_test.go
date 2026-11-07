// INPUT: Isolated app bundle manifests, approved fixture CLI paths and temporary Go product stores
// OUTPUT: No-argument installed startup, explicit terminal override, strict profile and source-admission regressions
// POS: [L7 Hub product] Verifies the actual cold bootstrap path without installed vendor processes, credentials, paid requests or windows
//go:build desktop && !smoke

package main

import (
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/platform/system/process"
)

func installedDesktopFixture(t *testing.T, manifest []byte) string {
	t.Helper()
	contents := filepath.Join(t.TempDir(), "Alva.app", "Contents")
	for _, directory := range []string{"MacOS", "Resources"} {
		if err := os.MkdirAll(filepath.Join(contents, directory), 0700); err != nil {
			t.Fatal(err)
		}
	}
	executable := filepath.Join(contents, "MacOS", "Alva")
	if err := os.WriteFile(executable, nil, 0700); err != nil {
		t.Fatal(err)
	}
	if manifest != nil {
		if err := os.WriteFile(filepath.Join(contents, "Resources", desktopLaunchProfileName), manifest, 0600); err != nil {
			t.Fatal(err)
		}
	}
	return executable
}

func TestInstalledNoArgumentDesktopRegistersThreeColdEngines(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "product")
	codexConfig, piConfig := t.TempDir(), t.TempDir()
	path := filepath.Join(t.TempDir(), "node", "bin") + string(os.PathListSeparator) + process.SafePath
	launch := desktopLaunchProfile{Version: 1, Arguments: []string{"--data-dir", directory, "--demo", "--codex-command", "/explicit/not-started-codex", "--codex-config-dir", codexConfig, "--pi-command", "/explicit/not-started-pi", "--pi-config-dir", piConfig, "--pi-transport", "rpc", "--external-host-approved"}, ExternalEnvironment: map[string]desktopExternalEnvironment{"pi": {Path: path}}}
	data, err := json.Marshal(launch)
	if err != nil {
		t.Fatal(err)
	}
	executable := installedDesktopFixture(t, data)
	startup, err := prepareDesktopFromExecutable(t.Context(), executable, nil, nil, io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	defer startup.close(t.Context())
	for _, vendor := range []string{"alva", "codex", "pi"} {
		descriptor, ok := startup.application.Runtime.Descriptor(vendor)
		if !ok || vendor != "alva" && !descriptor.Runtime.Capabilities().PermissionCallback {
			t.Fatal("installed engine unavailable or not approval-capable", vendor)
		}
		found := false
		for _, target := range startup.application.Targets {
			found = found || target.EngineID == vendor
		}
		if !found {
			t.Fatal("registered engine has no selectable target", vendor)
		}
	}
	if startup.application.Runtime.HasActive() {
		t.Fatal("installed launch started engine work")
	}
	sessions, err := startup.application.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 0 {
		t.Fatal("installed launch created sessions", sessions, err)
	}
	snapshots := externalSnapshots("", "", "/explicit/not-started-pi", "", "", piConfig, "app-server", "workspace-write", "on-request", "rpc", true)
	if err = applyDesktopLaunchEnvironment(snapshots, launch.ExternalEnvironment); err != nil || snapshots[0].Env["PATH"] != path || snapshots[0].CLIConfigDirectory != piConfig || snapshots[0].Transport != "rpc" {
		t.Fatal("explicit Pi runtime/configuration source was lost", err)
	}
}

func TestInstalledProfileNeverOverridesExplicitTerminalFlags(t *testing.T) {
	executable := installedDesktopFixture(t, []byte("invalid installed JSON"))
	directory := filepath.Join(t.TempDir(), "explicit-product")
	startup, err := prepareDesktopFromExecutable(t.Context(), executable, []string{"--data-dir", directory, "--demo"}, nil, io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	defer startup.close(t.Context())
	if startup.application.Runtime.HasActive() {
		t.Fatal("explicit terminal startup started work")
	}
	plainExecutable := filepath.Join(t.TempDir(), "alva-hub-desktop")
	if err = os.WriteFile(plainExecutable, nil, 0700); err != nil {
		t.Fatal(err)
	}
	if startup, err = prepareDesktopFromExecutable(t.Context(), plainExecutable, nil, nil, io.Discard); err == nil || startup != nil || !strings.Contains(err.Error(), "--data-dir is required") {
		t.Fatal("ordinary CLI acquired implicit installed defaults", err)
	}
}

func TestInstalledProfileRejectsAmbiguityAndUnapprovedSourcesBeforeStorage(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "not-created")
	base := desktopLaunchProfile{Version: 1, Arguments: []string{"--data-dir", directory, "--demo"}}
	marshal := func(value desktopLaunchProfile) []byte {
		data, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		return data
	}
	cases := [][]byte{nil, []byte(`{"version":1,"version":1,"arguments":["--demo"]}`), []byte(`{"Version":1,"arguments":["--demo"]}`), []byte(`{"version":2,"arguments":["--demo"]}`), []byte(`{"version":1,"arguments":["--demo"],"unexpected":true}`), []byte(`{"version":1,"arguments":["--demo"],"external_environment":{"pi":{"HOME":"/tmp"}}}`), []byte(strings.Repeat(" ", (64<<10)+1))}
	for _, environment := range []map[string]desktopExternalEnvironment{{"pi": {Path: process.SafePath}}, {"pi": {Path: "relative/bin"}}, {"pi": {Path: process.SafePath + string(os.PathListSeparator)}}} {
		launch := base
		launch.ExternalEnvironment = environment
		cases = append(cases, marshal(launch))
	}
	for _, args := range [][]string{{"--api-key-stdin"}, {"--startup-secrets-stdin=true"}, {"--pi-command", "/explicit/not-started-pi", "--pi-config-dir", t.TempDir()}, {"--pi-command", "/explicit/not-started-pi", "--pi-config-dir", filepath.Join(t.TempDir(), "missing"), "--external-host-approved"}} {
		launch := base
		launch.Arguments = append(append([]string(nil), base.Arguments...), args...)
		cases = append(cases, marshal(launch))
	}
	for i, manifest := range cases {
		executable := installedDesktopFixture(t, manifest)
		if startup, err := prepareDesktopFromExecutable(t.Context(), executable, nil, nil, io.Discard); err == nil || startup != nil {
			t.Fatalf("invalid installed profile %d accepted: %v", i, err)
		}
		if _, err := os.Stat(directory); !os.IsNotExist(err) {
			t.Fatalf("invalid installed profile %d created storage: %v", i, err)
		}
	}
}

func TestPiTransportFlagsRemainExplicitAndCold(t *testing.T) {
	for _, transport := range []string{"rpc", "exec"} {
		startup, err := prepareDesktop(t.Context(), []string{"--data-dir", filepath.Join(t.TempDir(), "product"), "--pi-command", "/explicit/not-started-pi", "--pi-transport", transport, "--external-host-approved"}, nil, io.Discard)
		if err != nil {
			t.Fatal(err)
		}
		descriptor, ok := startup.application.Runtime.Descriptor("pi")
		if !ok || descriptor.Runtime.Capabilities().PermissionCallback != (transport == "rpc") || startup.application.Runtime.HasActive() {
			t.Fatal("explicit Pi transport ignored or started work", transport)
		}
		if err := startup.close(t.Context()); err != nil {
			t.Fatal(err)
		}
	}
	for _, flags := range [][]string{{"--pi-transport", "rpc"}, {"--pi-command", "/explicit/not-started-pi", "--pi-transport", "unknown"}} {
		directory := filepath.Join(t.TempDir(), "not-created")
		if startup, err := prepareDesktop(t.Context(), append([]string{"--data-dir", directory}, flags...), nil, io.Discard); err == nil || startup != nil {
			t.Fatal("invalid Pi transport flags accepted", flags, err)
		}
		if _, err := os.Stat(directory); !os.IsNotExist(err) {
			t.Fatal("invalid Pi transport created storage", err)
		}
	}
}

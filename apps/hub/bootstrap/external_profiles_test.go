// INPUT: Explicit temporary CLI directories, local subprocess fixture and temporary product storage
// OUTPUT: Canonical external account route identity, changed-profile isolation and retained-native early cancellation
// POS: [L7 Hub product] Verify profile selection binds durable targets without reading credentials or running user CLIs.
package bootstrap

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestBootstrapCLIProfileFixtureProcess(t *testing.T) {
	if os.Getenv("ALVA_BOOTSTRAP_PROFILE_FIXTURE") == "" {
		return
	}
	vendor := os.Getenv("ALVA_BOOTSTRAP_PROFILE_VENDOR")
	profileKey := "CODEX_HOME"
	if vendor == "pi" {
		profileKey = "PI_CODING_AGENT_DIR"
	}
	if os.Getenv(profileKey) != os.Getenv("ALVA_EXPECTED_PROFILE") || os.Getenv("HOME") == os.Getenv(profileKey) {
		os.Exit(91)
	}
	if err := os.WriteFile(os.Getenv("ALVA_PROFILE_START_MARKER"), []byte("fixture started"), 0600); err != nil {
		os.Exit(92)
	}
	input, _ := io.ReadAll(os.Stdin)
	if strings.HasSuffix(string(input), "fixture pause before native header\n") {
		if err := os.WriteFile(os.Getenv("ALVA_PROFILE_START_MARKER")+".waiting", nil, 0600); err != nil {
			os.Exit(93)
		}
		for {
			time.Sleep(time.Second)
		}
	}
	if vendor == "pi" {
		for i, arg := range os.Args {
			if arg != "--session" || i+1 >= len(os.Args) {
				continue
			}
			path := os.Args[i+1]
			info, err := os.Stat(path)
			if err != nil {
				os.Exit(94)
			}
			if info.Size() == 0 {
				cwd, _ := os.Getwd()
				header, _ := json.Marshal(map[string]string{"type": "session", "id": "bootstrap-explicit-profile", "cwd": cwd})
				if err := os.WriteFile(path, append(header, '\n'), 0600); err != nil {
					os.Exit(95)
				}
			}
		}
		fmt.Println(`{"type":"session","id":"bootstrap-explicit-profile"}`)
		fmt.Println(`{"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"Pi profile fixture"}]}}`)
		fmt.Println(`{"type":"agent_end","willRetry":false}`)
		fmt.Println(`{"type":"agent_settled","aborted":false}`)
	} else {
		fmt.Println(`{"type":"thread.started","thread_id":"bootstrap-explicit-profile"}`)
		fmt.Println(`{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":0}}`)
	}
	os.Exit(0)
}

func externalProfileConfig(t *testing.T, directory, profile, marker string) Config {
	t.Helper()
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	return Config{OpenStores: testStores, OpenBindings: LocalBindings, DataDirectory: directory, Demo: true,
		External: []ExternalEngine{{Vendor: "codex", Command: binary, PrefixArgs: []string{"-test.run=^TestBootstrapCLIProfileFixtureProcess$", "--"}, ApprovedHostAccess: true,
			CLIConfigDirectory: profile, Env: map[string]string{"ALVA_BOOTSTRAP_PROFILE_FIXTURE": "1", "ALVA_EXPECTED_PROFILE": profile, "ALVA_PROFILE_START_MARKER": marker}}}}
}

func TestExternalCLIConfigurationTargetUsesCanonicalPathDigest(t *testing.T) {
	profile, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	marker := filepath.Join(t.TempDir(), "never-started")
	config := externalProfileConfig(t, filepath.Join(t.TempDir(), "data"), profile, marker)
	a, err := Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256([]byte(profile))
	source := a.Targets[1].AccountSource
	if source != "explicit CLI configuration sha256:"+hex.EncodeToString(digest[:]) || strings.Contains(source, profile) {
		t.Fatal("external target is not bound to the actual canonical configuration digest", source)
	}
	closeApp(t, a)
	config.External[0].CLIConfigDirectory = ""
	a, err = Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	if a.Targets[1].AccountSource == source || a.Targets[1].AccountSource != "explicit executable; private HOME" {
		t.Fatal("private default profile shares an explicit configuration identity")
	}
	closeApp(t, a)
	alias := filepath.Join(t.TempDir(), "profile-alias")
	if err := os.Symlink(profile, alias); err != nil {
		t.Skip("directory symlinks unavailable", err)
	}
	config.External[0].CLIConfigDirectory = alias
	a, err = Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	if a.Targets[1].AccountSource != source || config.External[0].CLIConfigDirectory != alias {
		t.Fatal("directory alias changed account identity or assembly mutated caller configuration")
	}
	if _, err := os.Stat(marker); !os.IsNotExist(err) {
		t.Fatal("cold configuration started an external CLI", err)
	}
}

func TestChangedExternalCLIConfigurationRejectsSavedBranchBeforeStartingChild(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "data")
	profileA, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	profileB, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	marker := filepath.Join(t.TempDir(), "child-started")
	config := externalProfileConfig(t, directory, profileA, marker)
	a, err := Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	session, err := a.Service.Create(t.Context(), "explicit CLI profile route", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	previous := a.Targets[1]
	if _, err := a.Store.Switch(t.Context(), session.ID, previous, "explicit fixture profile selection"); err != nil {
		t.Fatal(err)
	}
	closeApp(t, a)
	config.External[0].CLIConfigDirectory = profileB
	config.External[0].Env["ALVA_EXPECTED_PROFILE"] = profileB
	a, err = Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	if hub.RouteKey(a.Targets[1]) == hub.RouteKey(previous) {
		t.Fatal("different configured profiles share the same durable route")
	}
	_, done, err := a.Runtime.Start(t.Context(), session.ID, "must not execute through another profile")
	if err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-done:
		if err == nil || !strings.Contains(err.Error(), "account source") {
			t.Fatal("saved route silently used the newly configured account", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("stale external route did not finish")
	}
	if _, err := os.Stat(marker); !os.IsNotExist(err) {
		t.Fatal("stale saved account launched a process", err)
	}
	if _, err := a.Store.Switch(t.Context(), session.ID, a.Targets[1], "explicit new fixture profile selection"); err != nil {
		t.Fatal(err)
	}
	_, done, err = a.Runtime.Start(context.Background(), session.ID, "use the explicitly selected profile")
	if err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("explicit configured route did not finish")
	}
	detail, err := a.Store.Detail(t.Context(), session.ID)
	if err != nil || detail.Session.Status != engine.Succeeded {
		t.Fatal("selected configured route did not succeed", err)
	}
	if _, err := os.Stat(marker); err != nil {
		t.Fatal("explicitly selected route never reached the local child", err)
	}
}

func TestInvalidExternalCLIConfigurationDoesNotOpenStorage(t *testing.T) {
	for _, entry := range []ExternalEngine{
		{Vendor: "codex", Command: "not-started", CLIConfigDirectory: t.TempDir()},
		{Vendor: "codex", Command: "not-started", ApprovedHostAccess: true, CLIConfigDirectory: "relative"},
		{Vendor: "claude", Command: "not-started", ApprovedHostAccess: true, CLIConfigDirectory: filepath.Join(t.TempDir(), "missing")},
		{Vendor: "pi", Command: "not-started", ApprovedHostAccess: true, CLIConfigDirectory: "relative"},
	} {
		directory := filepath.Join(t.TempDir(), "not-created")
		config := Config{OpenStores: func(context.Context, string) (Stores, error) {
			t.Fatal("invalid profile reached storage factory")
			return Stores{}, nil
		}, DataDirectory: directory, Demo: true, External: []ExternalEngine{entry}}
		if _, err := Open(t.Context(), config); err == nil {
			t.Fatal("invalid profile accepted")
		}
		if _, err := os.Stat(directory); !os.IsNotExist(err) {
			t.Fatal("invalid profile created product storage", err)
		}
	}
}

func TestExternalResumeCancelledBeforeNativeHeaderKeepsRetainedJournalIdentity(t *testing.T) {
	for _, vendor := range []string{"codex", "pi"} {
		t.Run(vendor, func(t *testing.T) { testExternalResumeCancelledBeforeNativeHeader(t, vendor) })
	}
}

func testExternalResumeCancelledBeforeNativeHeader(t *testing.T, vendor string) {
	profile, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	marker := filepath.Join(t.TempDir(), "started")
	config := externalProfileConfig(t, filepath.Join(t.TempDir(), "data"), profile, marker)
	config.External[0].Vendor = vendor
	config.External[0].Env["ALVA_BOOTSTRAP_PROFILE_VENDOR"] = vendor
	a, err := Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	session, err := a.Service.Create(t.Context(), "early native resume cancellation", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	branch, err := a.Store.Switch(t.Context(), session.ID, a.Targets[1], "explicit fixture route")
	if err != nil {
		t.Fatal(err)
	}
	wait := func(done <-chan error) error {
		t.Helper()
		select {
		case err := <-done:
			return err
		case <-time.After(5 * time.Second):
			t.Fatal("external fixture execution did not finish")
			return nil
		}
	}
	_, _, done, err := a.Runtime.StartOnBranchOwned(t.Context(), session.ID, branch.ID, "first native fixture turn")
	if err != nil {
		t.Fatal(err)
	}
	if err := wait(done); err != nil {
		t.Fatal(err)
	}
	_, execution, done, err := a.Runtime.StartOnBranchOwned(t.Context(), session.ID, branch.ID, "fixture pause before native header")
	if err != nil {
		t.Fatal(err)
	}
	ready := marker + ".waiting"
	deadline := time.Now().Add(5 * time.Second)
	for {
		if _, err := os.Stat(ready); err == nil {
			break
		} else if !os.IsNotExist(err) {
			t.Fatal(err)
		}
		if time.Now().After(deadline) {
			t.Fatal("resumed child did not reach its pre-header barrier")
		}
		time.Sleep(time.Millisecond)
	}
	if err := a.Runtime.CancelExecution(t.Context(), session.ID, execution); err != nil {
		t.Fatal(err)
	}
	if err := wait(done); err == nil {
		t.Fatal("cancelled execution reported success")
	}
	detail, err := a.Store.Detail(t.Context(), session.ID)
	if err != nil || detail.Session.Status != engine.Cancelled || detail.Session.ActiveBranchID != branch.ID {
		t.Fatal("early resume cancellation failed durable product state", detail.Session, err)
	}
	turn, err := a.Store.BranchTurn(t.Context(), session.ID, branch.ID)
	if err != nil || turn.Status != engine.Cancelled || turn.ActiveAttemptID != execution {
		t.Fatal("early resume cancellation failed durable Turn state", turn, err)
	}
	for _, retained := range detail.Branches {
		if retained.ID == branch.ID && retained.NativeSessionID != "bootstrap-explicit-profile" {
			t.Fatal("early cancellation lost the existing native identity", retained)
		}
	}
	history, err := a.Store.History(t.Context(), session.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	var errorCount, terminals int
	for _, entry := range history {
		if entry.AttemptID != execution || entry.Event.Kind == "" {
			continue
		}
		event := entry.Event
		if event.Kind == engine.SessionStarted || event.SessionID != "" {
			t.Fatal("retained journal identity fabricated a native start", event)
		}
		if event.Reference == nil || event.Reference.SessionID != "bootstrap-explicit-profile" || !strings.HasPrefix(event.Reference.URI, "engine-journal://") {
			t.Fatal("pre-header cancellation lost its qualified native journal", event)
		}
		switch event.Kind {
		case engine.Error:
			errorCount++
			if event.Failure != engine.FailureCancelled {
				t.Fatal("cancellation became a strong observation failure", event)
			}
		case engine.Completed:
			terminals++
			if event.Outcome != engine.Cancelled {
				t.Fatal("cancellation terminal was not persisted", event)
			}
		default:
			t.Fatal("unexpected pre-header event", event)
		}
	}
	if errorCount != 1 || terminals != 1 {
		t.Fatal("early resume did not persist one cancellation error and terminal", errorCount, terminals)
	}
	_, _, done, err = a.Runtime.StartOnBranchOwned(t.Context(), session.ID, branch.ID, "continue the same native fixture after Stop")
	if err != nil {
		t.Fatal(err)
	}
	if err := wait(done); err != nil {
		t.Fatal("early Stop prevented a later same-branch resume", err)
	}
	detail, err = a.Store.Detail(t.Context(), session.ID)
	if err != nil || detail.Session.Status != engine.Succeeded || detail.Session.ActiveBranchID != branch.ID {
		t.Fatal("same native branch did not continue", detail.Session, err)
	}
	for _, retained := range detail.Branches {
		if retained.ID == branch.ID && retained.NativeSessionID != "bootstrap-explicit-profile" {
			t.Fatal("continued branch replaced its retained native identity", retained)
		}
	}
	closeApp(t, a)
	a, err = Open(t.Context(), config)
	if err != nil {
		t.Fatal("saved explicit profile failed cold reopen", err)
	}
	defer closeApp(t, a)
	_, _, done, err = a.Runtime.StartOnBranchOwned(t.Context(), session.ID, branch.ID, "continue the retained native fixture after reopen")
	if err != nil {
		t.Fatal(err)
	}
	if err := wait(done); err != nil {
		t.Fatal("product reopen lost exact native resume", err)
	}
	detail, err = a.Store.Detail(t.Context(), session.ID)
	if err != nil || detail.Session.Status != engine.Succeeded || detail.Session.ActiveBranchID != branch.ID {
		t.Fatal("reopened native branch did not continue", detail.Session, err)
	}
	if vendor == "pi" {
		history, err := a.Store.History(t.Context(), session.ID, 0, 128)
		if err != nil {
			t.Fatal(err)
		}
		var answers int
		for _, item := range history {
			if item.Event.Kind == engine.Message && item.Event.Text == "Pi profile fixture" {
				answers++
			}
		}
		if answers != 3 {
			t.Fatal("Pi history did not preserve successful rounds across Stop and reopen", answers)
		}
	}
}

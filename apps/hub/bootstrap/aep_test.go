// INPUT: Actual same-binary AEP subprocess, inspected source pins and shared native Hub assembly
// OUTPUT: No-start config, next-turn launch/cleanup, pin tamper rejection and failed-prepare risk regressions
// POS: [L7 Hub product] Local native Plugin acceptance; never runs user plugins, paid models or ambient credentials
package bootstrap

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/extension/builtin"
	"github.com/QuincySx/Alva/extension/loader"
	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/engine/alva"
	"github.com/QuincySx/Alva/integration/protocol/aep"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

func TestNativeAEPHelper(t *testing.T) {
	if os.Getenv("ALVA_AEP_TEST_CHILD") != "yes" {
		return
	}
	if err := os.WriteFile(os.Getenv("ALVA_AEP_TEST_MARKER"), []byte(fmt.Sprint(os.Getpid())), 0600); err != nil {
		os.Exit(9)
	}
	reader := bufio.NewScanner(os.Stdin)
	reader.Buffer(make([]byte, 4096), aep.FrameLimit+1)
	for reader.Scan() {
		var request struct{ ID, Method string }
		if json.Unmarshal(reader.Bytes(), &request) != nil {
			os.Exit(3)
		}
		if request.Method == "initialize" {
			version := aep.Version
			if os.Getenv("ALVA_AEP_TEST_BAD_HANDSHAKE") == "yes" {
				version = "invalid-fixture-version"
			}
			data, _ := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": request.ID, "result": aep.Handshake{ProtocolVersion: version, Plugin: aep.PluginInfo{Name: "native-fixture", Version: "1"}}})
			fmt.Println(string(data))
			continue
		}
		if request.Method == "initialized" {
			continue
		}
		if request.Method == "shutdown" {
			data, _ := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": request.ID, "result": map[string]any{}})
			fmt.Println(string(data))
			os.Exit(0)
		}
		os.Exit(4)
	}
	os.Exit(0)
}

func TestAEPActualProcessIsClosedIfInitialEngineObservationFails(t *testing.T) {
	ctx := context.Background()
	a, err := Open(ctx, Config{DataDirectory: filepath.Join(t.TempDir(), "private"), Demo: true})
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	session, err := a.Service.Create(ctx, "early observation failure", "")
	if err != nil {
		t.Fatal(err)
	}
	entry, marker, _ := nativeAEPFixture(t, a, session.ID, false)
	entry.Enabled, entry.HostAccessApproved = true, true
	var plugin *loader.Plugin
	var owned *ownedRunner
	adapter := alva.New(func(ctx context.Context, nativeID string, request engine.Request, _ bool) (harness.Runner, error) {
		log, err := a.Repository.Create(ctx, session.ID, nativeID, "")
		if err != nil {
			return nil, err
		}
		workspace, err := builtin.OpenWorkspace(session.Workspace)
		if err != nil {
			return nil, err
		}
		guard, err := security.NewGuard(session.Workspace, nil, workspace, security.NewPermissionManager(security.Ask, nil))
		if err != nil {
			workspace.Close()
			return nil, err
		}
		bundle, err := product.StagePinnedAEP(ctx, entry, filepath.Join(a.Service.DataDirectory, "aep-snapshots"))
		if err != nil {
			workspace.Close()
			return nil, err
		}
		plugin, err = loader.OpenPlugin(ctx, loader.PluginConfig{Bundle: bundle, Interpreter: entry.Interpreter, InterpreterArgs: entry.InterpreterArgs, Env: entry.Env, Workspace: session.Workspace, Repository: a.Repository, Owner: session.ID, NativeID: nativeID, Guard: guard, ApprovedHostAccess: true, BeforeLaunch: func(context.Context) error {
			return request.ReportActivity(engine.Activity{ID: "early-native", Name: "actual pre-Run AEP", Effect: engine.EffectUnknown})
		}})
		if err != nil {
			workspace.Close()
			_ = bundle.Close()
			return nil, err
		}
		runner, err := harness.NewDemoBuilder(log).Workspace(session.Workspace).Plugin(plugin).Build(ctx)
		if err != nil {
			_ = plugin.Close(ctx)
			workspace.Close()
			return nil, err
		}
		owned = &ownedRunner{Runner: runner, workspace: workspace, nativeID: nativeID, resources: []attemptResource{plugin}}
		return owned, nil
	})
	refused := errors.New("fixture SessionStarted durable commit rejected")
	activity := false
	err = adapter.Execute(ctx, engine.Request{Prompt: "must not enter SDK Run"}, func(event engine.Event) error {
		if event.Kind == engine.ExecutionActivity {
			activity = true
			if _, e := os.Stat(marker); !os.IsNotExist(e) {
				t.Fatal("native start preceded Activity admission", e)
			}
			return nil
		}
		if event.Kind == engine.SessionStarted {
			return refused
		}
		t.Fatal("observer replayed after commit failure", event.Kind)
		return nil
	})
	if !errors.Is(err, refused) || !activity || plugin == nil || !plugin.CleanupComplete() || owned == nil || !owned.CleanupComplete() {
		t.Fatal("early observer failure leaked actual Plugin/runner", err)
	}
	if _, err = os.Stat(marker); err != nil {
		t.Fatal("test did not launch actual native helper", err)
	}
	assertAEPSnapshotsRemoved(t, a)
	log, err := a.Repository.OpenSession(ctx, session.ID, owned.nativeID)
	if err != nil {
		t.Fatal(err)
	}
	records, err := log.Records(ctx, 0)
	if err != nil {
		t.Fatal(err)
	}
	for _, record := range records {
		if record.Kind == abi.RecordConversation {
			t.Fatal("SDK ran despite failed SessionStarted observation")
		}
	}
}

func nativeAEPFixture(t *testing.T, a *Application, id string, bad bool) (hub.AEPBundle, string, string) {
	t.Helper()
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	source := t.TempDir()
	if err = os.WriteFile(filepath.Join(source, "alva.toml"), []byte("name='native-fixture'\nversion='1'\nruntime='python'\nentry='plugin.py'\nrequested_capabilities=[]\n"), 0600); err != nil {
		t.Fatal(err)
	}
	entry := filepath.Join(source, "plugin.py")
	if err = os.WriteFile(entry, []byte("# This file is pinned; the same Go test binary implements the explicit local AEP fixture.\n"), 0600); err != nil {
		t.Fatal(err)
	}
	marker := filepath.Join(t.TempDir(), "child-started")
	snapshot := hub.AEPBundle{ID: "fixture", SourceDirectory: source, Interpreter: executable, InterpreterArgs: []string{"-test.run=^TestNativeAEPHelper$", "--"}, Env: map[string]string{"ALVA_AEP_TEST_CHILD": "yes", "ALVA_AEP_TEST_MARKER": marker}}
	if bad {
		snapshot.Env["ALVA_AEP_TEST_BAD_HANDSHAKE"] = "yes"
	}
	raw, _ := json.Marshal([]hub.AEPBundle{snapshot})
	if err = a.Service.SaveAEPDraft(context.Background(), id, string(raw)); err != nil {
		t.Fatal(err)
	}
	settings, err := a.Store.Settings(context.Background(), id)
	if err != nil || len(settings.AEP) != 1 {
		t.Fatal("missing inspected setting", err)
	}
	return settings.AEP[0], marker, entry
}

func waitOutcome(t *testing.T, a *Application, id string, want engine.Outcome) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		detail, err := a.Store.Detail(context.Background(), id)
		if err != nil {
			t.Fatal(err)
		}
		if detail.Session.Status != hub.Running && !a.Runtime.HasActive() {
			if detail.Session.Status != want {
				t.Fatal("unexpected actual terminal", detail.Session.Status, want)
			}
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("native AEP attempt did not terminate")
}

func assertAEPSnapshotsRemoved(t *testing.T, a *Application) {
	t.Helper()
	entries, err := os.ReadDir(filepath.Join(a.Service.DataDirectory, "aep-snapshots"))
	if err != nil || len(entries) != 0 {
		t.Fatal("owned snapshots not removed after actual process completion", len(entries), err)
	}
}

func TestNativeAEPPinApprovalAndNextTurnActualLaunch(t *testing.T) {
	ctx := context.Background()
	a, err := Open(ctx, Config{DataDirectory: filepath.Join(t.TempDir(), "private"), Demo: true})
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	session, err := a.Service.Create(ctx, "native Plugin", "")
	if err != nil {
		t.Fatal(err)
	}
	snapshot, marker, _ := nativeAEPFixture(t, a, session.ID, false)
	if snapshot.Enabled || snapshot.HostAccessApproved || len(snapshot.SHA256) != 64 || snapshot.Name != "native-fixture" {
		t.Fatal("inspection self-approved or omitted pin", snapshot)
	}
	if _, err = os.Stat(marker); !os.IsNotExist(err) || a.Runtime.HasActive() {
		t.Fatal("inspection started native code", err)
	}
	assertAEPSnapshotsRemoved(t, a)
	if err = a.Store.SetAEPEnabled(ctx, session.ID, snapshot.ID, true, false); err == nil {
		t.Fatal("enable skipped independent native host consent")
	}
	if err = a.Store.SetAEPEnabled(ctx, session.ID, snapshot.ID, true, true); err != nil {
		t.Fatal(err)
	}
	if _, err = os.Stat(marker); !os.IsNotExist(err) {
		t.Fatal("approval launched native code", err)
	}
	if _, err = a.Service.Submit(ctx, session.ID, "local demonstration with explicitly approved fixture plugin"); err != nil {
		t.Fatal(err)
	}
	detail := awaitTerminal(t, a, session.ID)
	if _, err = os.Stat(marker); err != nil {
		t.Fatal("approved next-turn native Plugin never started", err)
	}
	assertAEPSnapshotsRemoved(t, a)
	rows, err := a.Store.History(ctx, session.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	activity := uint64(0)
	started := uint64(0)
	for _, row := range rows {
		if row.Event.Kind == engine.ExecutionActivity {
			activity = row.Sequence
			if row.Event.Activity == nil || row.Event.Activity.Effect != engine.EffectUnknown {
				t.Fatal("native startup falsely read-only")
			}
		}
		if row.Event.Kind == engine.SessionStarted {
			started = row.Sequence
		}
	}
	if activity == 0 || started <= activity {
		t.Fatal("native startup risk was not committed before Agent start", activity, started)
	}
	page, err := a.Service.Inspector(ctx, session.ID, detail.Session.ActiveBranchID, "", 0, 0)
	if err != nil || len(page.Entries) == 0 {
		t.Fatal("native Plugin facts have no bounded Inspector entry", err)
	}
}

func TestNativeAEPPinChangesFailBeforeAnyProcessStarts(t *testing.T) {
	ctx := context.Background()
	a, err := Open(ctx, Config{DataDirectory: filepath.Join(t.TempDir(), "private"), Demo: true})
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	session, err := a.Service.Create(ctx, "tampered code", "")
	if err != nil {
		t.Fatal(err)
	}
	snapshot, marker, entry := nativeAEPFixture(t, a, session.ID, false)
	if err = a.Store.SetAEPEnabled(ctx, session.ID, snapshot.ID, true, true); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(entry, []byte("# Changed after explicit inspection\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = a.Service.Submit(ctx, session.ID, "must fail closed"); err != nil {
		t.Fatal(err)
	}
	waitOutcome(t, a, session.ID, engine.Failed)
	if _, err = os.Stat(marker); !os.IsNotExist(err) {
		t.Fatal("changed source was executed", err)
	}
	assertAEPSnapshotsRemoved(t, a)
	rows, err := a.Store.History(ctx, session.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	for _, row := range rows {
		if row.Event.Kind == engine.ExecutionActivity || row.Event.Kind == engine.SessionStarted {
			t.Fatal("pin rejection falsely claimed process/Agent startup", row.Event)
		}
	}
}

func TestNativeAEPHandshakeFailureIsDurableAndOwned(t *testing.T) {
	ctx := context.Background()
	a, err := Open(ctx, Config{DataDirectory: filepath.Join(t.TempDir(), "private"), Demo: true})
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	session, err := a.Service.Create(ctx, "failed handshake", "")
	if err != nil {
		t.Fatal(err)
	}
	snapshot, marker, _ := nativeAEPFixture(t, a, session.ID, true)
	if err = a.Store.SetAEPEnabled(ctx, session.ID, snapshot.ID, true, true); err != nil {
		t.Fatal(err)
	}
	if _, err = a.Service.Submit(ctx, session.ID, "local failure fixture"); err != nil {
		t.Fatal(err)
	}
	waitOutcome(t, a, session.ID, engine.Failed)
	if _, err = os.Stat(marker); err != nil {
		t.Fatal("bad-handshake fixture did not actually launch", err)
	}
	assertAEPSnapshotsRemoved(t, a)
	rows, err := a.Store.History(ctx, session.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	activity, fatal, completed := false, false, false
	for _, row := range rows {
		activity = activity || row.Event.Kind == engine.ExecutionActivity
		fatal = fatal || row.Event.Kind == engine.Error && strings.Contains(row.Event.Text, "AEP")
		completed = completed || row.Event.Kind == engine.Completed && row.Event.Outcome == engine.Failed
		if row.Event.Kind == engine.SessionStarted {
			t.Fatal("failed build claimed Agent started")
		}
	}
	if !activity || !fatal || !completed {
		t.Fatal("failed preparation lost real causal terminal facts", activity, fatal, completed)
	}
	detail, err := a.Store.Detail(ctx, session.ID)
	if err != nil {
		t.Fatal(err)
	}
	page, err := a.Service.Inspector(ctx, session.ID, detail.Session.ActiveBranchID, "", 0, 0)
	if err != nil || page.NativeID == "" || len(page.Entries) == 0 {
		t.Fatal("failed native build facts lost their actual owner/Inspector route", err)
	}
}

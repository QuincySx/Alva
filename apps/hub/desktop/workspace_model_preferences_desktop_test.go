// INPUT: Actual MyGo model controls and shared SQLite-backed settings/preference snapshots.
// OUTPUT: Scoped saves, revision-safe observation, cross-window drafts and truthful read-failure retries.
// POS: [L7 Hub UI tests] No OS window, model account, network request or implicit session creation.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"math"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

func TestWorkspaceModelPreferencesActualControlsAndCancel(t *testing.T) {
	s, _ := connectionsFixture(t)
	if err := s.Runtime.RegisterDescriptor("alva", engine.Descriptor{Runtime: &settingsRunningEngine{}, Config: engine.ConfigSurface{ProviderProtocol: "native", SupportsGeneration: true, SupportsSystemPrompt: true, SupportsMaxTurns: true}}); err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	p.model.Draft = "keep my prompt"
	tester := ui.NewTester(workspaceStaticView(p), 960, 680)
	connectionClick(t, tester, "Agent and model")
	connectionClick(t, tester, "Favorite model: "+s.DefaultTarget.ModelID)
	waitWorkspace(t, p, tester, func() bool { return !p.composer.modelPicker.saving })
	if !s.Connections.ModelPreference(s.DefaultTarget).Favorite {
		t.Fatal("favorite click was not committed")
	}
	connectionClick(t, tester, "Filter models: 收藏")
	if len(p.composer.modelPicker.visible) != 1 {
		t.Fatal("saved favorite missing from filter")
	}
	connectionClick(t, tester, "Model options: "+s.DefaultTarget.ModelID)
	connectionClick(t, tester, "Edit model defaults: "+s.DefaultTarget.ModelID)
	connectionType(t, tester, "Model default temperature", "0.4")
	connectionType(t, tester, "Model default max-tokens", "1024")
	connectionClick(t, tester, "Save model defaults")
	waitWorkspace(t, p, tester, func() bool { return !p.composer.modelPicker.saving })
	pref := s.Connections.ModelPreference(s.DefaultTarget)
	if p.composer.modelPicker.defaults.open || pref.Options.Generation.Temperature == nil || math.Abs(*pref.Options.Generation.Temperature-.4) > .001 || pref.Options.Generation.MaxTokens == nil || *pref.Options.Generation.MaxTokens != 1024 || !pref.Favorite {
		t.Fatalf("incomplete committed defaults: %+v, %s", pref, p.composer.modelPicker.defaults.problem)
	}
	p.openWorkspaceModelDefaults(s.DefaultTarget)
	tester.Frame()
	connectionType(t, tester, "Model default temperature", "NaN")
	connectionClick(t, tester, "Save model defaults")
	if p.composer.modelPicker.defaults.problem == "" || p.composer.modelPicker.saving {
		t.Fatal("invalid numeric value entered storage")
	}
	connectionClick(t, tester, "Cancel model defaults")
	if actual := s.Connections.ModelPreference(s.DefaultTarget); actual.Revision != pref.Revision || *actual.Options.Generation.Temperature != .4 {
		t.Fatal("cancel/invalid draft altered committed defaults")
	}
	rows, err := s.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 0 || p.model.Draft != "keep my prompt" || s.Runtime.HasActive() {
		t.Fatal("preferences selected, sent or created a session", err)
	}
}

func TestWorkspaceSessionModelOptionsSaveCancelConflictClearAndSwitch(t *testing.T) {
	s, _ := connectionsFixture(t)
	if err := s.Runtime.RegisterDescriptor("alva", engine.Descriptor{Runtime: &settingsRunningEngine{}, Config: engine.ConfigSurface{ProviderProtocol: "native", SupportsGeneration: true, SupportsSystemPrompt: true, SupportsMaxTurns: true}}); err != nil {
		t.Fatal(err)
	}
	temperature := .3
	pref := s.Connections.ModelPreference(s.DefaultTarget)
	pref.Options.Generation.Temperature = &temperature
	if _, err := s.Connections.SaveModelPreference(t.Context(), s.DefaultTarget, pref); err != nil {
		t.Fatal(err)
	}
	first, err := s.Create(t.Context(), "first scope", "")
	if err != nil {
		t.Fatal(err)
	}
	second, err := s.Create(t.Context(), "other scope", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.selectSession(first.ID)
	waitWorkspace(t, p, tester, func() bool { return !p.polling && p.detail.Session.ID == first.ID })
	open := func() {
		t.Helper()
		p.openWorkspaceModelOptionScope(s.DefaultTarget, "当前会话")
		waitWorkspace(t, p, tester, func() bool { return !p.composer.modelPicker.saving })
	}
	open()
	_, sessionScopeVisible := tester.Find("Session model options scope")
	if !sessionScopeVisible || p.composer.modelPicker.defaults.temperature != "0.3" {
		t.Fatal("scope or inherited default missing", p.composer.modelPicker.defaults)
	}
	connectionType(t, tester, "Model default temperature", "0.6")
	p.composer.modelPicker.defaults.effort = "high"
	connectionClick(t, tester, "Save model defaults")
	waitWorkspace(t, p, tester, func() bool { return !p.composer.modelPicker.saving })
	value, err := s.SessionModelOptions(t.Context(), first.ID)
	actual, found := value.Options(s.DefaultTarget)
	if err != nil || !found || actual.Generation.Temperature == nil || *actual.Generation.Temperature != .6 || *s.Connections.ModelPreference(s.DefaultTarget).Options.Generation.Temperature != .3 {
		t.Fatal("session save changed global defaults", value, err)
	}
	connectionClick(t, tester, "Agent and model")
	connectionClick(t, tester, "Model options: "+s.DefaultTarget.ModelID)
	if p.composer.modelPicker.inlineOptions.effort != "high" || p.workspaceTargetOptions(s.DefaultTarget).Generation.Temperature == nil || *p.workspaceTargetOptions(s.DefaultTarget).Generation.Temperature != .6 {
		t.Fatal("first save was missing from reopened inline options or next-Turn preflight")
	}
	tester.Key(0, ui.KeyEscape)
	tester.Key(0, ui.KeyEscape)
	open()
	connectionType(t, tester, "Model default temperature", "0.8")
	connectionClick(t, tester, "Cancel model defaults")
	unchanged, err := s.SessionModelOptions(t.Context(), first.ID)
	if err != nil || unchanged.Revision != value.Revision {
		t.Fatal("cancel committed session draft", err)
	}
	open()
	external := hub.ModelOptions{SystemPrompt: "other window commit"}
	if _, err = s.SaveSessionModelOptions(t.Context(), hub.SessionModelOptionsUpdate{SessionID: first.ID, BranchID: first.ActiveBranchID, Target: s.DefaultTarget, ExpectedRevision: value.Revision, Options: &external}); err != nil {
		t.Fatal(err)
	}
	connectionType(t, tester, "Model default temperature", "0.7")
	connectionClick(t, tester, "Save model defaults")
	waitWorkspace(t, p, tester, func() bool { return !p.composer.modelPicker.saving })
	if !p.composer.modelPicker.defaults.open || p.composer.modelPicker.defaults.problem == "" || p.composer.modelPicker.defaults.temperature != "0.7" {
		t.Fatal("conflict erased draft or reported success")
	}
	connectionClick(t, tester, "Reload model defaults")
	waitWorkspace(t, p, tester, func() bool { return !p.composer.modelPicker.saving })
	if p.composer.modelPicker.defaults.prompt != external.SystemPrompt {
		t.Fatal("explicit reload missed committed override")
	}
	connectionClick(t, tester, "Clear session model options")
	waitWorkspace(t, p, tester, func() bool { return !p.composer.modelPicker.saving })
	value, err = s.SessionModelOptions(t.Context(), first.ID)
	if err != nil || len(value.Entries) != 0 || value.Revision != 3 {
		t.Fatal("clear failed", value, err)
	}
	open()
	if p.composer.modelPicker.defaults.temperature != "0.3" {
		t.Fatal("clear did not show model default again")
	}
	connectionClick(t, tester, "Model options scope: 模型默认")
	_, defaultScopeVisible := tester.Find("Default model options scope")
	if p.composer.modelPicker.defaults.scope != "模型默认" || !defaultScopeVisible {
		t.Fatal("default scope not explicit")
	}
	connectionType(t, tester, "Model default temperature", "0.4")
	connectionClick(t, tester, "Save model defaults")
	waitWorkspace(t, p, tester, func() bool { return !p.composer.modelPicker.saving })
	if *s.Connections.ModelPreference(s.DefaultTarget).Options.Generation.Temperature != .4 {
		t.Fatal("default scope save failed")
	}
	open()
	connectionType(t, tester, "Model default temperature", "0.9")
	p.selectSession(second.ID)
	waitWorkspace(t, p, tester, func() bool { return !p.polling && p.detail.Session.ID == second.ID })
	if p.composer.modelPicker.defaults.open {
		t.Fatal("session switch retained old scoped form")
	}
	other, err := s.SessionModelOptions(context.Background(), second.ID)
	if err != nil || other.Revision != 0 || len(other.Entries) != 0 || s.Runtime.HasActive() {
		t.Fatal("scoped editing mutated another session or executed", other, err)
	}
}

func TestWorkspaceModelDefaultsParsingIsExplicit(t *testing.T) {
	target := hub.Target{EngineID: "alva", ProviderID: "p", ModelID: "m", AccountSource: "a", BillingRoute: "b"}
	v := workspaceModelDefaultsState{target: target, draft: hub.ModelPreference{Key: hub.ModelPreferenceKey(target)}, agent: "alva", temperature: "0", tokens: "1", turns: "1", effort: "high"}
	p, err := v.preference()
	if err != nil || p.Options.Generation.Temperature == nil || *p.Options.Generation.Temperature != 0 || p.Options.MaxTurns != 1 {
		t.Fatal("explicit zero temperature lost", err)
	}
	for _, value := range []string{"NaN", "+Inf", "-0.1", "x"} {
		v.temperature = value
		if _, err = v.preference(); err == nil {
			t.Fatal("invalid temperature accepted", value)
		}
	}
}

func TestWorkspaceSessionModelOptionsNavigationObservesCommittedOverride(t *testing.T) {
	s, _ := connectionsFixture(t)
	if err := s.Runtime.RegisterDescriptor("alva", engine.Descriptor{Runtime: &settingsRunningEngine{}, Config: engine.ConfigSurface{ProviderProtocol: "native", SupportsGeneration: true}}); err != nil {
		t.Fatal(err)
	}
	pref := s.Connections.ModelPreference(s.DefaultTarget)
	pref.Options.Generation.ReasoningEffort = "high"
	if _, err := s.Connections.SaveModelPreference(t.Context(), s.DefaultTarget, pref); err != nil {
		t.Fatal(err)
	}
	session, err := s.Create(t.Context(), "observed options", "")
	if err != nil {
		t.Fatal(err)
	}
	options := hub.ModelOptions{}
	options.Generation.ReasoningEffort = "low"
	if _, err = s.SaveSessionModelOptions(t.Context(), hub.SessionModelOptionsUpdate{SessionID: session.ID, BranchID: session.ActiveBranchID, Target: s.DefaultTarget, Options: &options}); err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
	p.selectSession(session.ID)
	waitWorkspace(t, p, tester, func() bool { return !p.polling && p.detail.Session.ID == session.ID })
	if actual := p.workspaceTargetOptions(s.DefaultTarget).Generation.ReasoningEffort; actual != "low" {
		t.Fatalf("normal session navigation ignored committed override: got %q, want low", actual)
	}
	connectionClick(t, tester, "Agent and model")
	connectionClick(t, tester, "Model options: "+s.DefaultTarget.ModelID)
	if p.composer.modelPicker.inlineOptions.effort != "low" {
		t.Fatal("inline model options ignored selected session override")
	}
	for range 2 {
		p.startWorkspaceTask()
		p.selectSession(session.ID)
		waitWorkspace(t, p, tester, func() bool { return !p.polling && p.detail.Session.ID == session.ID })
		if p.workspaceTargetOptions(s.DefaultTarget).Generation.ReasoningEffort != "low" {
			t.Fatal("returning to session lost its override")
		}
	}
	other := NewNativePage(s)
	otherTester := ui.NewTester(workspaceStaticView(other), 1440, 960)
	other.selectSession(session.ID)
	waitWorkspace(t, other, otherTester, func() bool { return !other.polling && other.detail.Session.ID == session.ID })
	if other.workspaceTargetOptions(s.DefaultTarget).Generation.ReasoningEffort != "low" {
		t.Fatal("new page did not observe durable override")
	}
	p.openWorkspaceModelOptionScope(s.DefaultTarget, "当前会话")
	waitWorkspace(t, p, tester, func() bool { return !p.composer.modelPicker.saving })
	connectionClick(t, tester, "Clear session model options")
	waitWorkspace(t, p, tester, func() bool { return !p.composer.modelPicker.saving })
	connectionClick(t, tester, "Agent and model")
	connectionClick(t, tester, "Model options: "+s.DefaultTarget.ModelID)
	if p.composer.modelPicker.inlineOptions.effort != "high" || p.workspaceTargetOptions(s.DefaultTarget).Generation.ReasoningEffort != "high" {
		t.Fatal("cleared override remained in the reopened inline menu")
	}
	other.poll()
	waitWorkspace(t, other, otherTester, func() bool { return !other.polling })
	if other.workspaceTargetOptions(s.DefaultTarget).Generation.ReasoningEffort != "high" {
		t.Fatal("other window did not observe cleared override")
	}
}

func TestWorkspaceSessionModelOptionsPollUpdatesOpenMenuPreservesDrafts(t *testing.T) {
	s, _ := connectionsFixture(t)
	if err := s.Runtime.RegisterDescriptor("alva", engine.Descriptor{Runtime: &settingsRunningEngine{}, Config: engine.ConfigSurface{ProviderProtocol: "native", SupportsGeneration: true, SupportsSystemPrompt: true}}); err != nil {
		t.Fatal(err)
	}
	session, err := s.Create(t.Context(), "cross-window options", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
	p.selectSession(session.ID)
	waitWorkspace(t, p, tester, func() bool { return !p.polling && p.detail.Session.ID == session.ID })
	connectionClick(t, tester, "Agent and model")
	connectionClick(t, tester, "Model options: "+s.DefaultTarget.ModelID)
	var revision uint64
	commit := func(effort string) {
		t.Helper()
		options := hub.ModelOptions{}
		options.Generation.ReasoningEffort = effort
		value, err := s.SaveSessionModelOptions(t.Context(), hub.SessionModelOptionsUpdate{SessionID: session.ID, BranchID: session.ActiveBranchID, Target: s.DefaultTarget, ExpectedRevision: revision, Options: &options})
		if err != nil {
			t.Fatal(err)
		}
		revision = value.Revision
		p.poll()
		waitWorkspace(t, p, tester, func() bool { return !p.polling })
	}
	commit("high")
	if p.composer.modelPicker.inlineOptions.effort != "high" {
		t.Fatal("open clean menu did not observe another window's commit")
	}
	connectionClick(t, tester, "Model effort: 低")
	commit("medium")
	if p.composer.modelPicker.inlineOptions.effort != "low" || p.workspaceTargetOptions(s.DefaultTarget).Generation.ReasoningEffort != "medium" {
		t.Fatal("poll erased local effort draft or retained stale committed options")
	}
	connectionClick(t, tester, "Edit model defaults: "+s.DefaultTarget.ModelID)
	waitWorkspace(t, p, tester, func() bool { return !p.composer.modelPicker.saving })
	connectionType(t, tester, "Model system prompt", "unsaved modal prompt")
	// Advanced settings and the model form are both editing state, never poll caches.
	p.settings.SessionID, p.settings.CompatibilityFile = session.ID, "unsaved/project.json"
	commit("high")
	if p.composer.modelPicker.defaults.prompt != "unsaved modal prompt" || p.composer.modelPicker.defaults.effort != "low" || p.composer.modelPicker.defaults.overrides.Revision != 2 || p.settings.CompatibilityFile != "unsaved/project.json" {
		t.Fatal("poll overwrote an unsaved form or advanced configuration")
	}
	if p.workspaceTargetOptions(s.DefaultTarget).Generation.ReasoningEffort != "high" {
		t.Fatal("preserving form drafts hid committed cross-window options")
	}
}

type workspaceSessionOptionsObservationStore struct {
	product.ServiceStore
	fail, block atomic.Bool
	entered     chan struct{}
	release     chan struct{}
	stale       atomic.Pointer[hub.Settings]
}

func (s *workspaceSessionOptionsObservationStore) Settings(ctx context.Context, id string) (hub.Settings, error) {
	value, err := s.ServiceStore.Settings(ctx, id)
	if s.block.CompareAndSwap(true, false) {
		close(s.entered)
		select {
		case <-s.release:
		case <-ctx.Done():
			return hub.Settings{}, ctx.Err()
		}
	}
	if s.fail.Load() {
		return hub.Settings{}, errors.New("session settings observation unavailable")
	}
	if old := s.stale.Load(); old != nil {
		return *old, nil
	}
	return value, err
}

func TestWorkspaceSessionModelOptionsLatePollCannotReplaceNewSave(t *testing.T) {
	for _, fail := range []bool{false, true} {
		name := "old-snapshot"
		if fail {
			name = "old-read-failure"
		}
		t.Run(name, func(t *testing.T) {
			s, _ := connectionsFixture(t)
			if err := s.Runtime.RegisterDescriptor("alva", engine.Descriptor{Runtime: &settingsRunningEngine{}, Config: engine.ConfigSurface{ProviderProtocol: "native", SupportsGeneration: true}}); err != nil {
				t.Fatal(err)
			}
			session, err := s.Create(t.Context(), "late poll", "")
			if err != nil {
				t.Fatal(err)
			}
			store := &workspaceSessionOptionsObservationStore{ServiceStore: s.Store, entered: make(chan struct{}), release: make(chan struct{})}
			s.Store = store
			p := NewNativePage(s)
			tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
			p.selectSession(session.ID)
			waitWorkspace(t, p, tester, func() bool { return !p.polling && p.detail.Session.ID == session.ID })
			old, err := store.ServiceStore.Settings(t.Context(), session.ID)
			if err != nil {
				t.Fatal(err)
			}
			store.block.Store(true)
			p.poll()
			select {
			case <-store.entered:
			case <-time.After(time.Second):
				t.Fatal("poll did not capture the old committed snapshot")
			}
			p.openWorkspaceModelOptionScope(s.DefaultTarget, "当前会话")
			waitWorkspace(t, p, tester, func() bool { return !p.composer.modelPicker.saving })
			p.composer.modelPicker.defaults.effort = "high"
			connectionClick(t, tester, "Save model defaults")
			waitWorkspace(t, p, tester, func() bool { return !p.composer.modelPicker.saving })
			store.fail.Store(fail)
			close(store.release)
			waitWorkspace(t, p, tester, func() bool { return !p.polling })
			if p.workspaceSessionModelOptionsProblem() != "" || p.workspaceTargetOptions(s.DefaultTarget).Generation.ReasoningEffort != "high" || p.sessionModelOptions.value.Revision != 1 {
				t.Fatal("late poll replaced the newer saved fact", p.sessionModelOptions)
			}
			if !fail {
				// Even a newly started read with the current epoch cannot regress
				// the already-observed revision returned by a successful Save.
				store.stale.Store(&old)
				p.poll()
				waitWorkspace(t, p, tester, func() bool { return !p.polling })
				if p.workspaceTargetOptions(s.DefaultTarget).Generation.ReasoningEffort != "high" || p.sessionModelOptions.value.Revision != 1 {
					t.Fatal("an older revision replaced the new Save")
				}
			}
		})
	}
}

func TestWorkspaceSessionModelOptionsObservationFailureBlocksAndRetries(t *testing.T) {
	s, _ := connectionsFixture(t)
	session, err := s.Create(t.Context(), "read retry", "")
	if err != nil {
		t.Fatal(err)
	}
	store := &workspaceSessionOptionsObservationStore{ServiceStore: s.Store}
	s.Store = store
	p := NewNativePage(s)
	tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
	p.selectSession(session.ID)
	if p.workspaceTargetProblem() == "" {
		t.Fatal("unread session options allowed sending")
	}
	waitWorkspace(t, p, tester, func() bool { return !p.polling && p.detail.Session.ID == session.ID })
	connectionClick(t, tester, "Agent and model")
	connectionClick(t, tester, "Model options: "+s.DefaultTarget.ModelID)
	store.fail.Store(true)
	p.poll()
	waitWorkspace(t, p, tester, func() bool { return !p.polling })
	p.model.Draft = "preserve this prompt"
	p.sendWorkspaceDraft()
	if p.workspaceTargetProblem() == "" || p.busy || s.Runtime.HasActive() || p.model.Draft != "preserve this prompt" {
		t.Fatal("failed observation silently used model defaults")
	}
	if _, ok := tester.Find("Reload observed session model options"); !ok {
		t.Fatal("failed observation has no retry control")
	}
	store.fail.Store(false)
	connectionClick(t, tester, "Reload observed session model options")
	waitWorkspace(t, p, tester, func() bool { return !p.polling })
	if p.workspaceTargetProblem() != "" {
		t.Fatal("successful retry did not restore committed session options", p.workspaceTargetProblem())
	}
}

func TestWorkspaceSessionModelOptionsInitialReadFailureCanReloadLatest(t *testing.T) {
	s, _ := connectionsFixture(t)
	session, err := s.Create(t.Context(), "initial read retry", "")
	if err != nil {
		t.Fatal(err)
	}
	store := &workspaceSessionOptionsObservationStore{ServiceStore: s.Store}
	store.fail.Store(true)
	s.Store = store
	p := NewNativePage(s)
	tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
	p.drafts[session.ID] = "retained during failed initial read"
	p.selectSession(session.ID)
	waitWorkspace(t, p, tester, func() bool { return !p.polling })
	p.sendSnapshot(p.model.Draft)
	if p.workspaceTargetProblem() == "" || p.busy || s.Runtime.HasActive() {
		t.Fatal("failed initial Settings read enabled submission")
	}
	store.fail.Store(false)
	if err := workspaceTestMenu(tester, "最新消息"); err != nil {
		t.Fatal("initial read failure has no actual reload path", err)
	}
	waitWorkspace(t, p, tester, func() bool { return !p.polling && p.detail.Session.ID == session.ID })
	if p.workspaceTargetProblem() != "" || p.model.Draft != "retained during failed initial read" {
		t.Fatal("actual Latest retry did not restore committed options and preserve the draft")
	}
}

func TestWorkspaceSessionModelOptionsEmptyExternalOverrideCanSelectAndSend(t *testing.T) {
	s, _ := connectionsFixture(t)
	requests := make(chan engine.Request, 1)
	if err := s.Runtime.Register("pi", workspaceVoiceSendEngine{requests: requests}); err != nil {
		t.Fatal(err)
	}
	external := s.DefaultTarget
	external.EngineID = "pi"
	s.Targets = append(s.Targets, external)
	pref := s.Connections.ModelPreference(s.DefaultTarget)
	pref.Options.Generation.ReasoningEffort = "high"
	if _, err := s.Connections.SaveModelPreference(t.Context(), s.DefaultTarget, pref); err != nil {
		t.Fatal(err)
	}
	session, err := s.Create(t.Context(), "empty explicit override", "")
	if err != nil {
		t.Fatal(err)
	}
	empty := hub.ModelOptions{}
	if _, err = s.SaveSessionModelOptions(t.Context(), hub.SessionModelOptionsUpdate{SessionID: session.ID, BranchID: session.ActiveBranchID, Target: external, Options: &empty}); err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
	p.selectSession(session.ID)
	waitWorkspace(t, p, tester, func() bool { return !p.polling && p.detail.Session.ID == session.ID })
	connectionClick(t, tester, "Agent and model")
	index := -1
	for i := range p.composer.modelPicker.rows {
		row := &p.composer.modelPicker.rows[i]
		for j, route := range row.routes {
			if hub.RouteKey(route) == hub.RouteKey(external) {
				row.chosen, index = j, i
			}
		}
	}
	if index < 0 {
		t.Fatal("external fixture route missing")
	}
	p.chooseWorkspaceModel(index)
	waitWorkspace(t, p, tester, func() bool { return !p.busy && !p.polling && p.workspaceComposerTarget().EngineID == "pi" })
	if p.workspaceTargetProblem() != "" || !p.workspaceTargetOptions(external).Generation.Empty() {
		t.Fatal("explicit empty override inherited incompatible generation defaults", p.workspaceTargetProblem())
	}
	p.model.Draft = "local external-engine fixture only"
	connectionClick(t, tester, "Send")
	waitWorkspace(t, p, tester, func() bool { return !p.busy && !p.polling && !s.Runtime.HasActive() && p.model.Draft == "" })
	select {
	case request := <-requests:
		if !request.Generation.Empty() {
			t.Fatal("empty session override was not used for admitted Attempt")
		}
	default:
		t.Fatal("explicit Send did not reach the local fixture")
	}
}

type workspaceSessionOptionsReadFailure struct {
	product.ServiceStore
	writes atomic.Int32
}

func (s *workspaceSessionOptionsReadFailure) SessionModelOptions(context.Context, string) (hub.SessionModelOptionsSnapshot, error) {
	return hub.SessionModelOptionsSnapshot{}, errors.New("session override read unavailable")
}

func (s *workspaceSessionOptionsReadFailure) SaveSessionModelOptions(ctx context.Context, edit hub.SessionModelOptionsUpdate) (hub.SessionModelOptionsSnapshot, error) {
	s.writes.Add(1)
	return s.ServiceStore.SaveSessionModelOptions(ctx, edit)
}

func TestWorkspaceSessionModelOptionsReadFailureNeverFallsBackToSavingDefaults(t *testing.T) {
	s, _ := connectionsFixture(t)
	if err := s.Runtime.RegisterDescriptor("alva", engine.Descriptor{Runtime: &settingsRunningEngine{}, Config: engine.ConfigSurface{ProviderProtocol: "native", SupportsGeneration: true}}); err != nil {
		t.Fatal(err)
	}
	session, err := s.Create(t.Context(), "unavailable options", "")
	if err != nil {
		t.Fatal(err)
	}
	options := hub.ModelOptions{SystemPrompt: "must survive a failed read"}
	if _, err = s.Store.SaveSessionModelOptions(t.Context(), hub.SessionModelOptionsUpdate{SessionID: session.ID, BranchID: session.ActiveBranchID, Target: s.DefaultTarget, Options: &options}); err != nil {
		t.Fatal(err)
	}
	backend := &workspaceSessionOptionsReadFailure{ServiceStore: s.Store}
	s.Store = backend
	p := NewNativePage(s)
	p.selected = session.ID
	p.detail, err = s.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.openWorkspaceModelOptionScope(s.DefaultTarget, "当前会话")
	waitWorkspace(t, p, tester, func() bool { return !p.composer.modelPicker.saving })
	if p.composer.modelPicker.defaults.ready || p.composer.modelPicker.defaults.problem == "" {
		t.Fatal("read failure presented defaults as a ready session snapshot")
	}
	connectionClick(t, tester, "Save model defaults")
	connectionClick(t, tester, "Clear session model options")
	p.saveWorkspaceSessionModelOptions(false)
	if backend.writes.Load() != 0 || p.composer.modelPicker.saving {
		t.Fatal("read failure wrote a default fallback")
	}
	actual, err := backend.ServiceStore.SessionModelOptions(t.Context(), session.ID)
	retained, found := actual.Options(s.DefaultTarget)
	if err != nil || !found || retained.SystemPrompt != options.SystemPrompt || actual.Revision != 1 {
		t.Fatal("read failure lost committed override", actual, err)
	}
}

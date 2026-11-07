// INPUT: Actual shared composer admission, immutable captured fixtures and a file-capable request-recording runtime.
// OUTPUT: Typed frozen engine inputs, original user-history text and unsupported-engine zero-execution regression.
// POS: [L7 Hub product acceptance] Exercises one execution path without a model, account or host document execution.
package product

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	productstore "github.com/QuincySx/Alva/apps/hub/sqlite"
	"github.com/QuincySx/Alva/integration/engine"
)

type composerFileRecordingRuntime struct{ calls chan engine.Request }

func (r composerFileRecordingRuntime) Capabilities() engine.Capabilities {
	return engine.Capabilities{FileInputs: true}
}
func (r composerFileRecordingRuntime) Execute(_ context.Context, q engine.Request, emit func(engine.Event) error) error {
	r.calls <- q
	if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "captured-file-native"}); err != nil {
		return err
	}
	return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
}
func (r composerFileRecordingRuntime) Cancel(context.Context, string) error { return nil }
func (r composerFileRecordingRuntime) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return engine.ErrUnsupported
}
func createComposerFileFixtureSession(t *testing.T, s *HubService, f hub.ComposerFile, title string) hub.Session {
	t.Helper()
	drafts := s.Store.(hub.ComposerDraftStore)
	current, err := drafts.ComposerDraft(t.Context(), f.Scope)
	if err != nil {
		t.Fatal(err)
	}
	draft, err := drafts.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: f.Scope, Text: title, References: []hub.ComposerDraftReference{SelectedComposerFile(f).DraftSelection()}}, current.Revision)
	if err != nil {
		t.Fatal(err)
	}
	workspace, err := s.prepareSessionWorkspace("")
	if err != nil {
		t.Fatal(err)
	}
	session, _, err := s.Store.(hub.ComposerDraftAdmissionStore).CreateComposerSession(t.Context(), draft, title, workspace, s.DefaultTarget)
	if err != nil {
		t.Fatal(err)
	}
	return session
}

func TestComposerFileSubmissionFreezesTypedFilesAndPreservesManualHistory(t *testing.T) {
	s, _ := fixture(t)
	calls := make(chan engine.Request, 1)
	if err := s.Runtime.Register("composer-file-fixture", composerFileRecordingRuntime{calls: calls}); err != nil {
		t.Fatal(err)
	}
	target := s.DefaultTarget
	target.EngineID = "composer-file-fixture"
	s.Targets = append(s.Targets, target)
	f, err := s.CaptureComposerFile(t.Context(), hub.ComposerDraftScope{Kind: "home", ID: "new-conversation"}, composerFixtureFile(t, "notes.txt", []byte("short selected file body")))
	if err != nil {
		t.Fatal(err)
	}
	session := createComposerFileFixtureSession(t, s, f, "file submission")
	if _, err = s.Store.Switch(t.Context(), session.ID, target, "fixture file-capable engine"); err != nil {
		t.Fatal(err)
	}
	prompt := "Only this manual text belongs in the transcript"
	if _, err = s.SubmitComposerFileDraft(t.Context(), session.ID, prompt, nil, nil, []ComposerFileSelection{SelectedComposerFile(f)}); err != nil {
		t.Fatal(err)
	}
	request := <-calls
	waitIdle(t, s, session.ID)
	if len(request.InputFiles) != 1 || request.InputFiles[0].SHA256 != f.SHA256 || request.InputFiles[0].TextPath != request.InputFiles[0].Path || !strings.Contains(request.Prompt, "short selected file body") {
		t.Fatal("captured file identity or bounded prompt was not delivered", request)
	}
	history, err := s.Store.History(t.Context(), session.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	found := false
	for _, entry := range history {
		if entry.Input != "" {
			found = true
			if entry.Input != prompt || strings.Contains(entry.Input, "composer://") || len(entry.InputFiles) != 1 || entry.InputFiles[0].SHA256 != f.SHA256 {
				t.Fatal("internal model manifest replaced manual transcript text", entry)
			}
		}
	}
	if !found {
		t.Fatal("manual input history missing")
	}
}
func TestComposerFileSubmissionRejectsUnsupportedEngineBeforeExecution(t *testing.T) {
	s, requests := fixture(t)
	f, err := s.CaptureComposerFile(t.Context(), hub.ComposerDraftScope{Kind: "home", ID: "new-conversation"}, composerFixtureFile(t, "notes.txt", []byte("source")))
	if err != nil {
		t.Fatal(err)
	}
	session := createComposerFileFixtureSession(t, s, f, "unsupported file input")
	if _, err = s.SubmitComposerFileDraft(t.Context(), session.ID, "keep draft", nil, nil, []ComposerFileSelection{SelectedComposerFile(f)}); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal("undeclared file delivery reached an engine", err)
	}
	if len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("unsupported input capability executed")
	}
}

func TestComposerFileOwnerRejectsForeignSessionAndForgedHomeAssociation(t *testing.T) {
	s, _ := fixture(t)
	calls := make(chan engine.Request, 2)
	if err := s.Runtime.Register("file-owner-fixture", composerFileRecordingRuntime{calls: calls}); err != nil {
		t.Fatal(err)
	}
	target := s.DefaultTarget
	target.EngineID = "file-owner-fixture"
	s.Targets = append(s.Targets, target)
	a, err := s.Create(t.Context(), "original owner", "")
	if err != nil {
		t.Fatal(err)
	}
	b, err := s.Create(t.Context(), "foreign owner", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Store.Switch(t.Context(), b.ID, target, "file owner test"); err != nil {
		t.Fatal(err)
	}
	file, err := s.CaptureComposerFile(t.Context(), hub.ComposerDraftScope{Kind: "session", ID: a.ID}, composerFixtureFile(t, "private.txt", []byte("session A captured original")))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.SubmitComposerFileDraft(t.Context(), b.ID, "foreign", nil, nil, []ComposerFileSelection{SelectedComposerFile(file)}); !errors.Is(err, hub.ErrComposerFileChanged) {
		t.Fatal("foreign session borrowed a captured original", err)
	}
	drafts := s.Store.(hub.ComposerDraftStore)
	if _, err = drafts.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: hub.ComposerDraftScope{Kind: "session", ID: b.ID}, References: []hub.ComposerDraftReference{SelectedComposerFile(file).DraftSelection()}}, 0); !errors.Is(err, hub.ErrComposerFileChanged) {
		t.Fatal("session draft DTO manufactured foreign file ownership", err)
	}
	home, err := s.CaptureComposerFile(t.Context(), hub.ComposerDraftScope{Kind: "home", ID: "owner-home"}, composerFixtureFile(t, "home.txt", []byte("home original")))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = drafts.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: hub.ComposerDraftScope{Kind: "session", ID: b.ID}, References: []hub.ComposerDraftReference{SelectedComposerFile(home).DraftSelection()}}, 0); !errors.Is(err, hub.ErrComposerFileChanged) {
		t.Fatal("arbitrary session reference minted home adoption", err)
	}
	if _, err = s.SubmitComposerFileDraft(t.Context(), b.ID, "unadopted home", nil, nil, []ComposerFileSelection{SelectedComposerFile(home)}); !errors.Is(err, hub.ErrComposerFileChanged) {
		t.Fatal("unbound home original borrowed arbitrary session", err)
	}
	adopted := createComposerFileFixtureSession(t, s, home, "actual atomic adoption")
	if err = s.ValidateComposerFileOwner(t.Context(), adopted.ID, home); err != nil {
		t.Fatal("actual first-Send grant unavailable", err)
	}
	if err = s.ValidateComposerFileOwner(t.Context(), b.ID, home); !errors.Is(err, hub.ErrComposerFileChanged) {
		t.Fatal("home grant broadened to another session", err)
	}
	current, err := drafts.ComposerDraft(t.Context(), home.Scope)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = drafts.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: home.Scope}, current.Revision); err != nil {
		t.Fatal(err)
	}
	if err = s.ValidateComposerFileOwner(t.Context(), adopted.ID, home); err != nil {
		t.Fatal("consuming home draft erased actual immutable ownership", err)
	}
	if len(calls) != 0 || s.Runtime.HasActive() {
		t.Fatal("rejected ownership executed a model runtime")
	}
	store := s.Store.(*productstore.Store)
	if err = store.Close(); err != nil {
		t.Fatal(err)
	}
	reopened, err := productstore.Open(t.Context(), filepath.Join(s.DataDirectory, "product"))
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.Close()
	cold := &HubService{Store: reopened, DataDirectory: s.DataDirectory}
	if err = cold.ValidateComposerFileOwner(t.Context(), adopted.ID, home); err != nil {
		t.Fatal("restart lost immutable adoption grant", err)
	}
	if err = cold.ValidateComposerFileOwner(t.Context(), b.ID, home); !errors.Is(err, hub.ErrComposerFileChanged) {
		t.Fatal("restart broadened immutable grant", err)
	}
}

// INPUT: Real SQLite product/native stores behind opaque contracts and confined project fixtures
// OUTPUT: Bounded cancellable metadata search, immutable selected references and branch/permission boundary regressions
// POS: [L7 Hub product] Headless source selection tests never use a window, external account or alternate storage authority
package product

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/platform/storage"
)

type opaqueReferenceRepository struct{ storage.Repository }
type opaqueReferenceStore struct{ ServiceStore }

func referenceProject(t *testing.T) (*HubService, hub.Session) {
	t.Helper()
	s, _ := fixture(t)
	s.Repository = opaqueReferenceRepository{s.Repository}
	s.Store = opaqueReferenceStore{s.Store}
	project, err := s.Create(t.Context(), "reference project", "")
	if err != nil {
		t.Fatal(err)
	}
	return s, project
}

func referenceFile(t *testing.T, project hub.Session, path, text string) {
	t.Helper()
	name := filepath.Join(project.Workspace, path)
	if err := os.MkdirAll(filepath.Dir(name), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(name, []byte(text), 0600); err != nil {
		t.Fatal(err)
	}
}

func selectReference(t *testing.T, s *HubService, project hub.Session, path string) WorkspaceReferenceSelection {
	t.Helper()
	page, err := s.SearchWorkspaceReferences(t.Context(), project.ID, path, "", maxWorkspaceReferencePageSize)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range page.Entries {
		if entry.Path == path {
			return entry
		}
	}
	t.Fatalf("missing project reference %q in %+v", path, page)
	return WorkspaceReferenceSelection{}
}

func TestWorkspaceReferenceSearchPagesStableIdentityAndCancellation(t *testing.T) {
	s, project := referenceProject(t)
	referenceFile(t, project, "src/a.go", "first")
	referenceFile(t, project, "src/b.go", "second")
	referenceFile(t, project, "unrelated.txt", "body")
	page, err := s.SearchWorkspaceReferences(t.Context(), project.ID, "src", "", 2)
	if err != nil || len(page.Entries) != 2 || !page.HasMore || page.Next == "" || page.Truncated {
		t.Fatal("missing bounded first page", page, err)
	}
	next, err := s.SearchWorkspaceReferences(t.Context(), project.ID, "src", page.Next, 2)
	if err != nil || len(next.Entries) != 1 || next.HasMore || next.Entries[0].Path != "src/b.go" {
		t.Fatal("metadata cursor lost or repeated a path", next, err)
	}
	if _, err = s.SearchWorkspaceReferences(t.Context(), project.ID, "other query", page.Next, 2); !errors.Is(err, ErrWorkspaceReferenceChanged) {
		t.Fatal("cursor reused across a different query", err)
	}
	before := selectReference(t, s, project, "src/a.go")
	referenceFile(t, project, "src/a.go", "new version with different bytes")
	after := selectReference(t, s, project, "src/a.go")
	if before.ID != after.ID || before.Revision == after.Revision {
		t.Fatal("identity followed content version rather than the source path")
	}
	if _, err = s.SearchWorkspaceReferences(t.Context(), project.ID, "src", page.Next, 2); !errors.Is(err, ErrWorkspaceReferenceChanged) {
		t.Fatal("changed index silently continued an old page", err)
	}
	if _, err = s.PrepareWorkspaceReference(t.Context(), project.ID, before); !errors.Is(err, ErrWorkspaceReferenceChanged) {
		t.Fatal("stale selected metadata was retained as current", err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err = s.SearchWorkspaceReferences(ctx, project.ID, "", "", 1); !errors.Is(err, context.Canceled) {
		t.Fatal("cancelled query succeeded", err)
	}
	detail, err := s.Store.Detail(t.Context(), project.ID)
	branch, branchErr := ActiveBranch(detail)
	if err != nil || branchErr != nil || branch.NativeSessionID != "" || s.Runtime.HasActive() {
		t.Fatal("metadata search created native storage or execution", detail, err, branchErr)
	}
}

func TestWorkspaceReferenceFrozenOriginalSubmitsOnlyActualReference(t *testing.T) {
	s, requests := fixture(t)
	s.Repository = opaqueReferenceRepository{s.Repository}
	s.Store = opaqueReferenceStore{s.Store}
	project, err := s.Create(t.Context(), "actual references", "")
	if err != nil {
		t.Fatal(err)
	}
	const original = "PROJECT_PRIVATE_ORIGINAL_NEVER_INLINE_IN_THE_MODEL_PROMPT"
	referenceFile(t, project, "notes/input.txt", original)
	selected := selectReference(t, s, project, "notes/input.txt")
	draft, err := s.PrepareWorkspaceReference(t.Context(), project.ID, selected)
	if err != nil || draft.Info.SHA256 == "" || draft.Reference.URI == "" || len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("prepare lacked a real retained artifact or started execution", draft, err)
	}
	repeated, err := s.PrepareWorkspaceReference(t.Context(), project.ID, selected)
	if err != nil || repeated.DraftID == draft.DraftID || repeated.Info != draft.Info || len(s.DraftWorkspaceReferences(project.ID)) != 2 {
		t.Fatal("same retained original did not receive independent observer draft identities", repeated, err)
	}
	if _, _, err = s.Repository.ReadArtifactFull(t.Context(), "foreign owner", draft.Info.SessionID, draft.Reference.URI, 4096); !errors.Is(err, storage.ErrNotFound) {
		t.Fatal("reference URI granted another owner the original", err)
	}
	referenceFile(t, project, "notes/input.txt", "edited after prepare")
	if _, err = s.Submit(t.Context(), project.ID, "inspect the selected original", []DraftReferenceSelection{draft.DraftSelection()}); err != nil {
		t.Fatal(err)
	}
	request := <-requests
	if strings.Contains(request.Prompt, original) || strings.Contains(request.Prompt, "edited after prepare") || strings.Contains(request.Prompt, project.Workspace) || !strings.Contains(request.Prompt, draft.Reference.URI) || !strings.Contains(request.Prompt, selected.ID) || request.ResumeSession != draft.Info.SessionID {
		t.Fatal("submit lost immutable selection or inlined original/path", request)
	}
	waitIdle(t, s, project.ID)
	_, bytes, err := s.Repository.ReadArtifactFull(t.Context(), project.ID, draft.Info.SessionID, draft.Reference.URI, 4096)
	if err != nil || string(bytes) != original || len(s.DraftWorkspaceReferences(project.ID)) != 1 {
		t.Fatal("retained original changed or admitted draft was not consumed", string(bytes), err)
	}
}

func TestWorkspaceDirectoryReferenceRetainsOnlyBoundedMetadataAndRemovalPreservesSource(t *testing.T) {
	s, project := referenceProject(t)
	const body = "FILE_BODY_MUST_NOT_APPEAR_IN_DIRECTORY_METADATA"
	referenceFile(t, project, "docs/readme.txt", body)
	referenceFile(t, project, "docs/.env", "sensitive value")
	outside := t.TempDir()
	if err := os.WriteFile(filepath.Join(outside, "outside.txt"), []byte("outside original"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(project.Workspace, "docs/escape")); err != nil {
		t.Fatal(err)
	}
	settingsBefore, err := s.Store.Settings(t.Context(), project.ID)
	if err != nil {
		t.Fatal(err)
	}
	draft, err := s.PrepareWorkspaceReference(t.Context(), project.ID, selectReference(t, s, project, "docs"))
	if err != nil {
		t.Fatal(err)
	}
	_, bytes, err := s.Repository.ReadArtifactFull(t.Context(), project.ID, draft.Info.SessionID, draft.Reference.URI, maxWorkspaceDirectoryBytes)
	var manifest WorkspaceReferencePage
	if err != nil || json.Unmarshal(bytes, &manifest) != nil || len(manifest.Entries) != 2 || strings.Contains(string(bytes), body) || strings.Contains(string(bytes), ".env") || strings.Contains(string(bytes), "escape") || !strings.Contains(draft.Info.Processing, "not an atomic or complete directory snapshot") {
		t.Fatal("directory manifest overclaimed or included bodies/forbidden paths", string(bytes), err)
	}
	settingsAfter, err := s.Store.Settings(t.Context(), project.ID)
	before, _ := json.Marshal(settingsBefore)
	after, _ := json.Marshal(settingsAfter)
	if err != nil || string(before) != string(after) {
		t.Fatal("directory selection changed next-turn grants or settings", err)
	}
	s.RemoveDraftWorkspaceReference(project.ID, draft.DraftID)
	_, bytes, err = s.Repository.ReadArtifactFull(t.Context(), project.ID, draft.Info.SessionID, draft.Reference.URI, maxWorkspaceDirectoryBytes)
	actual, readErr := os.ReadFile(filepath.Join(project.Workspace, "docs/readme.txt"))
	if err != nil || readErr != nil || len(bytes) == 0 || string(actual) != body || len(s.DraftWorkspaceReferences(project.ID)) != 0 {
		t.Fatal("removing a draft deleted source or retained metadata", err, readErr)
	}
}

func TestWorkspaceReferenceRejectsTraversalSymlinksForeignScopesAndParkedBranches(t *testing.T) {
	s, project := referenceProject(t)
	referenceFile(t, project, "safe.txt", "original")
	selected := selectReference(t, s, project, "safe.txt")
	for _, path := range []string{"../outside", "safe/../safe.txt", "/absolute", "nested\\path"} {
		forged := selected
		forged.Path = path
		if _, err := s.PrepareWorkspaceReference(t.Context(), project.ID, forged); err == nil {
			t.Fatalf("accepted noncanonical path %q", path)
		}
	}
	foreign, err := s.Create(t.Context(), "foreign product", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.PrepareWorkspaceReference(t.Context(), foreign.ID, selected); !errors.Is(err, ErrWorkspaceReferenceChanged) {
		t.Fatal("foreign selection was rebound to a different product", err)
	}
	if err := os.Rename(filepath.Join(project.Workspace, "safe.txt"), filepath.Join(project.Workspace, "original.txt")); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(filepath.Join(project.Workspace, "original.txt"), filepath.Join(project.Workspace, "safe.txt")); err != nil {
		t.Fatal(err)
	}
	if _, err := s.PrepareWorkspaceReference(t.Context(), project.ID, selected); err == nil {
		t.Fatal("a selected file replaced by a symlink was accepted")
	}
	selected = selectReference(t, s, project, "original.txt")
	if _, err = s.PrepareWorkspaceReference(t.Context(), project.ID, selected); err != nil {
		t.Fatal(err)
	}
	target := s.DefaultTarget
	target.ModelID = "park original references"
	if _, err = s.Store.Switch(t.Context(), project.ID, target, "explicit route change"); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Submit(t.Context(), project.ID, "must keep draft", []DraftReferenceSelection{s.DraftWorkspaceReferences(project.ID)[0].DraftSelection()}); err == nil || len(s.DraftWorkspaceReferences(project.ID)) != 1 || s.Runtime.HasActive() {
		t.Fatal("parked-branch reference crossed admission or was discarded", err)
	}
}

func TestWorkspaceReferenceIndexAndDirectoryManifestStayBounded(t *testing.T) {
	s, project := referenceProject(t)
	for i := 0; i < maxWorkspaceIndexNodes+10; i++ {
		referenceFile(t, project, fmt.Sprintf("wide/item-%04d.txt", i), "metadata only")
	}
	page, err := s.SearchWorkspaceReferences(t.Context(), project.ID, "wide", "", 1)
	if err != nil || !page.Truncated || page.Scanned > maxWorkspaceIndexNodes || len(page.Entries) != 1 {
		t.Fatal("wide directory exceeded index budget or hid truncation", page, err)
	}
	draft, err := s.PrepareWorkspaceReference(t.Context(), project.ID, page.Entries[0])
	if err != nil {
		t.Fatal(err)
	}
	_, data, err := s.Repository.ReadArtifactFull(t.Context(), project.ID, draft.Info.SessionID, draft.Reference.URI, maxWorkspaceDirectoryBytes)
	var manifest WorkspaceReferencePage
	if err != nil || json.Unmarshal(data, &manifest) != nil || !manifest.Truncated || manifest.Scanned > maxWorkspaceDirectoryNodes || len(data) > maxWorkspaceDirectoryBytes {
		t.Fatal("directory reference exceeded metadata bounds", len(data), manifest.Scanned, err)
	}
}

func TestWorkspaceReferencesShareUploadBudgetAndFailedArtifactWriteHasNoFallback(t *testing.T) {
	s, project := referenceProject(t)
	referenceFile(t, project, "selected.txt", "small")
	selected := selectReference(t, s, project, "selected.txt")
	if _, err := s.PrepareWorkspaceReference(t.Context(), project.ID, selected); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < maxDraftReferenceCount-1; i++ {
		if _, err := s.UploadReader(t.Context(), project.ID, fmt.Sprintf("upload-%d.txt", i), strings.NewReader("small")); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := s.UploadReader(t.Context(), project.ID, "overflow.txt", strings.NewReader("small")); err == nil {
		t.Fatal("upload ignored already prepared project references")
	}
	s.RemoveDraftWorkspaceReference(project.ID, s.DraftWorkspaceReferences(project.ID)[0].DraftID)
	if _, err := s.UploadReader(t.Context(), project.ID, "last.txt", strings.NewReader("small")); err != nil {
		t.Fatal("removing a draft did not release the draft count budget", err)
	}
	if _, err := s.PrepareWorkspaceReference(t.Context(), project.ID, selected); err == nil {
		t.Fatal("project reference ignored existing uploaded files")
	}
	for _, attachment := range s.DraftAttachments(project.ID) {
		s.RemoveDraftAttachment(project.ID, attachment.DraftID)
	}
	failure := errors.New("explicit artifact provider is offline")
	s.Repository = failingReferenceWriter{Repository: s.Repository, problem: failure}
	if _, err := s.PrepareWorkspaceReference(t.Context(), project.ID, selected); !errors.Is(err, failure) || len(s.DraftWorkspaceReferences(project.ID)) != 0 {
		t.Fatal("failed provider fell back or reported a prepared draft", err)
	}
}

func TestWorkspaceReferencesAndUploadsShareFortyMiBByteBudget(t *testing.T) {
	s, project := referenceProject(t)
	file, err := os.Create(filepath.Join(project.Workspace, "twenty-mib.bin"))
	if err != nil {
		t.Fatal(err)
	}
	if err = file.Truncate(storage.MaxArtifactBytes); err != nil {
		file.Close()
		t.Fatal(err)
	}
	if err = file.Close(); err != nil {
		t.Fatal(err)
	}
	selected := selectReference(t, s, project, "twenty-mib.bin")
	draft, err := s.PrepareWorkspaceReference(t.Context(), project.ID, selected)
	if err != nil || draft.Info.Bytes != storage.MaxArtifactBytes {
		t.Fatal("real 20 MiB source was not prepared", draft.Info, err)
	}
	if _, err = s.UploadReader(t.Context(), project.ID, "twenty-mib-upload.bin", strings.NewReader(strings.Repeat("x", storage.MaxArtifactBytes))); err != nil {
		t.Fatal("exact 40 MiB combined budget was rejected", err)
	}
	if _, err = s.UploadReader(t.Context(), project.ID, "one-byte-upload.txt", strings.NewReader("x")); err == nil {
		t.Fatal("upload exceeded combined project-reference byte budget")
	}
	referenceFile(t, project, "one-byte.txt", "x")
	if _, err = s.PrepareWorkspaceReference(t.Context(), project.ID, selectReference(t, s, project, "one-byte.txt")); err == nil {
		t.Fatal("project reference exceeded combined uploaded-file byte budget")
	}
	if len(s.DraftWorkspaceReferences(project.ID)) != 1 || len(s.DraftAttachments(project.ID)) != 1 || s.Runtime.HasActive() {
		t.Fatal("byte-budget rejection changed the retained draft or started execution")
	}
}

func TestWorkspaceReferenceSubmitRejectsWorkspaceAndConcurrentBranchChange(t *testing.T) {
	s, project := referenceProject(t)
	referenceFile(t, project, "selected.txt", "retain the actual original")
	selected := selectReference(t, s, project, "selected.txt")
	if _, err := s.PrepareWorkspaceReference(t.Context(), project.ID, selected); err != nil {
		t.Fatal(err)
	}
	detail, err := s.Store.Detail(t.Context(), project.ID)
	if err != nil {
		t.Fatal(err)
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	s.mu.Lock()
	_, err = s.workspaceReferencePromptLocked(project.ID, branch, t.TempDir(), "changed workspace", s.DraftWorkspaceReferences(project.ID))
	s.mu.Unlock()
	if err == nil {
		t.Fatal("source scope was silently rebound to another workspace")
	}
	target := s.DefaultTarget
	target.ModelID = "route changed during claim"
	wrapped := switchBeforeAdmission{ServiceStore: s.Store, target: target}
	s.Store = wrapped
	s.Runtime = hub.New(wrapped)
	t.Cleanup(func() { _ = s.Runtime.Close(context.Background()) })
	if _, err = s.Submit(t.Context(), project.ID, "must not reach another branch", []DraftReferenceSelection{s.DraftWorkspaceReferences(project.ID)[0].DraftSelection()}); err == nil || len(s.DraftWorkspaceReferences(project.ID)) != 1 {
		t.Fatal("stale selected scope reached another branch or lost the draft", err)
	}
	history, err := s.Store.History(t.Context(), project.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range history {
		if entry.Input != "" {
			t.Fatal("route race committed a selected manifest on the wrong branch")
		}
	}
}

func TestWorkspaceReferenceWorkerQueueConsumesOnlyItsOwnPinnedDraft(t *testing.T) {
	s, requests := fixture(t)
	lead, err := s.Create(t.Context(), "Lead with project references", "")
	if err != nil {
		t.Fatal(err)
	}
	team, err := s.Teams.Store.CreateTeam(t.Context(), lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	worker, err := s.AddWorker(t.Context(), lead.ID, team.Team.ID, "project reference Worker", s.DefaultTarget, true)
	if err != nil {
		t.Fatal(err)
	}
	detail, err := s.Store.Detail(t.Context(), worker.SessionID)
	if err != nil {
		t.Fatal(err)
	}
	const original = "WORKER_ORIGINAL_MUST_NOT_BE_COPIED_INTO_QUEUE_SCOPE"
	referenceFile(t, detail.Session, "worker.txt", original)
	draft, err := s.PrepareWorkspaceReference(t.Context(), worker.SessionID, selectReference(t, s, detail.Session, "worker.txt"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.EnqueueWorkerDraft(t.Context(), lead.ID, team.Team.ID, worker.ID, strings.Repeat("x", 16<<10), nil, []DraftReferenceSelection{s.DraftWorkspaceReferences(worker.SessionID)[0].DraftSelection()}); err == nil || len(s.DraftWorkspaceReferences(worker.SessionID)) != 1 {
		t.Fatal("rejected queue admission consumed the reference draft", err)
	}
	task, err := s.EnqueueWorkerDraft(t.Context(), lead.ID, team.Team.ID, worker.ID, "inspect selected source ranges", nil, []DraftReferenceSelection{s.DraftWorkspaceReferences(worker.SessionID)[0].DraftSelection()})
	if err != nil || strings.Contains(task.Scope, original) || !strings.Contains(task.Scope, draft.Reference.URI) || task.RequiredBranchID != draft.Selection.BranchID || len(s.DraftWorkspaceReferences(worker.SessionID)) != 0 || len(requests) != 0 {
		t.Fatal("queue did not freeze the exact Worker reference or executed it", task, err)
	}
	if _, _, err = s.Repository.ReadArtifactFull(t.Context(), lead.ID, draft.Info.SessionID, draft.Reference.URI, 4096); !errors.Is(err, storage.ErrNotFound) {
		t.Fatal("Worker reference transferred original access to Lead", err)
	}
}

type failingReferenceWriter struct {
	storage.Repository
	problem error
}

func (r failingReferenceWriter) PutArtifact(context.Context, string, string, storage.ArtifactInput) (storage.ArtifactInfo, error) {
	return storage.ArtifactInfo{}, r.problem
}

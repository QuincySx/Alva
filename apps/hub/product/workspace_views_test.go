// INPUT: Explicit view-store fakes, real product/native SQLite and confined source fixtures.
// OUTPUT: Bounded resource reads/metadata index, root-source retention/partial pagination and actual source/branch/cancellation isolation.
// POS: [L7 product tests] Observation never creates an execution, replays a tool or grants another owner evidence.
package product

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
)

func resourceService(t *testing.T) (*HubService, hub.Session) {
	t.Helper()
	s, _ := fixture(t)
	port, ok := s.Store.(hub.WorkspaceViewStore)
	if !ok {
		t.Fatal("fixture did not supply workspace view capability")
	}
	var err error
	s.WorkspaceViews, err = NewWorkspaceViewsService(port)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = s.WorkspaceViews.Close(context.Background()) })
	p, err := s.Create(t.Context(), "Resources", "")
	if err != nil {
		t.Fatal(err)
	}
	return s, p
}

func resourceFileTab(t *testing.T, s *HubService, p hub.Session, path string) hub.WorkspaceTab {
	t.Helper()
	f := selectReference(t, s, p, path)
	return hub.WorkspaceTab{ID: "file-tab", Kind: "file", Title: f.Name, BranchID: f.BranchID, File: &hub.WorkspaceFileSource{ID: f.ID, BranchID: f.BranchID, WorkspaceID: f.WorkspaceID, Path: f.Path, Name: f.Name, Kind: f.Kind, Revision: f.Revision, Bytes: f.Bytes}}
}

func TestWorkspaceResourcesActualBoundedFileReadAndChangedSourceRejection(t *testing.T) {
	s, p := resourceService(t)
	content := strings.Repeat("actual source bytes\n", 900)
	referenceFile(t, p, "nested/original.txt", content)
	tab := resourceFileTab(t, s, p, "nested/original.txt")
	page, err := s.WorkspaceResource(t.Context(), p.ID, tab)
	if err != nil || len(page.Body.Data) != WorkspaceResourcePageBytes || !page.Body.HasMore || string(page.Body.Data) != content[:WorkspaceResourcePageBytes] {
		t.Fatal("actual range was not read", page.Body, err)
	}
	tab.Offset = page.Body.NextOffset
	next, err := s.WorkspaceResource(t.Context(), p.ID, tab)
	if err != nil || !bytes.Equal(next.Body.Data, []byte(content[tab.Offset:tab.Offset+WorkspaceResourcePageBytes])) {
		t.Fatal("wrong next source range", err)
	}
	other, err := s.Create(t.Context(), "Other", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.WorkspaceResource(t.Context(), other.ID, tab); err == nil {
		t.Fatal("foreign session read source")
	}
	referenceFile(t, p, "nested/original.txt", "replacement")
	if _, err = s.WorkspaceResource(t.Context(), p.ID, tab); !errors.Is(err, ErrWorkspaceReferenceChanged) {
		t.Fatal("changed source was displayed as same selection", err)
	}
	if s.Runtime.HasActive() {
		t.Fatal("file observation executed")
	}
	detail, err := s.Store.Detail(t.Context(), p.ID)
	branch, branchErr := ActiveBranch(detail)
	if err != nil || branchErr != nil || branch.NativeSessionID != "" {
		t.Fatal("file observation created native records", err, branchErr)
	}
}

func TestWorkspaceResourcesRootFilesSurvivePartialMetadataBudgetAndPageHonestly(t *testing.T) {
	s, p := resourceService(t)
	referenceFile(t, p, "README.md", "root original")
	referenceFile(t, p, "README.extra.md", "another root original")
	for i := 0; i < maxWorkspaceIndexNodes; i++ {
		referenceFile(t, p, fmt.Sprintf(".hidden/deep/child-%04d.txt", i), "body is not an index entry")
	}
	first, err := s.SearchWorkspaceReferences(t.Context(), p.ID, "README", "", 1)
	if err != nil || len(first.Entries) != 1 || !first.HasMore || first.Next == "" || !first.Truncated || first.Scanned != maxWorkspaceIndexNodes {
		t.Fatal("root files were displaced or the bounded index overclaimed completeness", first, err)
	}
	next, err := s.SearchWorkspaceReferences(t.Context(), p.ID, "README", first.Next, 1)
	if err != nil || len(next.Entries) != 1 || next.HasMore || next.Next != "" || !next.Truncated || next.Scanned != maxWorkspaceIndexNodes || first.Entries[0].ID == next.Entries[0].ID {
		t.Fatal("partial-index pagination lost a root source or implied an unscanned continuation", next, err)
	}
	paths := map[string]bool{first.Entries[0].Path: true, next.Entries[0].Path: true}
	if !paths["README.md"] || !paths["README.extra.md"] {
		t.Fatal("shallow root files did not retain the metadata budget", paths)
	}
	page, err := s.WorkspaceResource(t.Context(), p.ID, hub.WorkspaceTab{ID: "files", Kind: "files", Title: "Files", BranchID: p.ActiveBranchID, Query: "README.md"})
	if err != nil || len(page.Entries) != 1 || page.Entries[0].Detail != "README.md" || page.HasMore || page.After != "" || len(page.Body.Data) != 0 || !strings.Contains(page.Processing, "metadata only") || !strings.Contains(page.Processing, "partial") {
		t.Fatal("Files did not preserve the real root source or partial metadata-only scope", page, err)
	}
	if s.Runtime.HasActive() {
		t.Fatal("metadata indexing started execution")
	}
}

func TestWorkspaceResourcesRejectSymlinkSensitiveMissingAndForgedFiles(t *testing.T) {
	s, p := resourceService(t)
	referenceFile(t, p, "readable.txt", "original")
	tab := resourceFileTab(t, s, p, "readable.txt")
	out := filepath.Join(t.TempDir(), "outside.txt")
	if err := os.WriteFile(out, []byte("outside"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Remove(filepath.Join(p.Workspace, "readable.txt")); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(out, filepath.Join(p.Workspace, "readable.txt")); err != nil {
		t.Fatal(err)
	}
	if _, err := s.WorkspaceResource(t.Context(), p.ID, tab); err == nil {
		t.Fatal("retargeted symlink read")
	}
	tab.File.Path = "../outside.txt"
	if _, err := s.WorkspaceResource(t.Context(), p.ID, tab); err == nil {
		t.Fatal("traversal read")
	}
	tab.File.Path = ".env"
	if err := os.WriteFile(filepath.Join(p.Workspace, ".env"), []byte("private"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := s.WorkspaceResource(t.Context(), p.ID, tab); err == nil {
		t.Fatal("sensitive file read")
	}
	tab.File.Path = "missing.txt"
	if _, err := s.WorkspaceResource(t.Context(), p.ID, tab); err == nil {
		t.Fatal("missing file became fake preview")
	}
}

func TestWorkspaceResourcesOriginalArtifactAndTaskAreActualOwnedFacts(t *testing.T) {
	s, p := resourceService(t)
	attachment, err := s.UploadReader(t.Context(), p.ID, "original.txt", strings.NewReader("actual immutable original"))
	if err != nil {
		t.Fatal(err)
	}
	detail, err := s.Store.Detail(t.Context(), p.ID)
	if err != nil {
		t.Fatal(err)
	}
	tab := hub.WorkspaceTab{ID: "artifact", Kind: "artifact", Title: "original.txt", BranchID: detail.Session.ActiveBranchID, URI: attachment.Reference.URI}
	page, err := s.WorkspaceResource(t.Context(), p.ID, tab)
	if err != nil || string(page.Body.Data) != "actual immutable original" || page.Body.SHA256 != attachment.Info.SHA256 {
		t.Fatal(page, err)
	}
	other, err := s.Create(t.Context(), "Other", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.WorkspaceResource(t.Context(), other.ID, tab); err == nil {
		t.Fatal("foreign artifact read")
	}
	team, err := s.Teams.Store.CreateTeam(t.Context(), p.ID)
	if err != nil {
		t.Fatal(err)
	}
	worker, err := s.Teams.Store.AddWorker(t.Context(), p.ID, team.Team.ID, "Observer task worker", s.DefaultTarget)
	if err != nil {
		t.Fatal(err)
	}
	task, err := s.Teams.Store.EnqueueWorker(t.Context(), p.ID, team.Team.ID, worker.ID, "Actual committed queued scope", nil)
	if err != nil {
		t.Fatal(err)
	}
	taskTab := hub.WorkspaceTab{ID: "task", Kind: "task", Title: "Task", BranchID: p.ActiveBranchID, TeamID: team.Team.ID, WorkerID: worker.ID, TaskID: task.ID, TaskSequence: task.Sequence}
	page, err = s.WorkspaceResource(t.Context(), p.ID, taskTab)
	if err != nil || !strings.Contains(string(page.Body.Data), task.Scope) {
		t.Fatal("committed task not read", page, err)
	}
	taskTab.TaskID = "another-task"
	if _, err = s.WorkspaceResource(t.Context(), p.ID, taskTab); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("forged task accepted", err)
	}
	if s.Runtime.HasActive() || s.Teams.Status(worker.ID).Active {
		t.Fatal("observation dispatched queue")
	}
}

type blockingWorkspaceViewStore struct {
	started, release chan struct{}
}

func (s blockingWorkspaceViewStore) WorkspaceView(ctx context.Context, id string) (hub.WorkspaceView, error) {
	close(s.started)
	<-ctx.Done()
	<-s.release
	return hub.WorkspaceView{}, ctx.Err()
}
func (s blockingWorkspaceViewStore) SaveWorkspaceView(context.Context, hub.WorkspaceView, uint64) (hub.WorkspaceView, error) {
	panic("unexpected save")
}

func TestWorkspaceViewsCloseWaitsForActualReadCleanup(t *testing.T) {
	store := blockingWorkspaceViewStore{started: make(chan struct{}), release: make(chan struct{})}
	owner, err := NewWorkspaceViewsService(store)
	if err != nil {
		t.Fatal(err)
	}
	done := make(chan error, 1)
	go func() { _, err := owner.Read(context.Background(), "session"); done <- err }()
	<-store.started
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Millisecond)
	defer cancel()
	if err = owner.Close(ctx); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("claimed cleanup before actual reader exit", err)
	}
	if _, err = owner.Read(t.Context(), "another"); err == nil {
		t.Fatal("closed owner admitted another read")
	}
	close(store.release)
	if err = <-done; !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	if err = owner.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
}

type workspaceReceiptStore struct{ read, saved hub.WorkspaceView }

func (s *workspaceReceiptStore) WorkspaceView(context.Context, string) (hub.WorkspaceView, error) {
	return s.read, nil
}
func (s *workspaceReceiptStore) SaveWorkspaceView(context.Context, hub.WorkspaceView, uint64) (hub.WorkspaceView, error) {
	return s.saved, nil
}

func TestWorkspaceViewsRejectForeignNoopAndMalformedStoreReceipts(t *testing.T) {
	view := hub.WorkspaceView{SessionID: "owner", Revision: 7, Active: "files", Tabs: []hub.WorkspaceTab{{ID: "files", Kind: "files", Title: "Files", BranchID: "branch", ScrollY: 70}}}
	for _, test := range []struct {
		name   string
		change func(*hub.WorkspaceView)
	}{
		{"foreign", func(v *hub.WorkspaceView) { v.SessionID = "another-session" }},
		{"noop", func(v *hub.WorkspaceView) { v.Revision = 7 }},
		{"skipped-revision", func(v *hub.WorkspaceView) { v.Revision = 9 }},
		{"changed-position", func(v *hub.WorkspaceView) { v.Tabs[0].ScrollY = 0 }},
		{"changed-title", func(v *hub.WorkspaceView) { v.Tabs[0].Title = "Other" }},
		{"changed-width", func(v *hub.WorkspaceView) { v.Width = 640 }},
		{"invalid", func(v *hub.WorkspaceView) { v.Tabs[0].Kind = "unsupported" }},
	} {
		t.Run(test.name, func(t *testing.T) {
			receipt := hub.CloneWorkspaceView(view)
			receipt.Revision++
			test.change(&receipt)
			store := &workspaceReceiptStore{read: hub.CloneWorkspaceView(view), saved: receipt}
			owner, err := NewWorkspaceViewsService(store)
			if err != nil {
				t.Fatal(err)
			}
			defer owner.Close(t.Context())
			if got, err := owner.Save(t.Context(), view, view.Revision); err == nil || got.Revision != 0 {
				t.Fatal("bad save receipt was published", got, err)
			}
			if view.Tabs[0].Title != "Files" || view.Tabs[0].ScrollY != 70 {
				t.Fatal("store mutated caller's view")
			}
		})
	}
	for _, bad := range []hub.WorkspaceView{{SessionID: "foreign"}, {SessionID: "owner", Active: "missing"}, {SessionID: "owner", Tabs: []hub.WorkspaceTab{{ID: "bad", Kind: "unknown"}}}} {
		owner, err := NewWorkspaceViewsService(&workspaceReceiptStore{read: bad})
		if err != nil {
			t.Fatal(err)
		}
		if got, err := owner.Read(t.Context(), "owner"); err == nil || got.SessionID != "" {
			t.Fatal("invalid/foreign read receipt published", got, err)
		}
		if err = owner.Close(t.Context()); err != nil {
			t.Fatal(err)
		}
	}
	good := hub.CloneWorkspaceView(view)
	good.Revision++
	store := &workspaceReceiptStore{read: view, saved: good}
	owner, err := NewWorkspaceViewsService(store)
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Close(t.Context())
	got, err := owner.Read(t.Context(), "owner")
	if err != nil {
		t.Fatal(err)
	}
	got.Tabs[0].Title = "caller edit"
	if store.read.Tabs[0].Title != "Files" {
		t.Fatal("read returned store-owned mutable slice")
	}
	got, err = owner.Save(t.Context(), view, 7)
	if err != nil || got.Revision != 8 {
		t.Fatal("valid exact receipt rejected", got, err)
	}
	got.Tabs[0].Title = "caller edit"
	if store.saved.Tabs[0].Title != "Files" {
		t.Fatal("save returned store-owned mutable slice")
	}
}

func TestWorkspaceViewWidthBoundsRemainExplicit(t *testing.T) {
	for _, width := range []float32{-1, 319, 961, float32(math.NaN()), float32(math.Inf(1)), float32(math.Inf(-1))} {
		view := hub.WorkspaceView{SessionID: "owner", Width: width}
		if err := view.Validate(); err == nil {
			t.Fatal("invalid preferred resource width accepted", width)
		}
	}
	for _, width := range []float32{0, 320, 432, 960} {
		view := hub.WorkspaceView{SessionID: "owner", Width: width}
		if err := view.Validate(); err != nil {
			t.Fatal("valid resource width rejected", width, err)
		}
	}
}

// INPUT: Real Hub assembly with and without an explicit approved Git executable.
// OUTPUT: Optional developer services appear only when their backend/source exists; no startup execution.
// POS: [L7 Hub assembly tests] Ordinary chat remains available while approved-Git navigation is explicitly unavailable.
package bootstrap

import (
	"context"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
)

func TestDeveloperWorkspaceServicesFollowExplicitBackendAvailability(t *testing.T) {
	withoutGit, err := Open(t.Context(), Config{
		OpenStores: testStores, OpenMemory: LocalMemory, OpenBindings: LocalBindings,
		DataDirectory: filepath.Join(t.TempDir(), "private"), Demo: true,
	})
	if err != nil {
		t.Fatal(err)
	}
	if withoutGit.Service.DeveloperWorkspaces != nil || withoutGit.Service.Worktrees != nil {
		t.Fatal("approved-Git capability appeared without an explicit executable/consent")
	}
	if withoutGit.Service.ConversationTabs == nil || withoutGit.Runtime.HasActive() {
		t.Fatal("ordinary chat backend lost optional tab metadata or startup executed")
	}
	closeApp(t, withoutGit)

	withGit, err := Open(t.Context(), Config{
		OpenStores: testStores, OpenMemory: LocalMemory, OpenBindings: LocalBindings,
		DataDirectory: filepath.Join(t.TempDir(), "private"), Demo: true,
		GitExecutable: "/usr/bin/git", GitHostAccessApproved: true,
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { closeApp(t, withGit) })
	if withGit.Service.Worktrees == nil || withGit.Service.DeveloperWorkspaces == nil || withGit.Service.ConversationTabs == nil {
		t.Fatal("explicit approved-Git assembly did not install developer services")
	}
	repository, err := withGit.Service.DeveloperWorkspaces.Import(t.Context(), worktreeRepo(t))
	if err != nil {
		t.Fatal(err)
	}
	worktrees, err := withGit.Service.DeveloperWorkspaces.Worktrees(t.Context(), repository.ID)
	if err != nil || len(worktrees) != 1 || !worktrees[0].Main {
		t.Fatal("assembled explicit import did not expose its live main worktree", worktrees, err)
	}
	if withGit.Runtime.HasActive() {
		t.Fatal("repository/worktree import started execution")
	}
	draft := hub.ComposerDraft{Scope: hub.ComposerDraftScope{Kind: "home", ID: hub.ConversationTabDraftID("assembly-tab")}, Revision: 1, Text: "captured"}
	if _, err = withGit.Service.ConversationTabs.CreateFirstSession(context.Background(), draft, "unpersisted draft", hub.WorktreeScope{RepositoryID: repository.ID, Path: repository.Path}, "assembly-tab", 0); err == nil {
		t.Fatal("assembly accepted a first Send without the durable home draft/tab set")
	}
}

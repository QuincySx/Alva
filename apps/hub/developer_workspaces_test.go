// INPUT: Pure repository/worktree/tab DTO values and the frozen metadata budgets.
// OUTPUT: Stable draft derivation, 1024-item support, active/archive and duplicate-binding validation.
// POS: [L7 Hub product tests] Conversation tabs remain distinct from resource tabs and execution state.
package hub

import (
	"fmt"
	"path/filepath"
	"testing"
)

func TestConversationTabSetSupportsTenHundredTwentyFourMetadataItems(t *testing.T) {
	scope := WorktreeScope{RepositoryID: "repository-test", Path: filepath.Join(t.TempDir(), "worktree")}
	tabs := make([]ConversationTab, MaxConversationTabs)
	for index := range tabs {
		id := fmt.Sprintf("tab-%04d", index)
		tabs[index] = ConversationTab{ID: id, DraftID: ConversationTabDraftID(id), Title: fmt.Sprintf("Conversation %d", index), CreatedAt: 1}
	}
	tabs[0].SelectedAt = 1
	value := ConversationTabSet{Scope: scope, Tabs: tabs, ActiveTabID: tabs[0].ID}
	if err := value.Validate(); err != nil {
		t.Fatal("1024 metadata items were treated as a hard product total limit", err)
	}
	extraID := "tab-over-budget"
	value.Tabs = append(value.Tabs, ConversationTab{ID: extraID, DraftID: ConversationTabDraftID(extraID), Title: "Over", CreatedAt: 1})
	if value.Validate() == nil {
		t.Fatal("metadata abuse budget above 1024 was accepted")
	}
}

func TestConversationTabDraftActiveArchiveAndSessionBindingsAreUnambiguous(t *testing.T) {
	scope := WorktreeScope{RepositoryID: "repository-test", Path: filepath.Join(t.TempDir(), "worktree")}
	first := ConversationTab{ID: "first", DraftID: ConversationTabDraftID("first"), Title: "First", CreatedAt: 1, SelectedAt: 1}
	second := ConversationTab{ID: "second", DraftID: ConversationTabDraftID("second"), Title: "Second", CreatedAt: 1}
	if err := (ConversationTabSet{Scope: scope, Tabs: []ConversationTab{first, second}, ActiveTabID: first.ID}).Validate(); err != nil {
		t.Fatal(err)
	}
	if err := (ConversationTabSet{Scope: scope, Tabs: []ConversationTab{first, second}}).Validate(); err == nil {
		t.Fatal("unarchived set without explicit active selection was accepted")
	}
	archived := first
	archived.Archived, archived.SelectedAt = true, 0
	if err := (ConversationTabSet{Scope: scope, Tabs: []ConversationTab{archived}}).Validate(); err != nil {
		t.Fatal("all-archived set could not preserve metadata", err)
	}
	badDraft := second
	badDraft.DraftID = "borrowed-draft"
	if err := (ConversationTabSet{Scope: scope, Tabs: []ConversationTab{first, badDraft}, ActiveTabID: first.ID}).Validate(); err == nil {
		t.Fatal("arbitrary draft identity was accepted")
	}
	first.SessionID, second.SessionID = "hub-one", "hub-one"
	if err := (ConversationTabSet{Scope: scope, Tabs: []ConversationTab{first, second}, ActiveTabID: first.ID}).Validate(); err == nil {
		t.Fatal("one HubSession was accepted in multiple tabs")
	}
}

// INPUT: Actual Foundation controls, temporary SQLite/Hub services and delayed companion storage owners.
// OUTPUT: Group draft/scope cancellation, disabled-member removal and late-mutation/private-navigation isolation regressions.
// POS: [L7 Hub UI tests] Uses no external accounts, user data or OS window; committed facts remain the execution authority.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"fmt"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

func companionEditableGroup(t *testing.T) (*NativePage, *ui.Tester, hub.CompanionGroup, []hub.CompanionProfile) {
	t.Helper()
	p, tester := companionPage(t)
	tester = ui.NewTester(workspaceStaticView(p), 1240, 1100)
	var profiles []hub.CompanionProfile
	var members []string
	for _, name := range []string{"Draft research", "Draft review"} {
		profile, err := p.Service.SaveCompanionProfile(t.Context(), hub.CompanionProfile{OwnerID: hub.LocalCompanionOwner, Name: name, Target: p.Service.DefaultTarget, Enabled: true}, 0)
		if err != nil {
			t.Fatal(err)
		}
		profiles = append(profiles, profile)
		members = append(members, profile.ID)
	}
	g, err := p.Service.SaveCompanionGroup(t.Context(), hub.CompanionGroup{OwnerID: hub.LocalCompanionOwner, Name: "Committed group", Members: members, ReplyMode: "mentions", ReplyLimit: 2, Workspace: t.TempDir(), Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	p.reloadWorkspaceCompanions()
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && len(p.companions.groups) == 1 })
	companionClick(t, tester, "Open group "+g.Name)
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
	return p, tester, g, profiles
}

func companionReplace(t *testing.T, tester *ui.Tester, label, value string) {
	t.Helper()
	companionClick(t, tester, label)
	tester.Key(ui.Cmd, ui.KeyA)
	tester.Type(value)
	tester.Frame()
}

func companionSelect(t *testing.T, tester *ui.Tester, label, value string) {
	t.Helper()
	companionClick(t, tester, label)
	companionClick(t, tester, value)
}

func TestWorkspaceCompanionGroupCancelKeepsCommittedExecutionPolicy(t *testing.T) {
	p, tester, g, profiles := companionEditableGroup(t)
	companionClick(t, tester, "Edit companion or group")
	companionReplace(t, tester, "Companion group name", "Discarded group")
	companionSelect(t, tester, "Group reply policy", "所有成员")
	companionSelect(t, tester, "Group reply limit", "1")
	companionReplace(t, tester, "Group workspace", t.TempDir())
	companionClick(t, tester, "Group member "+profiles[1].Name)
	companionClick(t, tester, "Cancel companion group")
	saved, err := p.Service.Companions.Group(t.Context(), g.OwnerID, g.ID)
	if err != nil || !reflect.DeepEqual(saved, g) || !reflect.DeepEqual(p.companions.group, g) {
		t.Fatalf("cancel changed the committed group: UI=%+v store=%+v error=%v", p.companions.group, saved, err)
	}
	companionClick(t, tester, "Mention companion "+profiles[0].Name)
	companionClick(t, tester, "Approve group accounts and billing routes")
	companionType(t, tester, "Companion group message", "Execute the saved mentions policy exactly once")
	companionClick(t, tester, "Send companion group round")
	waitWorkspace(t, p, tester, func() bool { return !p.companions.running && !p.companions.busy })
	if p.companions.problem != "" || len(p.companions.rounds) != 1 || len(p.companions.rounds[0].Members) != 1 || p.companions.rounds[0].Members[0].CompanionID != profiles[0].ID || len(p.companions.messages) != 2 {
		t.Fatal("cancelled draft altered the real round admission", p.companions.problem, p.companions.rounds, p.companions.messages)
	}
}

func TestWorkspaceCompanionGroupScopeSaveDropsPreviousFactsAndReadsNewScope(t *testing.T) {
	p, tester, g, profiles := companionEditableGroup(t)
	old, err := p.Service.RunCompanionGroup(context.Background(), g.OwnerID, g.ID, g.Revision, "Only the old scope may show this prompt", []string{profiles[0].ID}, true)
	if err != nil {
		t.Fatal(err)
	}
	p.loadWorkspaceCompanionGroup()
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && len(p.companions.messages) == 2 })
	p.companions.roundApprovals = map[string]uint64{old.ID: old.Revision}
	p.companions.terminationRound = old.ID
	store, release := companionGroupReadBarrier(t, p)
	store.holdRead.Store(true)
	companionClick(t, tester, "Edit companion or group")
	companionClick(t, tester, "Group member "+profiles[1].Name)
	companionReplace(t, tester, "Group workspace", t.TempDir())
	companionClick(t, tester, "Save companion group")
	waitWorkspace(t, p, tester, func() bool {
		select {
		case <-store.readCaptured:
			return p.companions.editor == ""
		default:
			return false
		}
	})
	v := &p.companions
	if v.scope != g.ScopeRevision+1 || v.group.ScopeRevision != v.scope || len(v.messages) != 0 || len(v.rounds) != 0 || len(v.roundReviews) != 0 || len(v.roundApprovals) != 0 || v.terminationRound != "" || v.messageBefore != "" || v.roundBefore != "" || v.moreMessages || v.moreRounds {
		t.Fatal("new scope retained previous group facts or recovery approvals", v)
	}
	if tester.HasText("Only the old scope may show this prompt") || p.Service.Runtime.HasActive() {
		t.Fatal("scope edit displayed old text or executed")
	}
	release()
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
	messages, err := p.Service.Companions.Messages(t.Context(), g.OwnerID, g.ID, g.ScopeRevision, 256)
	if err != nil || len(messages) != 2 {
		t.Fatal("scope edit lost the old saved facts", messages, err)
	}
	if rounds, err := p.Service.Companions.Rounds(t.Context(), g.OwnerID, g.ID, v.scope, 32); err != nil || len(rounds) != 0 {
		t.Fatal("scope edit executed a new round", rounds, err)
	}
}

func companionGroupReadBarrier(t *testing.T, p *NativePage) (*companionWorkspaceReadBarrierStore, func()) {
	t.Helper()
	if err := p.Service.Companions.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	store := &companionWorkspaceReadBarrierStore{CompanionStore: p.Service.Store.(hub.CompanionStore), readKind: "group", readCaptured: make(chan struct{}), readRelease: make(chan struct{})}
	var err error
	p.Service.Companions, err = product.NewCompanionService(store)
	if err != nil {
		t.Fatal(err)
	}
	var once sync.Once
	release := func() { once.Do(func() { close(store.readRelease) }) }
	t.Cleanup(release)
	return store, release
}

func TestWorkspaceCompanionGroupSaveFailureKeepsDraftAndCommittedScope(t *testing.T) {
	p, tester, g, profiles := companionEditableGroup(t)
	companionClick(t, tester, "Edit companion or group")
	companionReplace(t, tester, "Companion group name", "Retry this exact draft")
	companionSelect(t, tester, "Group reply policy", "所有成员")
	companionSelect(t, tester, "Group reply limit", "1")
	companionClick(t, tester, "Group member "+profiles[1].Name)
	companionReplace(t, tester, "Group workspace", filepath.Join(t.TempDir(), "missing-workspace"))
	companionClick(t, tester, "Save companion group")
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
	if p.companions.editor != "group" || p.companions.problem == "" || !reflect.DeepEqual(p.companions.group, g) || p.companions.scope != g.ScopeRevision {
		t.Fatal("failed save lost the draft or changed committed execution facts", p.companions)
	}
	saved, err := p.Service.Companions.Group(t.Context(), g.OwnerID, g.ID)
	if err != nil || !reflect.DeepEqual(saved, g) {
		t.Fatal("failed save changed storage", saved, err)
	}
	companionReplace(t, tester, "Group workspace", g.Workspace)
	companionClick(t, tester, "Save companion group")
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && p.companions.editor == "" })
	saved = p.companions.group
	if saved.Name != "Retry this exact draft" || saved.ReplyMode != "all" || saved.ReplyLimit != 1 || !reflect.DeepEqual(saved.Members, []string{profiles[0].ID}) || saved.ScopeRevision != g.ScopeRevision+1 || p.Service.Runtime.HasActive() {
		t.Fatal("failed-save retry did not preserve the full draft", saved)
	}
}

func TestWorkspaceCompanionGroupMetadataSavePreservesHistoryAndNavigationDiscardsDraft(t *testing.T) {
	p, tester, g, profiles := companionEditableGroup(t)
	if _, err := p.Service.RunCompanionGroup(t.Context(), g.OwnerID, g.ID, g.Revision, "Retain this same-scope fact", []string{profiles[0].ID}, true); err != nil {
		t.Fatal(err)
	}
	p.loadWorkspaceCompanionGroup()
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && len(p.companions.messages) == 2 })
	messages := append([]hub.CompanionMessage(nil), p.companions.messages...)
	rounds := append([]hub.CompanionRound(nil), p.companions.rounds...)
	p.companions.prompt = "Preserve the unsent same-scope message"
	companionClick(t, tester, "Edit companion or group")
	companionReplace(t, tester, "Companion group name", "Renamed group")
	companionSelect(t, tester, "Group reply policy", "所有成员")
	companionClick(t, tester, "Save companion group")
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && p.companions.editor == "" })
	if p.companions.scope != g.ScopeRevision || p.companions.group.ScopeRevision != g.ScopeRevision || !reflect.DeepEqual(p.companions.messages, messages) || !reflect.DeepEqual(p.companions.rounds, rounds) || p.companions.prompt != "Preserve the unsent same-scope message" {
		t.Fatal("metadata save cleared unrelated same-scope history or composer", p.companions)
	}
	g = p.companions.group
	companionClick(t, tester, "Edit companion or group")
	companionReplace(t, tester, "Companion group name", "Leave this in the form")
	companionSelect(t, tester, "Group reply policy", "仅 @ 的伙伴")
	p.setPane("")
	p.openWorkspaceCompanions()
	tester.Frame()
	if !reflect.DeepEqual(p.companions.group, g) || p.companions.editor != "group" {
		t.Fatal("back navigation published the unsaved form", p.companions)
	}
	other, err := p.Service.SaveCompanionGroup(t.Context(), hub.CompanionGroup{OwnerID: g.OwnerID, Name: "Other committed group", Members: []string{profiles[1].ID}, ReplyMode: "mentions", ReplyLimit: 1, Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	p.reloadWorkspaceCompanions()
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
	companionClick(t, tester, "Open group "+other.Name)
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
	if p.companions.editor != "" || !reflect.DeepEqual(p.companions.group, other) || len(p.companions.messages) != 0 || p.companions.prompt != "" {
		t.Fatal("another group inherited the first group's draft or facts", p.companions)
	}
	companionClick(t, tester, "Open group "+g.Name)
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
	companionClick(t, tester, "Edit companion or group")
	companionClick(t, tester, "Save companion group")
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && p.companions.editor == "" })
	if p.companions.group.Name != g.Name || p.companions.group.ReplyMode != g.ReplyMode || !reflect.DeepEqual(p.companions.group.Members, g.Members) {
		t.Fatal("reopening a committed group reused a discarded draft", p.companions.group)
	}
}

func TestWorkspaceCompanionGroupLateOldScopeReadCannotRefillNewScope(t *testing.T) {
	for _, older := range []bool{false, true} {
		t.Run(map[bool]string{false: "latest", true: "older"}[older], func(t *testing.T) {
			p, tester, g, profiles := companionEditableGroup(t)
			store := p.Service.Store.(hub.CompanionStore)
			// Fill enough real facts for one keyset page and its continuation.
			for i := 0; i < 33; i++ {
				round, err := store.BeginCompanionRound(t.Context(), g.OwnerID, g.ID, g.Revision, "Old scope only", []string{profiles[0].ID})
				if err != nil {
					t.Fatal(err)
				}
				if err = store.FinishCompanionRound(t.Context(), g.OwnerID, round.ID, "completed", ""); err != nil {
					t.Fatal(err)
				}
			}
			p.loadWorkspaceCompanionGroup()
			waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && p.companions.moreRounds })
			barrier, release := companionGroupReadBarrier(t, p)
			barrier.holdRead.Store(true)
			if older {
				p.loadOlderWorkspaceCompanionHistory(true)
			} else {
				p.loadWorkspaceCompanionGroup()
			}
			select {
			case <-barrier.readCaptured:
			case <-time.After(5 * time.Second):
				t.Fatal("old-scope observer did not capture its page")
			}
			g.Workspace = t.TempDir()
			g, err := p.Service.SaveCompanionGroup(t.Context(), g, g.Revision)
			if err != nil {
				t.Fatal(err)
			}
			p.selectWorkspaceCompanionGroup(g)
			if len(p.companions.messages) != 0 || len(p.companions.rounds) != 0 || !p.companions.groupReloadPending {
				t.Fatal("scope navigation retained facts while an old read was pending", p.companions)
			}
			release()
			waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
			if len(p.companions.messages) != 0 || len(p.companions.rounds) != 0 || p.companions.scope != 2 || p.companions.moreRounds || p.companions.roundBefore != "" || p.Service.Runtime.HasActive() {
				t.Fatal("late old-scope page replaced the new cold scope", p.companions)
			}
		})
	}
}

type companionGroupSaveBarrierStore struct {
	hub.CompanionStore
	hub.CompanionHistoryStore
	entered, release chan struct{}
}

func (s *companionGroupSaveBarrierStore) SaveCompanionGroup(ctx context.Context, g hub.CompanionGroup, revision uint64) (hub.CompanionGroup, error) {
	close(s.entered)
	<-s.release
	return s.CompanionStore.SaveCompanionGroup(ctx, g, revision)
}

func TestWorkspaceCompanionGroupLateSaveCannotReplaceAnotherGroup(t *testing.T) {
	p, tester, g, profiles := companionEditableGroup(t)
	other, err := p.Service.SaveCompanionGroup(t.Context(), hub.CompanionGroup{OwnerID: g.OwnerID, Name: "Navigation target", Members: []string{profiles[1].ID}, ReplyMode: "mentions", ReplyLimit: 1, Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	if err = p.Service.Companions.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	store := &companionGroupSaveBarrierStore{CompanionStore: p.Service.Store.(hub.CompanionStore), CompanionHistoryStore: p.Service.Store.(hub.CompanionHistoryStore), entered: make(chan struct{}), release: make(chan struct{})}
	p.Service.Companions, err = product.NewCompanionService(store)
	if err != nil {
		t.Fatal(err)
	}
	var once sync.Once
	release := func() { once.Do(func() { close(store.release) }) }
	t.Cleanup(release)
	companionClick(t, tester, "Edit companion or group")
	companionReplace(t, tester, "Companion group name", "Saved original group")
	companionClick(t, tester, "Save companion group")
	select {
	case <-store.entered:
	case <-time.After(5 * time.Second):
		t.Fatal("group save did not reach the actual store")
	}
	// A route change can outlive its disabled roster controls; the old receipt
	// still belongs to the group captured at admission.
	p.selectWorkspaceCompanionGroup(other)
	release()
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
	if p.companions.selected != other.ID || !reflect.DeepEqual(p.companions.group, other) || p.companions.editor != "" || len(p.companions.messages) != 0 {
		t.Fatal("late save replaced the selected group", p.companions)
	}
	saved, err := p.Service.Companions.Group(t.Context(), g.OwnerID, g.ID)
	if err != nil || saved.Name != "Saved original group" || saved.Revision != g.Revision+1 {
		t.Fatal("navigation lost the actual save receipt", saved, err)
	}
	if p.Service.Runtime.HasActive() {
		t.Fatal("saving during navigation executed")
	}
}

func TestWorkspaceCompanionGroupCanRemoveAnAlreadyDisabledMember(t *testing.T) {
	p, tester, g, profiles := companionEditableGroup(t)
	disabled := profiles[1]
	disabled.Enabled = false
	if _, err := p.Service.SaveCompanionProfile(t.Context(), disabled, disabled.Revision); err != nil {
		t.Fatal(err)
	}
	p.reloadWorkspaceCompanions()
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
	companionClick(t, tester, "Edit companion or group")
	companionClick(t, tester, "Group member "+disabled.Name)
	if p.companions.members[disabled.ID] {
		t.Fatal("an already selected disabled member cannot be removed")
	}
	// Removing a stopped member must not make it selectable again while disabled.
	companionClick(t, tester, "Group member "+disabled.Name)
	if p.companions.members[disabled.ID] {
		t.Fatal("the editor re-added a disabled member")
	}
	companionClick(t, tester, "Save companion group")
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && p.companions.editor == "" })
	saved, err := p.Service.Companions.Group(t.Context(), g.OwnerID, g.ID)
	if err != nil || !reflect.DeepEqual(saved.Members, []string{profiles[0].ID}) || saved.ScopeRevision != g.ScopeRevision+1 || p.Service.Runtime.HasActive() {
		t.Fatal("removing the disabled member did not create the expected cold scope", saved, err)
	}
}

type companionMutationBarrierStore struct {
	hub.CompanionStore
	hub.CompanionHistoryStore
	kind             string
	fail             bool
	entered, release chan struct{}
}

func (s *companionMutationBarrierStore) pause(kind string) error {
	if s.kind != kind {
		return nil
	}
	close(s.entered)
	<-s.release
	if s.fail {
		return errors.New("fixture old selection mutation failed")
	}
	return nil
}
func (s *companionMutationBarrierStore) SaveCompanion(ctx context.Context, p hub.CompanionProfile, revision uint64) (hub.CompanionProfile, error) {
	if err := s.pause("profile-save"); err != nil {
		return hub.CompanionProfile{}, err
	}
	return s.CompanionStore.SaveCompanion(ctx, p, revision)
}
func (s *companionMutationBarrierStore) SaveCompanionGroup(ctx context.Context, g hub.CompanionGroup, revision uint64) (hub.CompanionGroup, error) {
	if err := s.pause("group-save"); err != nil {
		return hub.CompanionGroup{}, err
	}
	return s.CompanionStore.SaveCompanionGroup(ctx, g, revision)
}
func (s *companionMutationBarrierStore) DeleteCompanion(ctx context.Context, owner, id string, revision uint64) error {
	if err := s.pause("profile-delete"); err != nil {
		return err
	}
	return s.CompanionStore.DeleteCompanion(ctx, owner, id, revision)
}
func (s *companionMutationBarrierStore) DeleteCompanionGroup(ctx context.Context, owner, id string, revision uint64) error {
	if err := s.pause("group-delete"); err != nil {
		return err
	}
	return s.CompanionStore.DeleteCompanionGroup(ctx, owner, id, revision)
}
func (s *companionMutationBarrierStore) CompanionConversation(ctx context.Context, owner, id, group string, scope uint64) (hub.CompanionConversation, error) {
	c, err := s.CompanionStore.CompanionConversation(ctx, owner, id, group, scope)
	if pauseErr := s.pause("private-read"); pauseErr != nil {
		return hub.CompanionConversation{}, pauseErr
	}
	return c, err
}
func (s *companionMutationBarrierStore) OpenCompanionConversation(ctx context.Context, owner, id, group string, scope uint64, workspace string) (hub.CompanionConversation, error) {
	c, err := s.CompanionStore.OpenCompanionConversation(ctx, owner, id, group, scope, workspace)
	if pauseErr := s.pause("private-open"); pauseErr != nil {
		return hub.CompanionConversation{}, pauseErr
	}
	return c, err
}

func TestWorkspaceCompanionLateMutationKeepsCurrentSelection(t *testing.T) {
	for _, action := range []string{"profile-edit", "profile-toggle", "profile-delete", "group-toggle", "group-delete"} {
		for _, fail := range []bool{false, true} {
			t.Run(fmt.Sprintf("%s/failure=%t", action, fail), func(t *testing.T) {
				p, tester, g, profiles := companionEditableGroup(t)
				kind := "group-save"
				if strings.HasPrefix(action, "profile") {
					kind = "profile-save"
					p.selectWorkspaceCompanion(profiles[0])
					waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
				}
				if err := p.Service.Companions.Close(t.Context()); err != nil {
					t.Fatal(err)
				}
				if strings.HasSuffix(action, "delete") {
					kind = strings.TrimSuffix(action, "-delete") + "-delete"
				}
				store := &companionMutationBarrierStore{CompanionStore: p.Service.Store.(hub.CompanionStore), CompanionHistoryStore: p.Service.Store.(hub.CompanionHistoryStore), kind: kind, fail: fail, entered: make(chan struct{}), release: make(chan struct{})}
				var err error
				p.Service.Companions, err = product.NewCompanionService(store)
				if err != nil {
					t.Fatal(err)
				}
				var once sync.Once
				release := func() { once.Do(func() { close(store.release) }) }
				t.Cleanup(release)
				switch {
				case action == "profile-edit":
					p.companions.editor = "profile"
					p.companions.profile.Role = "Save this original profile only"
					p.saveWorkspaceCompanion()
				case strings.HasSuffix(action, "toggle"):
					p.toggleWorkspaceCompanion()
				default:
					p.deleteWorkspaceCompanion()
				}
				select {
				case <-store.entered:
				case <-time.After(5 * time.Second):
					t.Fatal("original mutation did not reach storage")
				}
				// Navigation can supersede an in-flight mutation even while its
				// local roster controls are disabled.
				if strings.HasPrefix(action, "profile") {
					p.selectWorkspaceCompanionGroup(g)
				} else {
					p.selectWorkspaceCompanion(profiles[1])
				}
				release()
				waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
				v := &p.companions
				if strings.HasPrefix(action, "profile") {
					if v.kind != "group" || v.selected != g.ID || !reflect.DeepEqual(v.group, g) || v.profile.ID != "" {
						t.Fatal("old profile mutation replaced the current group", v)
					}
				} else if v.kind != "profile" || v.selected != profiles[1].ID || !reflect.DeepEqual(v.profile, profiles[1]) || v.group.ID != "" {
					t.Fatal("old group mutation replaced the current profile", v)
				}
				if v.problem != "" || v.editor != "" || v.deleteConfirm || p.Service.Runtime.HasActive() {
					t.Fatal("old mutation leaked its error, editor or execution into the current selection", v)
				}
				if strings.HasPrefix(action, "profile") {
					saved, err := p.Service.Companions.Read(t.Context(), profiles[0].OwnerID, profiles[0].ID)
					if err != nil || !fail && saved.Revision != profiles[0].Revision+1 || fail && saved.Revision != profiles[0].Revision {
						t.Fatal("original profile mutation was lost or replayed", saved, err)
					}
				} else {
					saved, err := p.Service.Companions.Group(t.Context(), g.OwnerID, g.ID)
					if err != nil || !fail && saved.Revision != g.Revision+1 || fail && saved.Revision != g.Revision {
						t.Fatal("original group mutation was lost or replayed", saved, err)
					}
				}
			})
		}
	}
}

func TestWorkspaceCompanionLatePrivateReceiptKeepsSettingsReturn(t *testing.T) {
	for _, kind := range []string{"private-read", "private-open"} {
		t.Run(kind, func(t *testing.T) {
			p, tester := companionPage(t)
			createCompanionUI(t, p, tester, "Delayed private owner")
			profile := p.companions.profile
			if kind == "private-read" {
				if _, err := p.Service.OpenCompanion(t.Context(), profile.OwnerID, profile.ID, true); err != nil {
					t.Fatal(err)
				}
			}
			if err := p.Service.Companions.Close(t.Context()); err != nil {
				t.Fatal(err)
			}
			store := &companionMutationBarrierStore{CompanionStore: p.Service.Store.(hub.CompanionStore), CompanionHistoryStore: p.Service.Store.(hub.CompanionHistoryStore), kind: kind, entered: make(chan struct{}), release: make(chan struct{})}
			var err error
			p.Service.Companions, err = product.NewCompanionService(store)
			if err != nil {
				t.Fatal(err)
			}
			var once sync.Once
			release := func() { once.Do(func() { close(store.release) }) }
			t.Cleanup(release)
			if kind == "private-read" {
				companionClick(t, tester, "Open companion "+profile.Name)
			} else {
				companionClick(t, tester, "Approve companion account and billing route")
				companionClick(t, tester, "Open companion private conversation")
			}
			select {
			case <-store.entered:
			case <-time.After(5 * time.Second):
				t.Fatal("private conversation did not reach its actual store")
			}
			selected, generation := p.selected, p.generation
			companionClick(t, tester, "General settings")
			release()
			waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
			conversation := p.companions.conversation
			if p.pane != "preferences" || p.selected != selected || p.generation != generation || conversation.SessionID == "" || conversation.CompanionID != profile.ID || p.Service.Runtime.HasActive() {
				t.Fatal("late private receipt changed the active page or lost its committed conversation", p.pane, p.selected, conversation)
			}
			companionClick(t, tester, "Back to workspace")
			if p.pane != "companions" || p.companions.selected != profile.ID {
				t.Fatal("settings lost the original companion return destination", p.pane)
			}
			companionClick(t, tester, "查看私聊记录")
			waitWorkspace(t, p, tester, func() bool { return p.selected == conversation.SessionID && !p.polling })
			if p.Service.Runtime.HasActive() {
				t.Fatal("explicit history navigation executed")
			}
		})
	}
}

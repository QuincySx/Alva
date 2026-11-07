// INPUT: Real temporary product SQLite, explicit owner/Profile/group edits and reopen/migration boundaries.
// OUTPUT: Durable CRUD/CAS, complete keyset/deleted history and rejected tombstone Start/Wait Attempt claims.
// POS: [L7 Hub storage tests] No external account, model, process or user database is accessed.
package sqlite

import (
	"context"
	"errors"
	"fmt"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func companionProfile(t *testing.T, s *Store, name string) hub.CompanionProfile {
	t.Helper()
	p, err := s.SaveCompanion(t.Context(), hub.CompanionProfile{OwnerID: hub.LocalCompanionOwner, Name: name, Role: "verify sources", Target: target("a"), Enabled: true, PermissionPolicy: hub.PermissionAsk}, 0)
	if err != nil {
		t.Fatal(err)
	}
	return p
}
func TestCompanionDurableCRUDCASAndScopeIsolation(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "product")
	s, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	closed := false
	t.Cleanup(func() {
		if !closed {
			_ = s.Close()
		}
	})
	p := companionProfile(t, s, "Research")
	q := companionProfile(t, s, "Review")
	sessions, err := s.ListSessions(ctx, 100)
	if err != nil || len(sessions) != 0 {
		t.Fatal("saving profiles created a session", sessions, err)
	}
	original := p
	p.Name = "Renamed research"
	p, err = s.SaveCompanion(ctx, p, p.Revision)
	if err != nil || p.ID != original.ID || p.Revision != 2 {
		t.Fatal(p, err)
	}
	if _, err = s.SaveCompanion(ctx, original, original.Revision); !errors.Is(err, hub.ErrCompanionConflict) {
		t.Fatal("stale edit accepted", err)
	}
	if _, err = s.Companion(ctx, "foreign-owner", p.ID); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("foreign owner read profile", err)
	}
	private, err := s.OpenCompanionConversation(ctx, p.OwnerID, p.ID, "", 0, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	g, err := s.SaveCompanionGroup(ctx, hub.CompanionGroup{OwnerID: p.OwnerID, Name: "Product", Members: []string{p.ID, q.ID}, ReplyMode: "mentions", ReplyLimit: 2, Workspace: t.TempDir(), Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	round, err := s.BeginCompanionRound(ctx, p.OwnerID, g.ID, g.Revision, "first group request", []string{q.ID, p.ID})
	if err != nil {
		t.Fatal(err)
	}
	if len(round.Members) != 2 || round.Members[0].CompanionID != p.ID || round.Members[0].SessionID == private.SessionID || round.Members[0].SessionID == round.Members[1].SessionID {
		t.Fatal("roster order or private/group ownership leaked", round)
	}
	oldSnapshot := round.Members[0].Profile
	p.Role = "changed after admission"
	p, err = s.SaveCompanion(ctx, p, p.Revision)
	if err != nil {
		t.Fatal(err)
	}
	saved, err := s.CompanionRounds(ctx, p.OwnerID, g.ID, 1, 32)
	if err != nil || saved[0].Members[0].Profile.Role != oldSnapshot.Role {
		t.Fatal("running snapshot changed", err)
	}
	if _, err = s.SaveCompanionGroup(ctx, g, g.Revision); err == nil {
		t.Fatal("edited active group")
	}
	if err = s.FinishCompanionRound(ctx, p.OwnerID, round.ID, "completed", ""); err != nil {
		t.Fatal(err)
	}
	renamed := g
	renamed.Name = "Renamed group"
	renamed, err = s.SaveCompanionGroup(ctx, renamed, g.Revision)
	if err != nil || renamed.ScopeRevision != g.ScopeRevision {
		t.Fatal("name-only edit lost scope", renamed, err)
	}
	next, err := s.BeginCompanionRound(ctx, p.OwnerID, g.ID, renamed.Revision, "second group request", []string{p.ID})
	if err != nil || next.Members[0].SessionID != round.Members[0].SessionID || next.Members[0].Profile.Revision != p.Revision {
		t.Fatal("name edit lost old session or reused stale profile", next, err)
	}
	if err = s.FinishCompanionRound(ctx, p.OwnerID, next.ID, "completed", ""); err != nil {
		t.Fatal(err)
	}
	renamed.Members = []string{p.ID}
	changed, err := s.SaveCompanionGroup(ctx, renamed, renamed.Revision)
	if err != nil || changed.ScopeRevision != 2 {
		t.Fatal("membership edit retained old scope", changed, err)
	}
	fresh, err := s.BeginCompanionRound(ctx, p.OwnerID, g.ID, changed.Revision, "new scope", []string{p.ID})
	if err != nil || fresh.Members[0].SessionID == round.Members[0].SessionID {
		t.Fatal("membership scope reused old conversation", fresh, err)
	}
	if err = s.FinishCompanionRound(ctx, p.OwnerID, fresh.ID, "completed", ""); err != nil {
		t.Fatal(err)
	}
	oldMessages, err := s.CompanionMessages(ctx, p.OwnerID, g.ID, 1, 256)
	if err != nil || len(oldMessages) != 2 {
		t.Fatal("old records disappeared", oldMessages, err)
	}
	newMessages, err := s.CompanionMessages(ctx, p.OwnerID, g.ID, 2, 256)
	if err != nil || len(newMessages) != 1 || newMessages[0].Text != "new scope" {
		t.Fatal("old messages entered new scope", newMessages, err)
	}
	p.Enabled = false
	p, err = s.SaveCompanion(ctx, p, p.Revision)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.OpenCompanionConversation(ctx, p.OwnerID, p.ID, "", 0, t.TempDir()); !errors.Is(err, hub.ErrCompanionDisabled) {
		t.Fatal("disabled profile opened", err)
	}
	if _, err = s.Detail(ctx, private.SessionID); err != nil {
		t.Fatal("disable removed history", err)
	}
	if err = s.DeleteCompanion(ctx, p.OwnerID, p.ID, p.Revision); err != nil {
		t.Fatal(err)
	}
	if err = s.DeleteCompanionGroup(ctx, p.OwnerID, g.ID, changed.Revision); err != nil {
		t.Fatal(err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	closed = true
	s, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	closed = false
	profiles, err := s.Companions(ctx, p.OwnerID)
	if err != nil || len(profiles) != 1 || profiles[0].ID != q.ID {
		t.Fatal("deleted profile returned to roster", profiles, err)
	}
	history, err := s.CompanionMessages(ctx, p.OwnerID, g.ID, 1, 256)
	if err != nil || len(history) != 2 {
		t.Fatal("tombstone deleted historical facts", history, err)
	}
	if _, err = s.Detail(ctx, private.SessionID); err != nil {
		t.Fatal("reopen lost private session", err)
	}
}
func TestCompanionMigrationPreservesOrdinaryRecordsAndNeverReplaysRound(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "product")
	s, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	ordinary, err := s.CreateSession(ctx, "pre-companion history", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	for _, q := range []string{`DROP TABLE hub_conversation_tab_sets`, `DROP TABLE hub_developer_repositories`, `DROP TABLE hub_companion_messages`, `DROP TABLE hub_companion_rounds`, `DROP TABLE hub_companion_conversations`, `DROP TABLE hub_companion_groups`, `DROP TABLE hub_companions`, `DROP TABLE hub_workspace_views`, `DROP TABLE hub_composer_drafts`, `DROP TABLE hub_composer_file_sessions`, `DROP TABLE hub_composer_files`, `DROP TABLE hub_connection_creations`, `PRAGMA user_version=4`} {
		if _, err = s.db.ExecContext(ctx, q); err != nil {
			t.Fatal(err)
		}
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	if detail, err := s.Detail(ctx, ordinary.ID); err != nil || detail.Session.Title != ordinary.Title {
		t.Fatal("migration changed old session", detail, err)
	}
	p := companionProfile(t, s, "Research")
	g, err := s.SaveCompanionGroup(ctx, hub.CompanionGroup{OwnerID: p.OwnerID, Name: "Group", Members: []string{p.ID}, ReplyMode: "all", ReplyLimit: 1, Workspace: t.TempDir(), Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	r, err := s.BeginCompanionRound(ctx, p.OwnerID, g.ID, g.Revision, "never replay", nil)
	if err != nil {
		t.Fatal(err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	rounds, err := s.CompanionRounds(ctx, p.OwnerID, g.ID, g.ScopeRevision, 32)
	if err != nil || rounds[0].ID != r.ID || rounds[0].Status != "interrupted" {
		t.Fatal("reopen lost interruption", rounds, err)
	}
	detail, err := s.Detail(ctx, r.Members[0].SessionID)
	if err != nil || detail.Session.Status == hub.Running {
		t.Fatal("reopen executed an unstarted member", detail, err)
	}
}

func TestCompanionHistoryKeysetsCoverAllMessagesAndRounds(t *testing.T) {
	s := newStore(t)
	ctx := t.Context()
	p := companionProfile(t, s, "paged")
	g, err := s.SaveCompanionGroup(ctx, hub.CompanionGroup{OwnerID: p.OwnerID, Name: "history", Workspace: t.TempDir(), Members: []string{p.ID}, ReplyMode: "all", ReplyLimit: 1, Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	expectedMessages, expectedRounds := map[string]bool{}, map[string]bool{}
	for i := 0; i < 129; i++ {
		r, e := s.BeginCompanionRound(ctx, p.OwnerID, g.ID, g.Revision, fmt.Sprintf("prompt-%03d", i), nil)
		if e != nil {
			t.Fatal(e)
		}
		reply := hub.CompanionMessage{ID: fmt.Sprintf("reply-%03d", i), RoundID: r.ID, GroupID: g.ID, GroupRevision: 1, Author: p.Name, Text: fmt.Sprintf("reply-%03d", i), CompanionID: p.ID, SessionID: r.Members[0].SessionID, Status: "completed", CreatedAt: r.CreatedAt}
		if e = s.AppendCompanionMessage(ctx, p.OwnerID, reply); e != nil {
			t.Fatal(e)
		}
		if e = s.FinishCompanionRound(ctx, p.OwnerID, r.ID, "completed", ""); e != nil {
			t.Fatal(e)
		}
		// All rounds tie on created, so every continuation must use the ID key too.
		r, e = s.CompanionRound(ctx, p.OwnerID, r.ID)
		if e != nil {
			t.Fatal(e)
		}
		r.CreatedAt = 12345
		b, e := body(r)
		if e != nil {
			t.Fatal(e)
		}
		if _, e = s.db.ExecContext(ctx, `UPDATE hub_companion_rounds SET created=?,data=? WHERE id=?`, r.CreatedAt, b, r.ID); e != nil {
			t.Fatal(e)
		}
		expectedRounds[r.ID] = true
		expectedMessages[reply.ID] = true
	}
	all, err := s.db.QueryContext(ctx, `SELECT id FROM hub_companion_messages WHERE group_id=?`, g.ID)
	if err != nil {
		t.Fatal(err)
	}
	for all.Next() {
		var id string
		if err = all.Scan(&id); err != nil {
			t.Fatal(err)
		}
		expectedMessages[id] = true
	}
	if err = all.Close(); err != nil {
		t.Fatal(err)
	}
	messageIDs, roundIDs := map[string]bool{}, map[string]bool{}
	before := ""
	var oldest, newest string
	for {
		page, e := s.CompanionMessagePage(ctx, p.OwnerID, g.ID, 1, hub.CompanionHistoryQuery{Before: before, Limit: 256})
		if e != nil {
			t.Fatal(e)
		}
		if before == "" {
			if !page.HasMore || len(page.Messages) != 256 {
				t.Fatal("first window hid older messages", page)
			}
			newest = page.Messages[len(page.Messages)-1].Text
		}
		for _, m := range page.Messages {
			if messageIDs[m.ID] || !expectedMessages[m.ID] {
				t.Fatal("duplicate or foreign message", m.ID)
			}
			messageIDs[m.ID] = true
		}
		if len(page.Messages) > 0 {
			oldest = page.Messages[0].Text
		}
		if !page.HasMore {
			if page.NextBefore != "" {
				t.Fatal("terminal cursor")
			}
			break
		}
		if page.NextBefore == before {
			t.Fatal("non-advancing cursor")
		}
		before = page.NextBefore
	}
	if len(messageIDs) != 258 || oldest != "prompt-000" || newest != "reply-128" {
		t.Fatal("message endpoints or coverage lost", len(messageIDs), oldest, newest)
	}
	before = ""
	for {
		page, e := s.CompanionRoundPage(ctx, p.OwnerID, g.ID, 1, hub.CompanionHistoryQuery{Before: before, Limit: 32})
		if e != nil {
			t.Fatal(e)
		}
		for _, r := range page.Rounds {
			if roundIDs[r.ID] || !expectedRounds[r.ID] {
				t.Fatal("duplicate or foreign round", r.ID)
			}
			roundIDs[r.ID] = true
		}
		if !page.HasMore {
			break
		}
		if page.NextBefore == before {
			t.Fatal("non-advancing cursor")
		}
		before = page.NextBefore
	}
	if len(roundIDs) != 129 {
		t.Fatal("round coverage lost", len(roundIDs))
	}
	if _, err = s.CompanionMessagePage(ctx, p.OwnerID, g.ID, 1, hub.CompanionHistoryQuery{Before: before, Limit: 256}); err == nil {
		t.Fatal("round cursor accepted as message cursor")
	}
	if _, err = s.CompanionRoundPage(ctx, p.OwnerID, g.ID, 2, hub.CompanionHistoryQuery{Before: before, Limit: 32}); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("future scope accepted", err)
	}
}

func TestCompanionTombstonesRejectAllAttemptClaimsIncludingWaitResume(t *testing.T) {
	for _, kind := range []string{"private", "group"} {
		for _, waiting := range []bool{false, true} {
			t.Run(fmt.Sprintf("%s-waiting-%t", kind, waiting), func(t *testing.T) {
				s := newStore(t)
				ctx := t.Context()
				p := companionProfile(t, s, "archive")
				var c hub.CompanionConversation
				var g hub.CompanionGroup
				var err error
				if kind == "group" {
					g, err = s.SaveCompanionGroup(ctx, hub.CompanionGroup{OwnerID: p.OwnerID, Name: "archive group", Workspace: t.TempDir(), Members: []string{p.ID}, ReplyMode: "all", ReplyLimit: 1, Enabled: true}, 0)
					if err != nil {
						t.Fatal(err)
					}
					r, e := s.BeginCompanionRound(ctx, p.OwnerID, g.ID, g.Revision, "saved group prompt", nil)
					if e != nil {
						t.Fatal(e)
					}
					c = r.Members[0]
					if err = s.FinishCompanionRound(ctx, p.OwnerID, r.ID, "completed", ""); err != nil {
						t.Fatal(err)
					}
				} else {
					c, err = s.OpenCompanionConversation(ctx, p.OwnerID, p.ID, "", 0, t.TempDir())
					if err != nil {
						t.Fatal(err)
					}
				}
				var claim hub.Claim
				if waiting {
					if err = s.SetDurableWaits(ctx, c.SessionID, true); err != nil {
						t.Fatal(err)
					}
					claim, err = s.BeginTurn(ctx, c.SessionID, "original wait")
					if err != nil {
						t.Fatal(err)
					}
					recordWaiting(t, s, claim)
					if err = s.FinishTurn(ctx, claim.Turn.ID, engine.Waiting); err != nil {
						t.Fatal(err)
					}
				}
				if kind == "group" {
					err = s.DeleteCompanionGroup(ctx, p.OwnerID, g.ID, g.Revision)
				} else {
					err = s.DeleteCompanion(ctx, p.OwnerID, p.ID, p.Revision)
				}
				if err != nil {
					t.Fatal(err)
				}
				before, err := s.Detail(ctx, c.SessionID)
				if err != nil {
					t.Fatal(err)
				}
				if waiting {
					_, err = s.BeginWaitResume(ctx, c.SessionID, claim.Branch.ID, claim.Turn.ID, waitingReceipt().Revision+1)
				} else {
					_, err = s.BeginTurn(ctx, c.SessionID, "must remain read-only")
				}
				if !errors.Is(err, hub.ErrCompanionDisabled) {
					t.Fatal("tombstone admitted an Attempt", err)
				}
				after, err := s.Detail(ctx, c.SessionID)
				if err != nil || !reflect.DeepEqual(before, after) {
					t.Fatal("rejected claim changed historical identity", err)
				}
				ordinary, err := s.CreateSession(ctx, "ordinary", t.TempDir(), target("a"))
				if err != nil {
					t.Fatal(err)
				}
				if _, err = s.BeginTurn(ctx, ordinary.ID, "ordinary unaffected"); err != nil {
					t.Fatal("ordinary session rejected", err)
				}
			})
		}
	}
}

func TestCompanionTombstoneDuringOwnerFailureBlocksAuthorizedFallback(t *testing.T) {
	s := newStore(t)
	p := companionProfile(t, s, "delete during actual owner")
	c, err := s.OpenCompanionConversation(t.Context(), p.OwnerID, p.ID, "", 0, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	fallback := target("external-fixture")
	fallback.AutomaticAuthorized = true
	if err = s.SetFallback(t.Context(), c.SessionID, []hub.Target{fallback}); err != nil {
		t.Fatal(err)
	}
	runtime := hub.New(s)
	defer runtime.Close(context.Background())
	if err = runtime.Register("a", runtimeFunc{run: func(ctx context.Context, _ engine.Request, emit func(engine.Event) error) error {
		if err := s.DeleteCompanion(ctx, p.OwnerID, p.ID, p.Revision); err != nil {
			return err
		}
		if err := emit(engine.Event{Kind: engine.Error, Text: "local fixture startup failure", Failure: engine.FailureStartup}); err != nil {
			return err
		}
		return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Failed})
	}}); err != nil {
		t.Fatal(err)
	}
	calls := 0
	if err = runtime.Register("external-fixture", runtimeFunc{run: func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
		calls++
		return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
	}}); err != nil {
		t.Fatal(err)
	}
	if err = runtime.Run(t.Context(), c.SessionID, "original admitted owner"); err == nil {
		t.Fatal("deleted companion fallback succeeded")
	}
	d, err := s.Detail(t.Context(), c.SessionID)
	if err != nil || calls != 0 || len(d.Branches) != 1 || d.Branches[0].Target.EngineID != "a" || runtime.HasActive() {
		t.Fatal("tombstone fallback executed or left phantom owner/branch", calls, d, err)
	}
}

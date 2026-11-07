// INPUT: Temporary product/native stores, actual local fixture sends, deleted identities and bounded history pages.
// OUTPUT: Exact-Turn publication and reopened discoverable private/group history with zero new execution.
// POS: [L7 product tests] Local fixtures only; deleted history never restores account or Attempt admission.
package product

import (
	"context"
	"errors"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	productstore "github.com/QuincySx/Alva/apps/hub/sqlite"
	"github.com/QuincySx/Alva/integration/engine"
)

type companionPagedReplyStore struct {
	ServiceStore
	calls   int
	missing bool
}

func (s *companionPagedReplyStore) HistoryBefore(context.Context, string, uint64, int) ([]hub.HistoryEntry, error) {
	s.calls++
	if s.calls == 1 {
		out := make([]hub.HistoryEntry, 128)
		for i := range out {
			out[i] = hub.HistoryEntry{Sequence: uint64(129 + i), TurnID: "newer-turn", AttemptID: "newer-attempt", Event: engine.Event{Kind: engine.MessageDelta, ID: "new", Text: "later message"}}
		}
		return out, nil
	}
	if s.missing {
		return nil, nil
	}
	return []hub.HistoryEntry{{Sequence: 128, TurnID: "original-turn", AttemptID: "original-attempt", Event: engine.Event{Kind: engine.Message, ID: "original", Text: "original group reply"}}}, nil
}

func TestCompanionPublicationFindsOriginalTurnBehindNewerHistory(t *testing.T) {
	for _, missing := range []bool{false, true} {
		t.Run(map[bool]string{false: "original-found", true: "missing-rejected"}[missing], func(t *testing.T) {
			s, requests := companionServiceFixture(t)
			store := &companionPagedReplyStore{ServiceStore: s.Store, missing: missing}
			s.Store = store
			text, err := s.companionTurnText(t.Context(), "member-session", "original-turn")
			if missing {
				if err == nil {
					t.Fatal("missing original Turn looked like a successfully empty reply")
				}
			} else if err != nil || text != "original group reply" || store.calls < 2 {
				t.Fatal("newer Turn hid original group reply", text, store.calls, err)
			}
			if len(requests) != 0 {
				t.Fatal("history read ran a model")
			}
		})
	}
}

func TestCompanionDeletedHistoryReopensWithoutRestoringExecution(t *testing.T) {
	s, requests := companionServiceFixture(t)
	ctx := t.Context()
	p := saveCompanion(t, s, "deleted companion")
	private, err := s.OpenCompanion(ctx, p.OwnerID, p.ID, true)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.SubmitCompanion(ctx, p.OwnerID, p.ID, "private archived fact", true); err != nil {
		t.Fatal(err)
	}
	waitIdle(t, s, private.SessionID)
	<-requests
	g, err := s.SaveCompanionGroup(ctx, hub.CompanionGroup{OwnerID: p.OwnerID, Name: "deleted group", Members: []string{p.ID}, ReplyMode: "all", ReplyLimit: 1, Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	round, err := s.RunCompanionGroup(ctx, p.OwnerID, g.ID, g.Revision, "group archived fact", nil, true)
	if err != nil {
		t.Fatal(err)
	}
	<-requests
	if err = s.Companions.Delete(ctx, p.OwnerID, p.ID, p.Revision); err != nil {
		t.Fatal(err)
	}
	if err = s.Companions.DeleteGroup(ctx, p.OwnerID, g.ID, g.Revision); err != nil {
		t.Fatal(err)
	}
	descriptor, ok := s.Runtime.Descriptor("alva")
	if !ok {
		t.Fatal("missing local fixture engine")
	}
	if err = s.Companions.Close(ctx); err != nil {
		t.Fatal(err)
	}
	if err = s.Runtime.Close(ctx); err != nil {
		t.Fatal(err)
	}
	if err = s.Teams.Close(ctx); err != nil {
		t.Fatal(err)
	}
	if err = s.Store.(*productstore.Store).Close(); err != nil {
		t.Fatal(err)
	}
	reopened, err := productstore.Open(ctx, filepath.Join(s.DataDirectory, "product"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = reopened.Close() })
	runtime := hub.New(reopened)
	t.Cleanup(func() { _ = runtime.Close(context.Background()) })
	if err = runtime.RegisterDescriptor("alva", descriptor); err != nil {
		t.Fatal(err)
	}
	cold, err := NewHubService(reopened, runtime, s.Repository, s.DefaultTarget, s.DataDirectory, true)
	if err != nil {
		t.Fatal(err)
	}
	cold.Companions, err = NewCompanionService(reopened)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = cold.Companions.Close(context.Background()); _ = cold.Teams.Close(context.Background()) })
	archive, err := cold.Companions.Archive(ctx, p.OwnerID)
	if err != nil || len(archive.Profiles) != 1 || len(archive.Groups) != 1 || archive.Profiles[0].ID != p.ID || archive.Groups[0].ID != g.ID {
		t.Fatal("cold archive not discoverable", archive, err)
	}
	if profiles, e := cold.Companions.List(ctx, p.OwnerID); e != nil || len(profiles) != 0 {
		t.Fatal("deleted identity restored", profiles, e)
	}
	conversation, err := cold.Companions.Conversation(ctx, p.OwnerID, archive.Profiles[0].ID, "", 0)
	if err != nil || conversation.SessionID != private.SessionID {
		t.Fatal("private archive binding lost", conversation, err)
	}
	history, err := cold.Store.HistoryBefore(ctx, conversation.SessionID, 0, 128)
	if err != nil || len(history) == 0 {
		t.Fatal("private actual facts missing", history, err)
	}
	messages, err := cold.Companions.MessagePage(ctx, p.OwnerID, archive.Groups[0].ID, 1, hub.CompanionHistoryQuery{Limit: 256})
	if err != nil || len(messages.Messages) != 2 || messages.Messages[0].Text != "group archived fact" || messages.Messages[1].Text == "" {
		t.Fatal("group facts missing", messages, err)
	}
	rounds, err := cold.Companions.RoundPage(ctx, p.OwnerID, g.ID, 1, hub.CompanionHistoryQuery{Limit: 32})
	if err != nil || len(rounds.Rounds) != 1 || rounds.Rounds[0].ID != round.ID {
		t.Fatal("round binding lost", rounds, err)
	}
	for _, id := range []string{private.SessionID, round.Members[0].SessionID} {
		before, e := cold.Store.Detail(ctx, id)
		if e != nil {
			t.Fatal(e)
		}
		if _, e = cold.Submit(ctx, id, "ordinary bypass", nil); !errors.Is(e, hub.ErrCompanionDisabled) {
			t.Fatal("archive Submit accepted", e)
		}
		if _, _, e = cold.Runtime.Start(ctx, id, "runtime bypass"); !errors.Is(e, hub.ErrCompanionDisabled) {
			t.Fatal("archive Runtime accepted", e)
		}
		after, e := cold.Store.Detail(ctx, id)
		if e != nil || !reflect.DeepEqual(before, after) {
			t.Fatal("archive claim changed facts", e)
		}
	}
	if _, err = cold.SubmitCompanion(ctx, p.OwnerID, p.ID, "companion bypass", true); !errors.Is(err, hub.ErrCompanionDisabled) {
		t.Fatal(err)
	}
	if _, err = cold.RunCompanionGroup(ctx, p.OwnerID, g.ID, archive.Groups[0].Revision, "group bypass", nil, true); !errors.Is(err, hub.ErrCompanionDisabled) {
		t.Fatal(err)
	}
	if len(requests) != 0 || runtime.HasActive() {
		t.Fatal("archive browsing/denied admission executed a model")
	}
}

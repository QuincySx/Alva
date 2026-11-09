// INPUT: Synthetic session snapshots, a real product service and injected foreground/notification sinks
// OUTPUT: Which transitions notify, that nothing notifies in the foreground or on first sight, and the click-to-session mapping
// POS: [L7 Hub UI testing] No OS notification is shown; the system sink and window focus are replaced by fakes
//go:build desktop

package hubdesktop

import (
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func snapshot(id string, status engine.Outcome) []hub.Session {
	return []hub.Session{{ID: id, Title: "任务 " + id, Status: status}}
}

func succeedTurn(t *testing.T, store hub.ExecutionStore, claim hub.Claim, native string) {
	t.Helper()
	ctx := t.Context()
	if err := store.Record(ctx, claim.Attempt.ID, engine.Event{Kind: engine.SessionStarted, SessionID: native}, false); err != nil {
		t.Fatal(err)
	}
	if err := store.Record(ctx, claim.Attempt.ID, engine.Event{Kind: engine.Completed, SessionID: native, Outcome: engine.Succeeded}, false); err != nil {
		t.Fatal(err)
	}
	if err := store.FinishTurn(ctx, claim.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
}

func TestAttentionTrackerNotifiesOnlyRealTransitions(t *testing.T) {
	tracker := newAttentionTracker()
	if got := tracker.observe(snapshot("a", hub.Running), nil); len(got) != 0 {
		t.Fatal("the first observation must only prime the tracker", got)
	}
	if got := tracker.observe(snapshot("a", hub.Running), nil); len(got) != 0 {
		t.Fatal("a steady running session is not news", got)
	}
	got := tracker.observe(snapshot("a", engine.Succeeded), nil)
	if len(got) != 1 || got[0].Session != "a" || got[0].Body != "回复已完成" || got[0].Title != "Alva" {
		t.Fatal("a finished reply must notify once", got)
	}
	if got = tracker.observe(snapshot("a", engine.Succeeded), nil); len(got) != 0 {
		t.Fatal("a finished session must not notify again", got)
	}
	// A session first seen already finished is history, not news.
	if got = tracker.observe(snapshot("b", engine.Succeeded), nil); len(got) != 0 {
		t.Fatal("a session first seen as finished notified", got)
	}
	// Stopped by the user is not an announcement.
	tracker.observe(snapshot("c", hub.Running), nil)
	if got = tracker.observe(snapshot("c", engine.Cancelled), nil); len(got) != 0 {
		t.Fatal("a user stop notified", got)
	}
	tracker.observe(snapshot("d", hub.Running), nil)
	if got = tracker.observe(snapshot("d", engine.Failed), nil); len(got) != 1 || got[0].Body != "任务失败，需要查看" {
		t.Fatal("a failure must notify", got)
	}
	tracker.observe(snapshot("e", hub.Running), nil)
	if got = tracker.observe(snapshot("e", engine.Waiting), nil); len(got) != 1 || got[0].Body != "任务在等待你的处理" {
		t.Fatal("a parked wait must notify", got)
	}
}

func TestAttentionTrackerAnnouncesEachPendingRequestOnce(t *testing.T) {
	tracker := newAttentionTracker()
	sessions := snapshot("a", hub.Running)
	tracker.observe(sessions, nil)
	got := tracker.observe(sessions, map[string]pendingRequests{"a": {Approvals: []string{"r1"}}})
	if len(got) != 1 || got[0].Body != "需要你的审批" {
		t.Fatal("a new approval must notify", got)
	}
	if got = tracker.observe(sessions, map[string]pendingRequests{"a": {Approvals: []string{"r1"}}}); len(got) != 0 {
		t.Fatal("the same pending approval was announced twice", got)
	}
	got = tracker.observe(sessions, map[string]pendingRequests{"a": {Approvals: []string{"r1", "r2"}, Questions: []string{"q1"}}})
	if len(got) != 2 {
		t.Fatal("only the new approval and the question are news", got)
	}
	for _, event := range got {
		if event.Body != "需要你的审批" && event.Body != "需要你的回答" {
			t.Fatal("unexpected body", event)
		}
	}
	// Once answered the ID leaves the set; a later request with a new ID is news again.
	tracker.observe(sessions, nil)
	if got = tracker.observe(sessions, map[string]pendingRequests{"a": {Approvals: []string{"r3"}}}); len(got) != 1 {
		t.Fatal("a later approval was lost", got)
	}
}

func TestAttentionNotifiesOnlyBackgroundedAndClickMapsToTheSession(t *testing.T) {
	service, _ := fixture(t)
	session, err := service.Create(t.Context(), "background", "")
	if err != nil {
		t.Fatal(err)
	}
	var shown []attention
	foreground := false
	observer := newAttentionObserver(service)
	observer.foreground = func() bool { return foreground }
	observer.show = func(a attention) { shown = append(shown, a) }
	observer.poll(t.Context()) // primes on a finished session
	claim, err := service.Store.BeginTurn(t.Context(), session.ID, "work")
	if err != nil {
		t.Fatal(err)
	}
	observer.poll(t.Context()) // now running
	succeedTurn(t, service.Store, claim, "native-1")
	foreground = true
	observer.poll(t.Context())
	if len(shown) != 0 {
		t.Fatal("a foreground window must not be notified", shown)
	}
	// Same transition while backgrounded.
	claim, err = service.Store.BeginTurn(t.Context(), session.ID, "more work")
	if err != nil {
		t.Fatal(err)
	}
	observer.poll(t.Context())
	succeedTurn(t, service.Store, claim, "native-1")
	foreground = false
	observer.poll(t.Context())
	if len(shown) != 1 || attentionID(shown[0].Session) != "session:"+session.ID {
		t.Fatal("a backgrounded completion must notify with the session identity", shown)
	}
	if id, ok := attentionSession(attentionID(session.ID)); !ok || id != session.ID {
		t.Fatal("click identity does not round-trip")
	}
	for _, bad := range []string{"", "session:", "other:abc", "abc"} {
		if _, ok := attentionSession(bad); ok {
			t.Fatalf("%q is not a session notification", bad)
		}
	}
}

func TestAttentionTrackerForgetsSessionsThatLeftThePage(t *testing.T) {
	tracker := newAttentionTracker()
	tracker.observe(snapshot("a", hub.Running), nil)
	tracker.observe(snapshot("b", hub.Running), nil)
	if len(tracker.status) != 1 {
		t.Fatal("sessions that left the page stay in the tracker", tracker.status)
	}
}

func TestPinnedSessionAppearsOnceAndRowKeysAreUnique(t *testing.T) {
	service, _ := fixture(t)
	p := NewNativePage(service)
	both := hub.Session{ID: "hub-both", Title: "both", Pinned: true}
	p.sessions = []hub.Session{both}
	p.sessionOps.pinned = []hub.Session{both}
	seen := map[string]bool{}
	var shown int
	for _, row := range p.workspaceNavigationRows() {
		if seen[row.key] {
			t.Fatal("duplicate row key", row.key)
		}
		seen[row.key] = true
		if row.session.ID == both.ID {
			shown++
		}
	}
	if shown != 1 {
		t.Fatal("a pinned session must not also repeat in the recent list", shown)
	}
}

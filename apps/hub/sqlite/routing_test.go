// INPUT: testing, actual private SQLite, hub.Runtime and deterministic engine fixtures
// OUTPUT: regressions for engine-keyed branches, per-turn fallback, route revalidation, long errors and FinishTurn recovery
// POS: [L7 Hub product] Routing and connection facts are resolved at execution; one failure never strands a session
package sqlite

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"sync"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/redaction"
)

func okEngine(calls *[]engine.Request, native string) runtimeFunc {
	var mu sync.Mutex
	return runtimeFunc{control: true, run: func(_ context.Context, req engine.Request, emit func(engine.Event) error) error {
		mu.Lock()
		*calls = append(*calls, req)
		mu.Unlock()
		if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: native}); err != nil {
			return err
		}
		return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
	}}
}

func failStartup(calls *[]engine.Request, native string) runtimeFunc {
	return runtimeFunc{control: true, run: func(_ context.Context, req engine.Request, emit func(engine.Event) error) error {
		*calls = append(*calls, req)
		if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: native}); err != nil {
			return err
		}
		if err := emit(engine.Event{Kind: engine.Error, Text: "startup fixture", Failure: engine.FailureStartup}); err != nil {
			return err
		}
		return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Failed})
	}}
}

func timelineKinds(t *testing.T, s *Store, id string) []string {
	t.Helper()
	history, err := s.History(context.Background(), id, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	var kinds []string
	for _, entry := range history {
		if entry.Timeline != nil {
			kinds = append(kinds, entry.Timeline.Kind)
		}
	}
	return kinds
}

// Decision 2: an automatic fallback serves one Turn; the next Turn tries the
// primary route again and the session's active branch never moved.
func TestFallbackAppliesToOneTurnOnly(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, err := s.CreateSession(ctx, "fixture", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	approved := target("b")
	approved.AutomaticAuthorized = true
	if err = s.SetFallback(ctx, session.ID, []hub.Target{approved}); err != nil {
		t.Fatal(err)
	}
	var callsA, callsB []engine.Request
	failA := true
	runtime := hub.New(s)
	_ = runtime.Register("a", runtimeFunc{control: true, run: func(c context.Context, req engine.Request, emit func(engine.Event) error) error {
		if failA {
			return failStartup(&callsA, "native-a").run(c, req, emit)
		}
		return okEngine(&callsA, "native-a").run(c, req, emit)
	}})
	_ = runtime.Register("b", okEngine(&callsB, "native-b"))
	if err = runtime.Run(ctx, session.ID, "turn 1"); err != nil {
		t.Fatal(err)
	}
	detail, _ := s.Detail(ctx, session.ID)
	active, err := branchOf(detail, detail.Session.ActiveBranchID)
	if err != nil || active.Target.EngineID != "a" || len(callsB) != 1 {
		t.Fatal("fallback moved the session's route", active, err, len(callsB))
	}
	failA = false
	if err = runtime.Run(ctx, session.ID, "turn 2"); err != nil {
		t.Fatal(err)
	}
	if len(callsA) != 2 || len(callsB) != 1 {
		t.Fatal("the next Turn did not return to the primary route", len(callsA), len(callsB))
	}
}

func branchOf(detail hub.Detail, id string) (hub.Branch, error) {
	for _, b := range detail.Branches {
		if b.ID == id {
			return b, nil
		}
	}
	return hub.Branch{}, errors.New("branch missing")
}

// Decision 1: a model change inside one engine keeps the branch, its native
// conversation and its watermark; nothing is replayed through a handoff.
func TestSameEngineModelSwitchKeepsBranchAndNativeSession(t *testing.T) {
	for _, legacy := range []bool{false, true} {
		s := newStore(t)
		ctx := context.Background()
		m1 := target("alva")
		session, err := s.CreateSession(ctx, "fixture", t.TempDir(), m1)
		if err != nil {
			t.Fatal(err)
		}
		if legacy {
			// Rows saved by an older build keyed the branch by the whole route.
			old, _ := json.Marshal(m1)
			if _, err = s.db.ExecContext(ctx, `UPDATE hub_branches SET route=? WHERE session=?`, string(old), session.ID); err != nil {
				t.Fatal(err)
			}
		}
		first, err := s.BeginTurn(ctx, session.ID, "remember PLUM")
		if err != nil {
			t.Fatal(err)
		}
		complete(t, s, first, "native-1")
		m2 := m1
		m2.ModelID = "fixture-2"
		branch, err := s.Switch(ctx, session.ID, m2, "pick another model")
		if err != nil {
			t.Fatal(err)
		}
		if branch.ID != first.Branch.ID || branch.NativeSessionID != "native-1" || branch.Target.ModelID != "fixture-2" {
			t.Fatal("same-engine switch did not keep the native branch", legacy, branch)
		}
		next, err := s.BeginTurn(ctx, session.ID, "what was the word?")
		if err != nil {
			t.Fatal(err)
		}
		if next.Branch.ID != first.Branch.ID || next.Attempt.Target.ModelID != "fixture-2" || next.Branch.NativeSessionID != "native-1" || next.Handoff != nil && len(next.Handoff.Entries) != 0 {
			t.Fatal("next Turn lost context or used the old model", legacy, next.Attempt.Target, next.Handoff)
		}
		detail, _ := s.Detail(ctx, session.ID)
		if len(detail.Branches) != 1 {
			t.Fatal("same-engine switch created another branch", len(detail.Branches))
		}
	}
}

type fakeResolver struct {
	mu    sync.Mutex
	facts map[string]hub.RouteFacts // by model ID
	bad   map[string]error
}

func (f *fakeResolver) ResolveRoute(_ context.Context, target hub.Target) (hub.RouteFacts, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if err := f.bad[target.ModelID]; err != nil {
		return hub.RouteFacts{}, err
	}
	if facts, ok := f.facts[target.ModelID]; ok {
		return facts, nil
	}
	return hub.RouteFacts{Target: target}, nil
}

// Decision 4/5: a stale fallback is skipped with a visible reason and the chain
// goes on; the session is never moved to the stale route.
func TestStaleFallbackIsSkippedVisiblyAndChainContinues(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, err := s.CreateSession(ctx, "fixture", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	stale, good := target("b"), target("c")
	stale.AutomaticAuthorized, good.AutomaticAuthorized = true, true
	if err = s.SetFallback(ctx, session.ID, []hub.Target{stale, good}); err != nil {
		t.Fatal(err)
	}
	var callsA, callsB, callsC []engine.Request
	runtime := hub.New(s)
	runtime.SetRouteResolver(&fakeResolver{bad: map[string]error{"stale-model": errors.New("connection disabled")}})
	staleTarget := stale
	staleTarget.ModelID = "stale-model"
	// Re-save the fallback list with distinct model IDs so the fake can tell them apart.
	stale.ModelID = "stale-model"
	if err = s.SetFallback(ctx, session.ID, []hub.Target{stale, good}); err != nil {
		t.Fatal(err)
	}
	_ = runtime.Register("a", failStartup(&callsA, "native-a"))
	_ = runtime.Register("b", okEngine(&callsB, "native-b"))
	_ = runtime.Register("c", okEngine(&callsC, "native-c"))
	if err = runtime.Run(ctx, session.ID, "turn"); err != nil {
		t.Fatal(err)
	}
	if len(callsB) != 0 || len(callsC) != 1 {
		t.Fatal("stale fallback ran or broke the chain", len(callsB), len(callsC))
	}
	found := false
	history, _ := s.History(ctx, session.ID, 0, 128)
	for _, e := range history {
		if e.Timeline != nil && e.Timeline.Kind == "fallback_unavailable" && strings.Contains(e.Timeline.Text, "connection disabled") {
			found = true
		}
	}
	if !found {
		t.Fatal("skipped fallback was not reported to the user", timelineKinds(t, s, session.ID))
	}
	detail, _ := s.Detail(ctx, session.ID)
	if active, _ := branchOf(detail, detail.Session.ActiveBranchID); active.Target.EngineID != "a" {
		t.Fatal("session moved onto a fallback branch")
	}
}

// A failed BeginFallback is reported too, instead of silently skipped.
func TestFailedFallbackSwitchIsReported(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, err := s.CreateSession(ctx, "fixture", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	b := target("b")
	b.AutomaticAuthorized = true
	if err = s.SetFallback(ctx, session.ID, []hub.Target{b}); err != nil {
		t.Fatal(err)
	}
	var callsA, callsB []engine.Request
	runtime := hub.New(failBeginFallback{s})
	_ = runtime.Register("a", failStartup(&callsA, "native-a"))
	_ = runtime.Register("b", okEngine(&callsB, "native-b"))
	_ = runtime.Run(ctx, session.ID, "turn")
	if len(callsB) != 0 || !contains(timelineKinds(t, s, session.ID), "fallback_unavailable") {
		t.Fatal("BeginFallback failure was silent", len(callsB), timelineKinds(t, s, session.ID))
	}
}

type failBeginFallback struct{ *Store }

func (failBeginFallback) BeginFallback(context.Context, string, hub.Target) (hub.Claim, error) {
	return hub.Claim{}, errors.New("fixture switch failure")
}
func contains(list []string, want string) bool {
	for _, v := range list {
		if v == want {
			return true
		}
	}
	return false
}

// A moved endpoint is followed: the engine receives the new endpoint, the audit
// event records revision and endpoint, and the stream shows a host-only notice.
func TestEndpointMoveIsFollowedAndAnnounced(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	route := target("alva")
	route.AccountSource, route.ProviderID, route.BillingRoute = "connection:c1", "openai_chat", "https://old.example.invalid/v1/chat"
	session, err := s.CreateSession(ctx, "fixture", t.TempDir(), route)
	if err != nil {
		t.Fatal(err)
	}
	var calls []engine.Request
	resolver := &fakeResolver{facts: map[string]hub.RouteFacts{}}
	set := func(rev uint64, endpoint string) {
		fresh := route
		fresh.BillingRoute = endpoint
		resolver.mu.Lock()
		resolver.facts[route.ModelID] = hub.RouteFacts{Target: fresh, Revision: rev, Endpoint: endpoint}
		resolver.mu.Unlock()
	}
	runtime := hub.New(s)
	runtime.SetRouteResolver(resolver)
	_ = runtime.Register("alva", okEngine(&calls, "native-alva"))
	set(1, "https://old.example.invalid/v1/chat")
	if err = runtime.Run(ctx, session.ID, "one"); err != nil {
		t.Fatal(err)
	}
	set(3, "https://new.example.invalid:8443/v1/chat?token=x")
	if err = runtime.Run(ctx, session.ID, "two"); err != nil {
		t.Fatal(err)
	}
	if len(calls) != 2 || calls[1].BillingRoute != "https://new.example.invalid:8443/v1/chat?token=x" {
		t.Fatal("engine did not receive the newest endpoint", calls)
	}
	history, _ := s.History(ctx, session.ID, 0, 128)
	var resolved []hub.TimelineEvent
	notice := ""
	for _, e := range history {
		if e.Timeline == nil {
			continue
		}
		switch e.Timeline.Kind {
		case "route_resolved":
			resolved = append(resolved, *e.Timeline)
		case "route_endpoint_changed":
			notice = e.Timeline.Text
		}
	}
	if len(resolved) != 2 || resolved[0].Revision != 1 || resolved[1].Revision != 3 || !strings.Contains(resolved[1].Endpoint, "new.example.invalid") {
		t.Fatal("audit did not record the revision and endpoint actually used", resolved)
	}
	if notice != "连接地址已更新为 new.example.invalid" || strings.Contains(notice, "token") {
		t.Fatal("endpoint notice must show the host only", notice)
	}
}

type finishFailStore struct {
	*Store
	mu    sync.Mutex
	fails int
}

func (s *finishFailStore) FinishTurn(ctx context.Context, id string, status engine.Outcome) error {
	s.mu.Lock()
	if s.fails > 0 {
		s.fails--
		s.mu.Unlock()
		return errors.New("fixture FinishTurn unavailable")
	}
	s.mu.Unlock()
	return s.Store.FinishTurn(ctx, id, status)
}

// Decision 7: a failed FinishTurn never strands the session in running.
func TestFinishTurnFailureDoesNotStrandSession(t *testing.T) {
	for name, fails := range map[string]int{"retry marks interrupted": 1, "next message recovers": 2} {
		t.Run(name, func(t *testing.T) {
			s := newStore(t)
			ctx := context.Background()
			session, err := s.CreateSession(ctx, "fixture", t.TempDir(), target("a"))
			if err != nil {
				t.Fatal(err)
			}
			store := &finishFailStore{Store: s, fails: fails}
			var calls []engine.Request
			runtime := hub.New(store)
			_ = runtime.Register("a", okEngine(&calls, "native-a"))
			if err = runtime.Run(ctx, session.ID, "work"); err == nil {
				t.Fatal("a failed FinishTurn must be reported")
			}
			if fails == 1 {
				if detail, _ := s.Detail(ctx, session.ID); hub.BlocksInput(detail.Session.Status) {
					t.Fatal("session is still claimed after the interrupted retry", detail.Session.Status)
				}
			}
			if err = runtime.Run(ctx, session.ID, "again"); err != nil {
				t.Fatal("the user could not continue without a restart", err)
			}
			if len(calls) != 2 {
				t.Fatal("follow-up message was not delivered", len(calls))
			}
		})
	}
}

// Decision 6: an overlong engine error keeps its prefix and says it was cut.
func TestLongEngineErrorKeepsPrefix(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, err := s.CreateSession(ctx, "fixture", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	runtime := hub.New(s)
	_ = runtime.Register("a", runtimeFunc{control: true, run: func(context.Context, engine.Request, func(engine.Event) error) error {
		return errors.New("PREFIX-MARKER " + strings.Repeat("x", 10000))
	}})
	runErr := runtime.Run(ctx, session.ID, "work")
	history, _ := s.History(ctx, session.ID, 0, 128)
	for _, e := range history {
		if e.Event.Kind == engine.Error {
			if !strings.HasPrefix(e.Event.Text, "PREFIX-MARKER") || len(e.Event.Text) > 4096+32 || !strings.Contains(e.Event.Text, "已截断") {
				t.Fatal("long error was not kept as a bounded prefix", len(e.Event.Text))
			}
			return
		}
	}
	t.Fatal("no error event recorded", len(history), runErr != nil)
}

func endpointNotices(t *testing.T, s *Store, id string) []string {
	t.Helper()
	history, err := s.History(context.Background(), id, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	var out []string
	for _, e := range history {
		if e.Timeline != nil && e.Timeline.Kind == "route_endpoint_changed" {
			out = append(out, e.Timeline.Text)
		}
	}
	return out
}

func connectionRoute(id, endpoint string) hub.Target {
	t := target("alva")
	t.ModelID, t.AccountSource, t.ProviderID, t.BillingRoute = "m", "connection:"+id, "openai_chat", endpoint
	return t
}

// One branch carries several routes of its engine; a notice needs the SAME
// route's endpoint to change, never a different route being used.
func TestEndpointNoticeIsPerRoute(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	primary, backup := connectionRoute("c1", "https://one.example.invalid/v1"), connectionRoute("c2", "https://two.example.invalid/v1")
	session, err := s.CreateSession(ctx, "fixture", t.TempDir(), primary)
	if err != nil {
		t.Fatal(err)
	}
	approved := backup
	approved.AutomaticAuthorized = true
	if err = s.SetFallback(ctx, session.ID, []hub.Target{approved}); err != nil {
		t.Fatal(err)
	}
	endpoints := map[string]string{"connection:c1": primary.BillingRoute, "connection:c2": backup.BillingRoute}
	var mu sync.Mutex
	resolver := resolverFunc(func(tg hub.Target) (hub.RouteFacts, error) {
		mu.Lock()
		defer mu.Unlock()
		fresh := tg
		fresh.BillingRoute = endpoints[tg.AccountSource]
		return hub.RouteFacts{Target: fresh, Revision: 1, Endpoint: fresh.BillingRoute}, nil
	})
	failPrimary := true
	var calls []engine.Request
	runtime := hub.New(s)
	runtime.SetRouteResolver(resolver)
	_ = runtime.Register("alva", runtimeFunc{control: true, run: func(c context.Context, req engine.Request, emit func(engine.Event) error) error {
		if failPrimary && req.AccountSource == "connection:c1" {
			return failStartup(&calls, "native-alva").run(c, req, emit)
		}
		return okEngine(&calls, "native-alva").run(c, req, emit)
	}})
	run := func(prompt string) {
		t.Helper()
		if err := runtime.Run(ctx, session.ID, prompt); err != nil {
			t.Fatal(err)
		}
	}
	run("one") // primary fails, fallback to the other connection serves the Turn
	failPrimary = false
	run("two") // back on the primary
	if n := endpointNotices(t, s, session.ID); len(n) != 0 {
		t.Fatal("a fallback round trip announced an endpoint move", n)
	}
	if _, err = s.Switch(ctx, session.ID, backup, "manual connection change"); err != nil {
		t.Fatal(err)
	}
	run("three")
	if n := endpointNotices(t, s, session.ID); len(n) != 0 {
		t.Fatal("switching connection announced an endpoint move", n)
	}
	mu.Lock()
	endpoints["connection:c2"] = "https://moved.example.invalid/v1"
	mu.Unlock()
	run("four")
	if n := endpointNotices(t, s, session.ID); len(n) != 1 || n[0] != "连接地址已更新为 moved.example.invalid" {
		t.Fatal("the same connection moving its endpoint was not announced once", n)
	}
}

type resolverFunc func(hub.Target) (hub.RouteFacts, error)

func (f resolverFunc) ResolveRoute(_ context.Context, t hub.Target) (hub.RouteFacts, error) {
	return f(t)
}

// A branch saved before per-route memory has none; its configured endpoint is
// the baseline, so the first run after upgrade still notices an earlier move.
func TestEndpointNoticeUsesConfiguredEndpointAsLegacyBaseline(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	route := connectionRoute("c1", "https://old.example.invalid/v1")
	session, err := s.CreateSession(ctx, "fixture", t.TempDir(), route)
	if err != nil {
		t.Fatal(err)
	}
	var calls []engine.Request
	runtime := hub.New(s)
	runtime.SetRouteResolver(resolverFunc(func(tg hub.Target) (hub.RouteFacts, error) {
		fresh := tg
		fresh.BillingRoute = "https://new.example.invalid/v1"
		return hub.RouteFacts{Target: fresh, Revision: 2, Endpoint: fresh.BillingRoute}, nil
	}))
	_ = runtime.Register("alva", okEngine(&calls, "native-alva"))
	if err = runtime.Run(ctx, session.ID, "first run after upgrade"); err != nil {
		t.Fatal(err)
	}
	if n := endpointNotices(t, s, session.ID); len(n) != 1 || n[0] != "连接地址已更新为 new.example.invalid" {
		t.Fatal("an endpoint moved before the first recorded run was not announced", n)
	}
}

// The kept prefix reaches the message stream, so credentials are redacted first.
func TestLongEngineErrorIsRedactedBeforeItsPrefixIsKept(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, err := s.CreateSession(ctx, "fixture", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	redactor, err := redaction.NewRedactor("sk-literal-secret-value")
	if err != nil {
		t.Fatal(err)
	}
	runtime := hub.New(s)
	runtime.SetErrorRedactor(redactor.Text)
	_ = runtime.Register("a", runtimeFunc{control: true, run: func(context.Context, engine.Request, func(engine.Event) error) error {
		return errors.New("upstream said no: sk-literal-secret-value and Authorization: Bearer abcdef0123456789token " + strings.Repeat("x", 6000))
	}})
	_ = runtime.Run(ctx, session.ID, "work")
	history, _ := s.History(ctx, session.ID, 0, 128)
	for _, e := range history {
		if e.Event.Kind == engine.Error {
			if strings.Contains(e.Event.Text, "sk-literal-secret-value") || strings.Contains(e.Event.Text, "abcdef0123456789token") {
				t.Fatal("a credential reached the kept error prefix", e.Event.Text)
			}
			return
		}
	}
	t.Fatal("no error event recorded")
}

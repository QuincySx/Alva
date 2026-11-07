// INPUT: Real product stores and a local engine with separate pending permission/question channels
// OUTPUT: Opaque ownership, native-accepted suppression, retryable failure and late-owner regressions
// POS: [L7 product interaction tests] No external process, account or model is executed
package product

import (
	"context"
	"errors"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/integration/engine"
)

type interactionFixtureEngine struct {
	ready          chan struct{}
	readyOnce      sync.Once
	response       chan string
	permissions    atomic.Int32
	questions      atomic.Int32
	holdResolved   <-chan struct{}
	rejectResponse atomic.Bool
}

func (e *interactionFixtureEngine) Execute(ctx context.Context, _ engine.Request, emit func(engine.Event) error) error {
	for _, event := range []engine.Event{
		{Kind: engine.SessionStarted, SessionID: "interaction-native"},
		{Kind: engine.PermissionRequest, SessionID: "interaction-native", ID: "same-request-id", Name: "fileChange", Input: []byte(`{"path":"fixture"}`)},
		{Kind: engine.UserInputRequested, SessionID: "interaction-native", ID: "same-request-id", Name: "question", Question: &engine.UserInputRequest{Questions: []engine.UserInputQuestion{{ID: "choice", Header: "Choice", Question: "Choose", Options: []engine.UserInputOption{{Label: "Blue"}}}}}},
	} {
		if err := emit(event); err != nil {
			return err
		}
	}
	e.readyOnce.Do(func() { close(e.ready) })
	for i := 0; i < 2; i++ {
		select {
		case kind := <-e.response:
			if e.holdResolved != nil {
				select {
				case <-e.holdResolved:
				case <-ctx.Done():
					return ctx.Err()
				}
			}
			if err := emit(engine.Event{Kind: engine.InteractionResolved, SessionID: "interaction-native", ID: "same-request-id", Name: kind}); err != nil {
				return err
			}
		case <-ctx.Done():
			return ctx.Err()
		}
	}
	return emit(engine.Event{Kind: engine.Completed, SessionID: "interaction-native", Outcome: engine.Succeeded})
}
func (*interactionFixtureEngine) Cancel(context.Context, string) error { return nil }
func (e *interactionFixtureEngine) RespondPermission(_ context.Context, native, id string, _ engine.PermissionDecision) error {
	if native != "interaction-native" || id != "same-request-id" {
		return engine.ErrSessionNotFound
	}
	if e.rejectResponse.Swap(false) {
		return errors.New("fixture response rejected")
	}
	e.permissions.Add(1)
	e.response <- "permission"
	return nil
}
func (e *interactionFixtureEngine) RespondUserInput(_ context.Context, native, id string, _ engine.UserInputResponse) error {
	if native != "interaction-native" || id != "same-request-id" {
		return engine.ErrSessionNotFound
	}
	if e.rejectResponse.Swap(false) {
		return errors.New("fixture response rejected")
	}
	e.questions.Add(1)
	e.response <- "user_input"
	return nil
}
func (*interactionFixtureEngine) Capabilities() engine.Capabilities {
	return engine.Capabilities{Streaming: true, Resume: true, Cancel: true, PermissionCallback: true, UserInputCallback: true}
}

func pendingInteractionFixture(t *testing.T) (*HubService, string, *interactionFixtureEngine, []EngineInteraction) {
	t.Helper()
	return pendingInteractionFixtureWithEngine(t, &interactionFixtureEngine{ready: make(chan struct{}), response: make(chan string, 2)})
}

func pendingInteractionFixtureWithEngine(t *testing.T, e *interactionFixtureEngine) (*HubService, string, *interactionFixtureEngine, []EngineInteraction) {
	t.Helper()
	s, _ := fixture(t)
	owner, err := s.Create(t.Context(), "native interactions", "")
	if err != nil {
		t.Fatal(err)
	}
	target := s.DefaultTarget
	target.EngineID = "native-interactions"
	s.Targets = append(s.Targets, target)
	if _, err := s.Store.Switch(t.Context(), owner.ID, target, "explicit local fixture"); err != nil {
		t.Fatal(err)
	}
	if err := s.Runtime.Register(target.EngineID, e); err != nil {
		t.Fatal(err)
	}
	if _, _, err := s.Runtime.Start(t.Context(), owner.ID, "fixture"); err != nil {
		t.Fatal(err)
	}
	select {
	case <-e.ready:
	case <-time.After(time.Second):
		t.Fatal("native fixture did not become ready")
	}
	pending, err := s.PendingEngineInteractions(t.Context(), owner.ID)
	if err != nil || len(pending) != 2 {
		t.Fatal("missing native requests", pending, err)
	}
	return s, owner.ID, e, pending
}

func TestEngineInteractionHandlesKeepQuestionAndPermissionSeparate(t *testing.T) {
	s, id, e, pending := pendingInteractionFixture(t)
	var permission, question EngineInteraction
	for _, item := range pending {
		if item.Event.Kind == engine.PermissionRequest {
			permission = item
		} else {
			question = item
		}
	}
	if permission.Handle.ID() == question.Handle.ID() {
		t.Fatal("interaction kind missing from opaque identity")
	}
	if err := s.RespondEnginePermission(t.Context(), id, question.Handle, true); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal("question granted permission", err)
	}
	if err := s.RespondEngineUserInput(t.Context(), id, permission.Handle, engine.UserInputResponse{}); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal("permission answered question", err)
	}
	if err := s.RespondEnginePermission(t.Context(), id, EngineInteractionHandle{}, true); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal("fabricated handle accepted", err)
	}
	if err := s.RespondEnginePermission(t.Context(), "foreign-session", permission.Handle, true); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal("foreign product accepted", err)
	}
	if err := s.RespondEnginePermission(t.Context(), id, permission.Handle, false); err != nil {
		t.Fatal(err)
	}
	if err := s.RespondEngineUserInput(t.Context(), id, question.Handle, engine.UserInputResponse{Answers: map[string][]string{"choice": {"Blue"}}}); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(time.Second)
	for s.Runtime.HasActive() && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	if e.permissions.Load() != 1 || e.questions.Load() != 1 {
		t.Fatal("responses crossed native methods", e.permissions.Load(), e.questions.Load())
	}
	if shown, err := s.PendingEngineInteractions(t.Context(), id); err != nil || len(shown) != 0 {
		t.Fatal("completed owner retained pending UI", shown, err)
	}
}

func TestEngineInteractionLateHandleCannotRediscoverReplacementOwner(t *testing.T) {
	s, id, e, pending := pendingInteractionFixture(t)
	if err := s.Runtime.CancelAndWait(t.Context(), id); err != nil {
		t.Fatal(err)
	}
	if _, _, err := s.Runtime.Start(t.Context(), id, "replacement fixture"); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(time.Second)
	var replacement []EngineInteraction
	for time.Now().Before(deadline) {
		current, _ := s.PendingEngineInteractions(t.Context(), id)
		if len(current) == 2 && current[0].Handle.binding.ExecutionID != pending[0].Handle.binding.ExecutionID {
			replacement = current
			break
		}
		time.Sleep(time.Millisecond)
	}
	if len(replacement) != 2 {
		t.Fatal("replacement native owner did not publish its own requests")
	}
	for _, item := range pending {
		var err error
		if item.Event.Kind == engine.PermissionRequest {
			err = s.RespondEnginePermission(t.Context(), id, item.Handle, true)
		} else {
			err = s.RespondEngineUserInput(t.Context(), id, item.Handle, engine.UserInputResponse{})
		}
		if err == nil {
			t.Fatal("late handle granted a replacement owner")
		}
	}
	if e.permissions.Load() != 0 || e.questions.Load() != 0 {
		t.Fatal("late callback reached replacement")
	}
}

func TestEngineAcceptedResponseDisappearsBeforeNativeResolvedFact(t *testing.T) {
	for _, kind := range []string{"allow", "deny", "question"} {
		t.Run(kind, func(t *testing.T) {
			release := make(chan struct{})
			defer close(release)
			e := &interactionFixtureEngine{ready: make(chan struct{}), response: make(chan string, 2), holdResolved: release}
			s, id, _, pending := pendingInteractionFixtureWithEngine(t, e)
			var chosen EngineInteraction
			for _, item := range pending {
				if (kind == "question") == (item.Event.Kind == engine.UserInputRequested) {
					chosen = item
				}
			}
			respond := func() error {
				if kind == "question" {
					return s.RespondEngineUserInput(t.Context(), id, chosen.Handle, engine.UserInputResponse{Answers: map[string][]string{"choice": {"Blue"}}})
				}
				return s.RespondEnginePermission(t.Context(), id, chosen.Handle, kind == "allow")
			}
			e.rejectResponse.Store(true)
			if err := respond(); err == nil {
				t.Fatal("fixture response was not rejected")
			}
			if visible, err := s.PendingEngineInteractions(t.Context(), id); err != nil || len(visible) != 2 {
				t.Fatal("failed response consumed the pending request", visible, err)
			}
			if err := respond(); err != nil {
				t.Fatal(err)
			}
			visible, err := s.PendingEngineInteractions(t.Context(), id)
			if err != nil || len(visible) != 1 || visible[0].Handle == chosen.Handle || !s.Runtime.SessionActive(id) {
				t.Fatal("accepted response reappeared before the delayed native resolved event", visible, err)
			}
			if err = respond(); !errors.Is(err, engine.ErrSessionNotFound) {
				t.Fatal("accepted handle dispatched again", err)
			}
			if e.permissions.Load()+e.questions.Load() != 1 {
				t.Fatal("a second observer duplicated the accepted callback")
			}
			history, err := s.Store.History(t.Context(), id, 0, 32)
			if err != nil {
				t.Fatal(err)
			}
			for _, entry := range history {
				if entry.Event.Kind == engine.InteractionResolved {
					t.Fatal("callback acceptance invented a native resolved fact")
				}
			}
		})
	}
}

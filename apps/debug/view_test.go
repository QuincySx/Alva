// INPUT: Confined UI callbacks, actual cancellation barriers and immutable snapshot expectations
// OUTPUT: One-closure backpressure, no late effect, exact state cloning and action-settlement regressions
// POS: [L6 diagnostics] Exercise owner boundaries rather than mirroring HTTP route implementation.
package debug

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/platform/system/loopback"
)

func fixtureSnapshot(count int) Snapshot {
	return Snapshot{Tree: Node{ID: "root", Type: "ActualFixtureView"}, State: map[string]any{"count": count}}
}
func fixtureServer(t *testing.T, controls, execution bool) *Server {
	t.Helper()
	token, err := loopback.NewToken()
	if err != nil {
		t.Fatal(err)
	}
	s, err := New(Config{Token: token, AllowControls: controls, AllowExecution: execution})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		if err := s.Close(ctx); err != nil {
			t.Error(err)
		}
	})
	return s
}

func TestCancelledQueuedViewCannotRunLateOrFloodUIDispatch(t *testing.T) {
	queue := make(chan func(), 1)
	calls := 0
	v, err := NewView(t.Context(), func(fn func()) { queue <- fn }, func() Snapshot { calls++; return fixtureSnapshot(0) }, nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	done := make(chan error, 1)
	go func() { _, err := v.read(ctx, "window"); done <- err }()
	var queued func()
	select {
	case queued = <-queue:
	case <-time.After(time.Second):
		t.Fatal("view did not queue")
	}
	cancel()
	if err := <-done; !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	if _, err := v.read(t.Context(), "window"); !errors.Is(err, ErrBusy) {
		t.Fatal("cancelled UI request released its still-queued slot", err)
	}
	queued()
	if calls != 0 {
		t.Fatal("cancelled view read ran after return")
	}
	v.detach()
	if _, err := v.read(t.Context(), "window"); !errors.Is(err, ErrUnavailable) {
		t.Fatal(err)
	}
}

func TestSnapshotOwnsExactDetachedState(t *testing.T) {
	state := map[string]any{"sequence": uint64(9007199254740993)}
	v, _ := NewView(t.Context(), func(fn func()) { fn() }, func() Snapshot { return Snapshot{Tree: Node{ID: "root", Type: "Fixture"}, State: state} }, nil, nil)
	snapshot, err := v.read(t.Context(), "window")
	if err != nil {
		t.Fatal(err)
	}
	state["sequence"] = uint64(1)
	raw, err := json.Marshal(snapshot)
	if err != nil || !strings.Contains(string(raw), "9007199254740993") {
		t.Fatal("snapshot shared mutable UI state or rounded its integer", err, string(raw))
	}
}

func TestRunningActionSettlesOnceAfterClientCancellation(t *testing.T) {
	s := fixtureServer(t, true, false)
	queue := make(chan func(), 1)
	entered, release := make(chan struct{}), make(chan struct{})
	count := 0
	v, _ := NewView(t.Context(), func(fn func()) { queue <- fn }, func() Snapshot { return fixtureSnapshot(count) }, func(string, json.RawMessage) error { count++; close(entered); <-release; return nil }, nil)
	remove, err := s.Register("window", "Fixture", []Method{{Name: "increment", Effect: "ui"}}, v)
	if err != nil {
		t.Fatal(err)
	}
	defer remove()
	initial, err := v.current("window")
	if err != nil {
		t.Fatal(err)
	}
	action := Action{OperationID: "once", ViewID: "window", Revision: initial.Revision, Method: "increment", Args: json.RawMessage(`{}`)}
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	result := make(chan ActionResult, 1)
	go func() { r, _ := s.act(ctx, action); result <- r }()
	fn := <-queue
	finished := make(chan struct{})
	go func() { fn(); close(finished) }()
	<-entered
	cancel()
	if r := <-result; r.Status != "pending" {
		t.Fatal("running effect falsely reported settled", r)
	}
	close(release)
	<-finished
	r, err := s.act(t.Context(), action)
	if err != nil || r.Status != "applied" || count != 1 {
		t.Fatal("retry did not preserve actual effect", r, err, count)
	}
	changed := action
	changed.Method = "different"
	if _, err := s.act(t.Context(), changed); err == nil {
		t.Fatal("operation ID was reused for a different effect")
	}
}

func TestStaleAndUndeclaredExecutionDoNotMutate(t *testing.T) {
	s := fixtureServer(t, true, false)
	count := 0
	v, _ := NewView(t.Context(), func(fn func()) { fn() }, func() Snapshot { return fixtureSnapshot(count) }, func(string, json.RawMessage) error { count++; return nil }, nil)
	remove, err := s.Register("window", "Fixture", []Method{{Name: "increment", Effect: "ui"}, {Name: "send", Effect: "execute"}}, v)
	if err != nil {
		t.Fatal(err)
	}
	defer remove()
	initial, _ := v.current("window")
	count = 1
	a := Action{OperationID: "stale", ViewID: "window", Revision: initial.Revision, Method: "increment", Args: json.RawMessage(`{}`)}
	result, err := s.act(t.Context(), a)
	if err != nil || result.Status != "rejected" || count != 1 {
		t.Fatal(result, err, count)
	}
	a.OperationID = "execution"
	a.Method = "send"
	if _, err := s.act(t.Context(), a); err == nil || count != 1 {
		t.Fatal("execution metadata authorized billing", err)
	}
}

func TestAppliedActionRemainsAppliedWhenNextSnapshotExceedsBudget(t *testing.T) {
	s := fixtureServer(t, true, false)
	effects := 0
	v, _ := NewView(t.Context(), func(fn func()) { fn() }, func() Snapshot {
		snapshot := fixtureSnapshot(effects)
		if effects > 0 {
			snapshot.State["oversized"] = strings.Repeat("x", 128<<10)
		}
		return snapshot
	}, func(string, json.RawMessage) error { effects++; return nil }, nil)
	unregister, err := s.Register("window", "Fixture", []Method{{Name: "increment", Effect: "ui"}}, v)
	if err != nil {
		t.Fatal(err)
	}
	defer unregister()
	initial, _ := v.current("window")
	action := Action{OperationID: "applied", ViewID: "window", Revision: initial.Revision, Method: "increment", Args: json.RawMessage(`{}`)}
	for range 2 {
		result, err := s.act(t.Context(), action)
		if err != nil || result.Status != "applied" || result.Error == "" || effects != 1 {
			t.Fatal("settled effect was rejected or replayed after snapshot failure", result, err, effects)
		}
	}
}

func TestActionArgumentsMustBeExactJSONObjectBeforeAnyUICallback(t *testing.T) {
	a := Action{OperationID: "object", ViewID: "window", Revision: strings.Repeat("a", 64), Method: "increment"}
	for _, raw := range []string{`null`, `[]`, `"text"`, `true`, `{"duplicate":1,"duplicate":2}`} {
		a.Args = json.RawMessage(raw)
		if err := a.validate(); err == nil {
			t.Fatal("nonobject/ambiguous UI arguments accepted")
		}
	}
}

func TestCaptureRefusesPreviousSceneUntilActualViewBuild(t *testing.T) {
	state, captures, invalidates := 0, 0, 0
	v, _ := NewView(t.Context(), func(fn func()) { fn() }, func() Snapshot { return fixtureSnapshot(state) }, nil, func() ([]byte, error) { captures++; return []byte("only after actual Build"), nil })
	rendered, changed := v.TrackRendering(func() { invalidates++ })
	for phase := range 4 {
		if phase == 1 {
			rendered()
		}
		if phase == 2 {
			state++
		}
		if phase == 3 {
			rendered()
			changed()
		}
		_, err := v.image(t.Context())
		if phase == 1 {
			if err != nil || captures != 1 {
				t.Fatal("actual rendered scene refused", err)
			}
		} else if !errors.Is(err, ErrFramePending) || captures != 1 && phase != 0 {
			t.Fatal("stale scene returned as current pixels", phase, err)
		}
	}
	if invalidates != 3 {
		t.Fatal("pending capture failed to request an actual frame")
	}
}

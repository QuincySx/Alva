// INPUT: Real persistent Hub waits, next-Turn parameter edits and loopback HTTP requests.
// OUTPUT: Same-Turn continuation accepts new defaults while the original Attempt parameters stay frozen.
// POS: [L7 assembly tests] Exercises the real recipe hash, journal, Runtime and provider; no paid account.
package bootstrap

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestSessionModelOptionsEditDuringWaitKeepsActualContinuationRecipe(t *testing.T) {
	var calls atomic.Int32
	temperatures := make(chan float64, 3)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			Temperature float64 `json:"temperature"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Error(err)
		}
		temperatures <- body.Temperature
		if calls.Add(1) == 1 {
			waitReply(w, "await_external", `{"request":{"job":"frozen-parameters"}}`, "")
		} else {
			waitReply(w, "", "", "continued with original parameters")
		}
	}))
	defer server.Close()
	config := waitConfig(t, server.URL)
	config.Provider.Model = "gpt-5.6"
	a, err := Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { closeApp(t, a) }()
	s, err := a.Service.Create(t.Context(), "session parameters and wait", "")
	if err != nil {
		t.Fatal(err)
	}
	initial := .25
	options := hub.ModelOptions{Generation: engine.GenerationOptions{Temperature: &initial}}
	edit := hub.SessionModelOptionsUpdate{SessionID: s.ID, BranchID: s.ActiveBranchID, Target: a.Targets[0], Options: &options}
	snapshot, err := a.Service.SaveSessionModelOptions(t.Context(), edit)
	if err != nil {
		t.Fatal(err)
	}
	if err := a.Service.EnableDurableWaits(t.Context(), s.ID, true); err != nil {
		t.Fatal(err)
	}
	turn, err := a.Service.Submit(t.Context(), s.ID, "one original input", nil)
	if err != nil {
		t.Fatal(err)
	}
	view := waitStatus(t, a, s.ID, engine.Waiting)
	next := .75
	edit.ExpectedRevision = snapshot.Revision
	options.Generation.Temperature = &next
	if _, err := a.Service.SaveSessionModelOptions(t.Context(), edit); err != nil {
		t.Fatal(err)
	}
	closeApp(t, a)
	a, err = Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	view, err = a.Service.Waits(t.Context(), s.ID)
	if err != nil {
		t.Fatal(err)
	}
	resolveFirst(t, a, s.ID, view, `{"value":"resolved"}`)
	view, err = a.Service.Waits(t.Context(), s.ID)
	if err != nil || !view.CanResume {
		t.Fatal("editable next-Turn snapshot blocked original recipe", view, err)
	}
	resumed, err := a.Service.ResumeWait(t.Context(), s.ID, waitTarget(view))
	if err != nil || resumed != turn {
		t.Fatal(resumed, err)
	}
	waitStatus(t, a, s.ID, engine.Succeeded)
	if calls.Load() != 2 || <-temperatures != initial || <-temperatures != initial {
		t.Fatal("continuation changed admitted parameters or replayed input", calls.Load())
	}
	if _, err := a.Service.Submit(t.Context(), s.ID, "next new turn", nil); err != nil {
		t.Fatal(err)
	}
	waitStatus(t, a, s.ID, engine.Succeeded)
	if calls.Load() != 3 || <-temperatures != next {
		t.Fatal("next Turn did not consume saved override", calls.Load())
	}
}

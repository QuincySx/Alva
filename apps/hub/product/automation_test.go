// INPUT: Real Alva harness, private product/native stores and explicit local schedule actions
// OUTPUT: Scheduled normal conversations, exact connection revision recovery and owned timer shutdown tests
// POS: [L7 Hub product] Product coordinator acceptance does not mock or duplicate the normal Hub runtime
package product

import (
	"context"
	"errors"
	"reflect"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
)

type automationReadBarrier struct {
	hub.AutomationStore
	entered chan struct{}
	release chan struct{}
}

func (b automationReadBarrier) Automation(ctx context.Context, owner, id string) (hub.Automation, error) {
	a, err := b.AutomationStore.Automation(ctx, owner, id)
	close(b.entered)
	<-b.release // Deliberately ignores cancellation to exercise the next seam.
	return a, err
}

func TestAutomationCloseDuringColdReadCannotAdmitAfterOwnerCancellation(t *testing.T) {
	s, requests := fixture(t)
	ctx := context.Background()
	p, err := s.Create(ctx, "close race", "")
	if err != nil {
		t.Fatal(err)
	}
	a, err := s.Automations.Store.CreateAutomation(ctx, p.ID, hub.AutomationDraft{Title: "review", Prompt: "must not execute", IntervalMinutes: 5})
	if err != nil {
		t.Fatal(err)
	}
	a, err = s.SetAutomationEnabled(ctx, p.ID, a.ID, a.Revision, true, true)
	if err != nil {
		t.Fatal(err)
	}
	b := automationReadBarrier{AutomationStore: s.Automations.Store, entered: make(chan struct{}), release: make(chan struct{})}
	s.Automations.Store = b
	tickDone := make(chan error, 1)
	go func() { tickDone <- s.Automations.Tick(ctx, a.NextRunAt) }()
	<-b.entered
	closeDone := make(chan error, 1)
	go func() {
		cleanup, cancel := context.WithTimeout(ctx, time.Second)
		defer cancel()
		closeDone <- s.Automations.Close(cleanup)
	}()
	<-s.Automations.owner.Done()
	close(b.release)
	if err = <-tickDone; !errors.Is(err, context.Canceled) {
		t.Fatal("read admitted beyond cancelled owner", err)
	}
	if err = <-closeDone; err != nil {
		t.Fatal(err)
	}
	if len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("shutdown started a model request")
	}
	if entries, err := s.Store.History(ctx, p.ID, 0, 32); err != nil || len(entries) != 0 {
		t.Fatal("shutdown created a Turn", entries, err)
	}
}

func TestAutomationActualNativeConversationAndExplicitOwner(t *testing.T) {
	s, requests := fixture(t)
	ctx := context.Background()
	p, err := s.Create(ctx, "schedule", "")
	if err != nil {
		t.Fatal(err)
	}
	a, err := s.Automations.Store.CreateAutomation(ctx, p.ID, hub.AutomationDraft{Title: "review", Prompt: "scheduled input", IntervalMinutes: 5})
	if err != nil {
		t.Fatal(err)
	}
	if s.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("creating/observing started a model")
	}
	if _, err = s.SetAutomationEnabled(ctx, p.ID, a.ID, a.Revision, true, false); err == nil {
		t.Fatal("billing auto approved")
	}
	a, err = s.SetAutomationEnabled(ctx, p.ID, a.ID, a.Revision, true, true)
	if err != nil {
		t.Fatal(err)
	}
	if s.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("enabling ran immediately rather than waiting interval")
	}
	if err = s.Automations.Tick(ctx, a.NextRunAt); err != nil {
		t.Fatal(err)
	}
	waitIdle(t, s, p.ID)
	request := <-requests
	if request.OwnerID != p.ID || request.Prompt != "scheduled input" {
		t.Fatal("not ordinary pinned request", request)
	}
	settled, err := s.Automations.Store.Automation(ctx, p.ID, a.ID)
	if err != nil || !settled.Enabled || settled.LastStatus != "completed" || settled.RunningTurnID != "" {
		t.Fatal("not settled", settled, err)
	}
	history, err := s.Store.History(ctx, p.ID, 0, 32)
	if err != nil || len(history) < 3 || history[0].Input != "scheduled input" {
		t.Fatal("not normal durable history", history, err)
	}
	if err = s.Automations.Start(); err != nil {
		t.Fatal(err)
	}
	cleanup, stop := context.WithTimeout(ctx, time.Second)
	defer stop()
	if err = s.Automations.Close(cleanup); err != nil {
		t.Fatal(err)
	}
	if err = s.Automations.Tick(ctx, time.Now().Add(24*time.Hour).UnixMilli()); err == nil {
		t.Fatal("closed owner started work")
	}
	if err = s.Automations.Start(); err == nil {
		t.Fatal("closed owner revived")
	}
	if len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("timer Start/Close replayed work")
	}
}

func TestAutomationConnectionRevisionPausesBeforeClaimAndRequiresExplicitRecovery(t *testing.T) {
	s, requests := fixture(t)
	ctx := t.Context()
	connections, err := NewConnectionsService(ctx, s.Store.(hub.ConnectionsStore))
	if err != nil {
		t.Fatal(err)
	}
	s.Connections = connections
	t.Cleanup(func() { _ = connections.Close(context.Background()) })
	saved, err := connections.Save(ctx, productConnectionFixture())
	if err != nil {
		t.Fatal(err)
	}
	p, err := s.Create(ctx, "revision-bound schedule", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Store.Switch(ctx, p.ID, saved.Target(saved.Models[0]), "explicit saved connection"); err != nil {
		t.Fatal(err)
	}
	a, err := s.Automations.Store.CreateAutomation(ctx, p.ID, hub.AutomationDraft{Title: "review", Prompt: "exact scheduled fixture", IntervalMinutes: 5})
	if err != nil {
		t.Fatal(err)
	}
	a, err = s.SetAutomationEnabled(ctx, p.ID, a.ID, a.Revision, true, true)
	if err != nil {
		t.Fatal(err)
	}
	before, err := s.Store.History(ctx, p.ID, 0, 32)
	if err != nil {
		t.Fatal(err)
	}
	edit := connectionEdit(saved)
	edit.Name = "new revision, same model and endpoint"
	saved, err = connections.Save(ctx, edit)
	if err != nil {
		t.Fatal(err)
	}
	if err = s.Automations.Tick(ctx, a.NextRunAt); err != nil {
		t.Fatal(err)
	}
	a, err = s.Automations.Store.Automation(ctx, p.ID, a.ID)
	if err != nil || a.Enabled || a.RunningTurnID != "" || a.NextRunAt != 0 || a.LastStatus != "failed" || s.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("stale connection revision admitted or fabricated work", a, err)
	}
	if history, err := s.Store.History(ctx, p.ID, 0, 32); err != nil || !reflect.DeepEqual(history, before) {
		t.Fatal("stale route consumed a durable Turn", history, err)
	}
	if _, err = s.SetAutomationEnabled(ctx, p.ID, a.ID, a.Revision, true, true); err == nil {
		t.Fatal("enable silently retargeted the old approved connection revision")
	}
	if _, err = s.Store.Switch(ctx, p.ID, saved.Target(saved.Models[0]), "explicit current connection revision"); err != nil {
		t.Fatal(err)
	}
	a, err = s.Automations.Store.UpdateAutomation(ctx, p.ID, a.ID, a.Revision, a.AutomationDraft)
	if err != nil || a.Enabled {
		t.Fatal("route review did not leave schedule paused", a, err)
	}
	a, err = s.SetAutomationEnabled(ctx, p.ID, a.ID, a.Revision, true, true)
	if err != nil {
		t.Fatal(err)
	}
	if err = s.Automations.Tick(ctx, a.NextRunAt); err != nil {
		t.Fatal(err)
	}
	waitIdle(t, s, p.ID)
	request := <-requests
	if request.AccountSource != saved.Target(saved.Models[0]).AccountSource || request.Prompt != a.Prompt || len(requests) != 0 {
		t.Fatal("explicit recovery lost the exact approved route or repeated work", request)
	}
}

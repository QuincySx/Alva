// INPUT: Real Alva harness, private product/native stores and explicit local schedule actions
// OUTPUT: Actual scheduled normal conversation, no implicit Open execution and owned timer shutdown tests
// POS: [L7 Hub product] Product coordinator acceptance does not mock or duplicate the normal Hub runtime
package product

import (
	"context"
	"errors"
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

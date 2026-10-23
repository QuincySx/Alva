// INPUT: Selected real temporary SQLite and explicit fault/slow provider wrappers.
// OUTPUT: Shared connection validation, safe cold snapshots, failed-write retention and actual shutdown ownership.
// POS: [L7 product service] Verifies service policy and cancellation without real model accounts or network calls.
package product

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	productsqlite "github.com/QuincySx/Alva/apps/hub/sqlite"
	"github.com/QuincySx/Alva/platform/storage"
)

type connectionFaultStore struct {
	hub.ConnectionsStore
	failure error
	entered chan struct{}
	finish  chan struct{}
	stale   []hub.ModelConnection
}

func (s *connectionFaultStore) SaveConnection(ctx context.Context, u hub.ConnectionUpdate) (hub.ModelConnection, error) {
	if s.entered != nil {
		close(s.entered)
		<-s.finish
		ctx = context.WithoutCancel(ctx)
	}
	if s.failure != nil {
		return hub.ModelConnection{}, s.failure
	}
	return s.ConnectionsStore.SaveConnection(ctx, u)
}
func (s *connectionFaultStore) Connections(ctx context.Context) ([]hub.ModelConnection, error) {
	if s.stale != nil {
		return s.stale, nil
	}
	return s.ConnectionsStore.Connections(ctx)
}
func productConnectionFixture() hub.ConnectionUpdate {
	return hub.ConnectionUpdate{Name: "Explicit local fixture", Protocol: "openai_chat", Endpoint: "http://127.0.0.1:33421/chat", Auth: "none", Models: []hub.ConnectionModel{{ID: "accurate-ID", Enabled: true}}, Enabled: true}
}

func TestConnectionsServiceValidatesAndPublishesOnlyCommittedSnapshots(t *testing.T) {
	ctx := t.Context()
	db, err := productsqlite.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	backend := &connectionFaultStore{ConnectionsStore: db}
	service, err := NewConnectionsService(ctx, backend)
	if err != nil {
		t.Fatal(err)
	}
	defer service.Close(ctx)
	u := productConnectionFixture()
	u.Models = nil
	saved, err := service.Save(ctx, u)
	if err != nil || len(service.Targets()) != 0 {
		t.Fatal("empty model list must save without auto-enabling models", err)
	}
	edit := connectionEdit(saved)
	edit.Models = []hub.ConnectionModel{{ID: "exact-case-ID", Name: "Display", Enabled: true}}
	saved, err = service.Save(ctx, edit)
	if err != nil || len(service.Targets()) != 1 {
		t.Fatal("explicit model did not enable route", err)
	}
	snapshot := service.Snapshot()
	snapshot[0].Name = "mutated"
	snapshot[0].Models[0].ID = "mutated"
	if actual, _ := service.Connection(saved.ID); actual.Name == "mutated" || actual.Models[0].ID == "mutated" {
		t.Fatal("caller mutated committed snapshot")
	}
	edit = connectionEdit(saved)
	edit.Models = append(edit.Models, edit.Models[0])
	if _, err = service.Save(ctx, edit); err == nil {
		t.Fatal("duplicate exact model IDs accepted")
	}
	backend.failure = errors.New("synthetic-secret-must-stay-private")
	edit = connectionEdit(saved)
	edit.Name = "must not publish"
	if _, err = service.Save(ctx, edit); err == nil || strings.Contains(err.Error(), "synthetic-secret") {
		t.Fatal("backend failure leaked a private cause")
	}
	if actual, _ := service.Connection(saved.ID); actual.Revision != saved.Revision || actual.Name != saved.Name {
		t.Fatal("failed save published draft")
	}
	backend.failure = nil
	old := saved
	saved, err = service.SetEnabled(ctx, saved.ID, saved.Revision, false)
	if err != nil {
		t.Fatal(err)
	}
	backend.stale = []hub.ModelConnection{old}
	if _, err = service.Read(ctx); err == nil {
		t.Fatal("stale backend read overwrote committed snapshot")
	}
	if actual, _ := service.Connection(saved.ID); actual.Revision != saved.Revision || actual.Enabled {
		t.Fatal("stale read published")
	}
	backend.stale = nil
	if _, _, _, err = service.ResolveTarget(ctx, old.Target(old.Models[0])); err == nil {
		t.Fatal("old revision route resolved")
	}
	if err = service.Delete(ctx, saved.ID, saved.Revision); err != nil || len(service.Snapshot()) != 0 {
		t.Fatal("delete did not publish", err)
	}
}

func TestConnectionsServiceCloseRetainsActualSlowWriteUntilComplete(t *testing.T) {
	ctx := t.Context()
	db, err := productsqlite.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	backend := &connectionFaultStore{ConnectionsStore: db, entered: make(chan struct{}), finish: make(chan struct{})}
	service, err := NewConnectionsService(ctx, backend)
	if err != nil {
		t.Fatal(err)
	}
	result := make(chan error, 1)
	go func() { _, err := service.Save(ctx, productConnectionFixture()); result <- err }()
	<-backend.entered
	short, cancel := context.WithTimeout(ctx, 10*time.Millisecond)
	defer cancel()
	if err = service.Close(short); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("Close claimed unfinished storage completion", err)
	}
	if _, err = service.Save(ctx, productConnectionFixture()); !errors.Is(err, storage.ErrClosed) {
		t.Fatal("closed owner admitted another save", err)
	}
	close(backend.finish)
	if err = <-result; err != nil {
		t.Fatal("actual late commit was lost", err)
	}
	if err = service.Close(ctx); err != nil {
		t.Fatal(err)
	}
	if len(service.Snapshot()) != 1 {
		t.Fatal("actual committed write not observed after Close retry")
	}
}

func TestGeminiConnectionResolvesOnlyItsExplicitModelPlaceholder(t *testing.T) {
	ctx := t.Context()
	db, err := productsqlite.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	service, err := NewConnectionsService(ctx, db)
	if err != nil {
		t.Fatal(err)
	}
	defer service.Close(ctx)
	u := productConnectionFixture()
	u.Protocol = "gemini"
	u.Endpoint = "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"
	u.Models = []hub.ConnectionModel{{ID: "explicit-model", Enabled: true}}
	saved, err := service.Save(ctx, u)
	if err != nil {
		t.Fatal(err)
	}
	target := saved.Target(saved.Models[0])
	if strings.Contains(target.BillingRoute, "{model}") {
		t.Fatal("unresolved template entered route")
	}
	_, config, images, err := service.ResolveTarget(ctx, target)
	if err != nil || images || config.Model != "explicit-model" || config.Endpoint != "https://generativelanguage.googleapis.com/v1beta/models/explicit-model:generateContent" {
		t.Fatal("explicit Gemini model endpoint mismatch", err)
	}
}

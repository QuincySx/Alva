// INPUT: Selected real temporary SQLite and explicit fault/slow provider wrappers.
// OUTPUT: Shared connection validation, safe cold snapshots, failed-write retention and actual shutdown ownership.
// POS: [L7 product service] Verifies service policy and cancellation without real model accounts or network calls.
package product

import (
	"context"
	"errors"
	"path/filepath"
	"reflect"
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

type invalidConnectionSecretsStore struct {
	hub.ConnectionsStore
	secret hub.ConnectionSecrets
}

func (s invalidConnectionSecretsStore) ConnectionSecrets(context.Context, string, uint64) (hub.ConnectionSecrets, error) {
	return s.secret, nil
}

func TestConnectionResolveRevalidatesPrivateStorageValues(t *testing.T) {
	db, err := productsqlite.Open(t.Context(), filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	u := productConnectionFixture()
	u.Auth, u.APIKey = "api_key", "synthetic-saved-key"
	saved, err := db.SaveConnection(t.Context(), u)
	if err != nil {
		t.Fatal(err)
	}
	for _, secret := range []hub.ConnectionSecrets{{}, {APIKey: "synthetic-key\nmalformed"}, {APIKey: "synthetic-key", Headers: map[string]string{"Content-Type": "forbidden"}}} {
		service, err := NewConnectionsService(t.Context(), invalidConnectionSecretsStore{ConnectionsStore: db, secret: secret})
		if err != nil {
			t.Fatal(err)
		}
		if _, _, _, err = service.ResolveTarget(t.Context(), saved.Target(saved.Models[0])); err == nil || strings.Contains(err.Error(), "synthetic") {
			t.Fatal("invalid private storage values resolved or leaked", err)
		}
		if err = service.Close(t.Context()); err != nil {
			t.Fatal(err)
		}
	}
}

type connectionReceiptStore struct {
	hub.ConnectionsStore
	mutate func(*hub.ModelConnection)
}

func (s *connectionReceiptStore) SaveConnection(ctx context.Context, edit hub.ConnectionUpdate) (hub.ModelConnection, error) {
	value, err := s.ConnectionsStore.SaveConnection(ctx, edit)
	if err == nil && s.mutate != nil {
		s.mutate(&value)
	}
	return value, err
}

func TestConnectionSaveVerifiesEveryConfigurationFieldBeforePublishing(t *testing.T) {
	for _, creating := range []bool{true, false} {
		for _, tc := range []struct {
			name   string
			mutate func(*hub.ModelConnection)
		}{
			{"name", func(c *hub.ModelConnection) { c.Name = "unexpected name" }},
			{"protocol", func(c *hub.ModelConnection) { c.Protocol = "openai_responses" }},
			{"endpoint", func(c *hub.ModelConnection) { c.Endpoint = "http://127.0.0.1:33421/other" }},
			{"catalog", func(c *hub.ModelConnection) { c.CatalogURL = "http://127.0.0.1:33421/other-models" }},
			{"authentication", func(c *hub.ModelConnection) { c.Auth, c.HasAPIKey = "api_key", true }},
			{"enabled", func(c *hub.ModelConnection) { c.Enabled = false }},
			{"key-presence", func(c *hub.ModelConnection) { c.HasAPIKey = true }},
			{"header-names", func(c *hub.ModelConnection) { c.HeaderNames = []string{"X-Unexpected-Header"} }},
			{"model-id", func(c *hub.ModelConnection) { c.Models[0].ID = "unexpected-model" }},
			{"model-name", func(c *hub.ModelConnection) { c.Models[0].Name = "unexpected name" }},
			{"model-enabled", func(c *hub.ModelConnection) { c.Models[0].Enabled = false }},
			{"model-images", func(c *hub.ModelConnection) { c.Models[0].ImageCapable = true }},
		} {
			mode := "create/"
			if !creating {
				mode = "edit/"
			}
			t.Run(mode+tc.name, func(t *testing.T) {
				db, err := productsqlite.Open(t.Context(), filepath.Join(t.TempDir(), "private"))
				if err != nil {
					t.Fatal(err)
				}
				defer db.Close()
				backend := &connectionReceiptStore{ConnectionsStore: db}
				service, err := NewConnectionsService(t.Context(), backend)
				if err != nil {
					t.Fatal(err)
				}
				defer service.Close(t.Context())
				edit := productConnectionFixture()
				edit.CatalogURL = "http://127.0.0.1:33421/models"
				var before hub.ModelConnection
				if !creating {
					before, err = service.Save(t.Context(), edit)
					if err != nil {
						t.Fatal(err)
					}
					edit = connectionEdit(before)
					edit.Name = "intended edit"
				}
				backend.mutate = tc.mutate
				if _, err = service.Save(t.Context(), edit); err == nil || !strings.Contains(err.Error(), "reload before retrying") {
					t.Fatal("valid but mismatched configuration receipt was accepted", err)
				}
				values := service.Snapshot()
				if creating && len(values) != 0 || !creating && (len(values) != 1 || values[0].Revision != before.Revision || values[0].Name != before.Name) {
					t.Fatal("unverified receipt changed the published configuration")
				}
				values, err = service.Read(t.Context())
				if err != nil || len(values) != 1 || values[0].Revision != edit.ExpectedRevision+1 || values[0].Name != edit.Name || values[0].Models[0].ID != edit.Models[0].ID {
					t.Fatal("explicit reload failed to reconcile the actual committed save", err)
				}
			})
		}
	}
}

type uncertainConnectionCreationStore struct {
	hub.ConnectionsStore
	creation    hub.ConnectionCreationStore
	loseReceipt bool
	forgeRead   bool
}

func (s *uncertainConnectionCreationStore) CreateConnection(ctx context.Context, request hub.ConnectionCreate) (hub.ModelConnection, error) {
	value, err := s.creation.CreateConnection(ctx, request)
	if err == nil && s.loseReceipt {
		return hub.ModelConnection{}, errors.New("synthetic-private-lost-receipt")
	}
	return value, err
}
func (s *uncertainConnectionCreationStore) ConnectionCreation(ctx context.Context, request string) (hub.ConnectionCreationReceipt, error) {
	value, err := s.creation.ConnectionCreation(ctx, request)
	if err == nil && s.forgeRead {
		value.RequestID = "another-request"
	}
	return value, err
}

func TestConnectionServiceUncertainCreateExplicitResolveAndReplay(t *testing.T) {
	db, err := productsqlite.Open(t.Context(), filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	backend := &uncertainConnectionCreationStore{ConnectionsStore: db, creation: db, loseReceipt: true}
	service, err := NewConnectionsService(t.Context(), backend)
	if err != nil {
		t.Fatal(err)
	}
	defer service.Close(t.Context())
	request := hub.ConnectionCreate{RequestID: "fixture-service-create", Update: productConnectionFixture()}
	if _, err = service.Create(t.Context(), request); err == nil || strings.Contains(err.Error(), "synthetic-private") || len(service.Snapshot()) != 0 {
		t.Fatal("uncertain commit leaked private cause or claimed a published save", err)
	}
	backend.forgeRead = true
	if _, err = service.ResolveCreation(t.Context(), request.RequestID); err == nil || len(service.Snapshot()) != 0 {
		t.Fatal("recovery accepted another request's receipt")
	}
	backend.forgeRead = false
	receipt, err := service.ResolveCreation(t.Context(), request.RequestID)
	if err != nil || receipt.Current == nil || receipt.Created.ID == "" || len(service.Snapshot()) != 1 {
		t.Fatal("explicit recovery lost the actual stable created identity", err)
	}
	backend.loseReceipt = false
	again, err := service.Create(t.Context(), request)
	if err != nil || !reflect.DeepEqual(again, receipt.Created) || len(service.Snapshot()) != 1 {
		t.Fatal("exact retry created another entity or changed its original receipt", err)
	}
	changed := request
	changed.Update.Name = "changed new-create payload"
	if _, err = service.Create(t.Context(), changed); !errors.Is(err, hub.ErrConnectionCreationConflict) {
		t.Fatal("changed payload bypassed request identity", err)
	}
	edit := connectionEdit(again)
	edit.Name = "other window edit"
	edited, err := service.Save(t.Context(), edit)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = service.Create(t.Context(), request); !errors.Is(err, hub.ErrConnectionCreationConflict) {
		t.Fatal("create replay silently accepted a revised connection", err)
	}
	resolved, err := service.ResolveCreation(t.Context(), request.RequestID)
	if err != nil || !reflect.DeepEqual(resolved.Created, receipt.Created) || resolved.Current == nil || resolved.Current.Revision != edited.Revision {
		t.Fatal("recovery conflated original and current revisions", err)
	}
	if err = service.Delete(t.Context(), edited.ID, edited.Revision); err != nil {
		t.Fatal(err)
	}
	resolved, err = service.ResolveCreation(t.Context(), request.RequestID)
	if err != nil || resolved.Current != nil || resolved.Created.ID != edited.ID || len(service.Snapshot()) != 0 {
		t.Fatal("deleted creation could not be recovered without recreation", err)
	}
	if _, err = service.Create(t.Context(), request); !errors.Is(err, hub.ErrConnectionCreationConflict) {
		t.Fatal("retry recreated a deliberately deleted connection", err)
	}
}

func TestConnectionServiceMissingCreationPortNeverFallsBackToSave(t *testing.T) {
	db, err := productsqlite.Open(t.Context(), filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	baseOnly := struct{ hub.ConnectionsStore }{db}
	service, err := NewConnectionsService(t.Context(), baseOnly)
	if err != nil {
		t.Fatal(err)
	}
	defer service.Close(t.Context())
	if _, err = service.Create(t.Context(), hub.ConnectionCreate{RequestID: "fixture-no-fallback", Update: productConnectionFixture()}); err == nil {
		t.Fatal("recoverable creation silently used non-idempotent SaveConnection")
	}
	if _, err = service.ResolveCreation(t.Context(), "fixture-no-fallback"); err == nil {
		t.Fatal("missing creation resolver claimed success")
	}
	values, err := db.Connections(t.Context())
	if err != nil || len(values) != 0 {
		t.Fatal("missing optional creation capability wrote an entity", err)
	}
}

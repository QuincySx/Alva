// INPUT: Shared connection owner, real private SQLite and deterministic failing/slow preference providers.
// OUTPUT: Committed cache isolation, independent connection revisions, per-model CAS and joined shutdown.
// POS: [L7 product service testing] No external account, model request or implicit route selection.
package product

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	productsqlite "github.com/QuincySx/Alva/apps/hub/sqlite"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage"
)

type modelPreferenceFaultStore struct {
	hub.ConnectionsStore
	failure                   error
	reply                     *hub.ModelPreferencesSnapshot
	readFailure               error
	readReply                 *hub.ModelPreferencesSnapshot
	commitUnknown             bool
	writes                    int
	entered, canceled, finish chan struct{}
	ignoreCancel              bool
}

func (s *modelPreferenceFaultStore) ModelPreferences(ctx context.Context) (hub.ModelPreferencesSnapshot, error) {
	if s.readFailure != nil {
		return hub.ModelPreferencesSnapshot{}, s.readFailure
	}
	if s.readReply != nil {
		return s.readReply.Clone(), nil
	}
	return s.ConnectionsStore.ModelPreferences(ctx)
}

func (s *modelPreferenceFaultStore) SaveModelPreference(ctx context.Context, p hub.ModelPreference) (hub.ModelPreferencesSnapshot, error) {
	s.writes++
	if s.entered != nil {
		close(s.entered)
		<-ctx.Done()
		close(s.canceled)
		<-s.finish
		if !s.ignoreCancel {
			return hub.ModelPreferencesSnapshot{}, ctx.Err()
		}
		ctx = context.WithoutCancel(ctx)
	}
	if s.failure != nil {
		return hub.ModelPreferencesSnapshot{}, s.failure
	}
	if s.reply != nil {
		return s.reply.Clone(), nil
	}
	value, err := s.ConnectionsStore.SaveModelPreference(ctx, p)
	if err == nil && s.commitUnknown {
		return hub.ModelPreferencesSnapshot{}, errors.New("synthetic unknown result after commit")
	}
	return value, err
}

func modelPreferenceService(t *testing.T) (*ConnectionsService, *modelPreferenceFaultStore) {
	t.Helper()
	db, err := productsqlite.Open(t.Context(), filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = db.Close() })
	backend := &modelPreferenceFaultStore{ConnectionsStore: db}
	service, err := NewConnectionsService(t.Context(), backend)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = service.Close(context.Background()) })
	return service, backend
}

func serviceModelRoute() hub.Target {
	return hub.Target{EngineID: "alva", ModelID: "model", ProviderID: "configured", AccountSource: "explicit-account", BillingRoute: "explicit-billing"}
}

func TestModelPreferenceSaveRejectsMissingOrMismatchedReceipts(t *testing.T) {
	for _, kind := range []string{"missing", "old-entry", "wrong-content"} {
		t.Run(kind, func(t *testing.T) {
			service, backend := modelPreferenceService(t)
			route := serviceModelRoute()
			p := service.ModelPreference(route)
			p.Favorite = true
			reply := hub.ModelPreferencesSnapshot{Revision: 2}
			if kind != "missing" {
				x := p.Clone()
				x.Revision = 1
				if kind == "old-entry" {
					x.Revision = 2
				} else {
					x.Favorite = false
				}
				reply.Entries = []hub.ModelPreference{x}
			}
			backend.reply = &reply
			if _, err := service.SaveModelPreference(t.Context(), route, p); err == nil {
				t.Fatal("unverified receipt reported success")
			}
			if current := service.ModelPreference(route); current.Revision != 0 || current.Favorite {
				t.Fatal("unverified receipt replaced committed cache")
			}
		})
	}
}

func TestModelPreferenceServiceMetadataDoesNotReviseConnectionAuthority(t *testing.T) {
	service, _ := modelPreferenceService(t)
	connection, err := service.Save(t.Context(), productConnectionFixture())
	if err != nil {
		t.Fatal(err)
	}
	route := connection.Target(connection.Models[0])
	p := service.ModelPreference(route)
	p.Favorite, p.PreferredEngine = true, "alva"
	p, err = service.SaveModelPreference(t.Context(), route, p)
	if err != nil || p.Revision != 1 {
		t.Fatal(p, err)
	}
	current, ok := service.Connection(connection.ID)
	if !ok || current.Revision != connection.Revision || hub.RouteKey(current.Target(current.Models[0])) != hub.RouteKey(route) {
		t.Fatal("favorite changed connection execution/auth revision")
	}
	edit := connectionEdit(connection)
	edit.Name = "new display name"
	updated, err := service.Save(t.Context(), edit)
	if err != nil {
		t.Fatal(err)
	}
	newRoute := updated.Target(updated.Models[0])
	if updated.Revision == connection.Revision || service.ModelPreference(newRoute).Key != p.Key || !service.ModelPreference(newRoute).Favorite {
		t.Fatal("connection revision lost stable model preferences")
	}
	other := newRoute
	other.AccountSource = "connection:other-account@1"
	if service.ModelPreference(other).Revision != 0 || service.ModelPreference(other).Favorite {
		t.Fatal("another connection inherited model preferences")
	}
}

func TestModelPreferenceServicePublishesOnlyCommittedIndependentClones(t *testing.T) {
	service, backend := modelPreferenceService(t)
	route := serviceModelRoute()
	p := service.ModelPreference(route)
	temperature, tokens := .35, uint32(500)
	p.Options = hub.ModelOptions{Generation: engine.GenerationOptions{Temperature: &temperature, MaxTokens: &tokens}, SystemPrompt: "saved"}
	p, err := service.SaveModelPreference(t.Context(), route, p)
	if err != nil {
		t.Fatal(err)
	}
	*p.Options.Generation.Temperature = .95
	*p.Options.Generation.MaxTokens = 900
	actual := service.ModelPreference(route)
	if *actual.Options.Generation.Temperature != .35 || *actual.Options.Generation.MaxTokens != 500 {
		t.Fatal("caller changed committed owner cache")
	}
	backend.failure = errors.New("synthetic-private-provider-cause")
	draft := actual.Clone()
	draft.Favorite, draft.Options.SystemPrompt = true, "must not publish"
	if _, err = service.SaveModelPreference(t.Context(), route, draft); err == nil || strings.Contains(err.Error(), "synthetic-private") {
		t.Fatal("failed save leaked provider detail", err)
	}
	if value := service.ModelPreference(route); value.Revision != actual.Revision || value.Favorite || value.Options.SystemPrompt != "saved" {
		t.Fatal("failed draft entered owner cache", value)
	}
	backend.failure = nil
	stale := hub.ModelPreferencesSnapshot{Revision: 1, Entries: []hub.ModelPreference{actual.Clone()}}
	backend.reply = &stale
	if _, err = service.SaveModelPreference(t.Context(), route, draft); err == nil {
		t.Fatal("stale snapshot replaced committed cache")
	}
	invalid := stale.Clone()
	invalid.Revision = 99
	invalid.Entries = append(invalid.Entries, invalid.Entries[0])
	backend.reply = &invalid
	if _, err = service.SaveModelPreference(t.Context(), route, draft); err == nil {
		t.Fatal("invalid snapshot replaced committed cache")
	}
	if value := service.ModelPreference(route); value.Revision != actual.Revision || value.Favorite {
		t.Fatal("rejected provider snapshot was published", value)
	}
	backend.reply = nil
	if _, err = service.SaveModelPreference(t.Context(), route, draft); err != nil {
		t.Fatal("failure prevented explicit retry", err)
	}
}

func TestModelPreferenceUnknownCommitRequiresExplicitReloadThenExplicitSave(t *testing.T) {
	service, backend := modelPreferenceService(t)
	route := serviceModelRoute()
	draft := service.ModelPreference(route)
	draft.Favorite, draft.Options.SystemPrompt = true, "explicit draft"
	backend.commitUnknown = true
	if _, err := service.SaveModelPreference(t.Context(), route, draft); err == nil {
		t.Fatal("unknown provider completion was reported successful")
	}
	if cached := service.ModelPreference(route); cached.Revision != 0 || cached.Favorite || cached.Options.SystemPrompt != "" {
		t.Fatal("unknown write result entered committed cache", cached)
	}
	actual, err := backend.ConnectionsStore.ModelPreferences(t.Context())
	if err != nil || len(actual.Entries) != 1 || actual.Entries[0].Revision != 1 || !actual.Entries[0].Favorite || backend.writes != 1 {
		t.Fatal("fixture did not commit before returning its unknown result", actual, err)
	}
	if err = service.ReloadModelPreferences(t.Context()); err != nil {
		t.Fatal(err)
	}
	reloaded := service.ModelPreference(route)
	if reloaded.Revision != 1 || !reloaded.Favorite || reloaded.Options.SystemPrompt != "explicit draft" || backend.writes != 1 {
		t.Fatal("explicit reload lost committed revision or replayed a write", reloaded, backend.writes)
	}
	backend.commitUnknown = false
	reloaded.Favorite = false
	saved, err := service.SaveModelPreference(t.Context(), route, reloaded)
	if err != nil || saved.Revision != 2 || saved.Favorite || backend.writes != 2 {
		t.Fatal("explicit save after reload did not use the actual CAS revision", saved, err)
	}
}

func TestModelPreferenceReloadRejectsCorruptStaleAndFailedReads(t *testing.T) {
	service, backend := modelPreferenceService(t)
	route := serviceModelRoute()
	value := service.ModelPreference(route)
	temperature := .6
	value.Favorite, value.Options.Generation.Temperature = true, &temperature
	committed, err := service.SaveModelPreference(t.Context(), route, value)
	if err != nil {
		t.Fatal(err)
	}
	for _, mode := range []string{"failed", "stale", "corrupt"} {
		t.Run(mode, func(t *testing.T) {
			backend.readFailure, backend.readReply = nil, nil
			switch mode {
			case "failed":
				backend.readFailure = errors.New("synthetic-private-read-cause")
			case "stale":
				backend.readReply = &hub.ModelPreferencesSnapshot{Revision: 1, Entries: []hub.ModelPreference{committed.Clone()}}
			case "corrupt":
				backend.readReply = &hub.ModelPreferencesSnapshot{Revision: 99, Entries: []hub.ModelPreference{committed.Clone(), committed.Clone()}}
			}
			if err := service.ReloadModelPreferences(t.Context()); err == nil || strings.Contains(err.Error(), "synthetic-private") {
				t.Fatal("invalid reload succeeded or leaked provider detail", err)
			}
			if cached := service.ModelPreference(route); cached.Revision != committed.Revision || !cached.Favorite || *cached.Options.Generation.Temperature != .6 || backend.writes != 1 {
				t.Fatal("failed reload replaced cache or replayed a write", cached, backend.writes)
			}
		})
	}
	backend.readFailure, backend.readReply = nil, nil
	if err := service.ReloadModelPreferences(t.Context()); err != nil {
		t.Fatal("valid explicit reload did not recover", err)
	}
	cached := service.ModelPreference(route)
	*cached.Options.Generation.Temperature = 1
	if *service.ModelPreference(route).Options.Generation.Temperature != .6 {
		t.Fatal("reloaded cache leaked mutable pointers")
	}
}

func TestModelPreferenceServiceRejectsUnsupportedEditsAndMismatchedIdentity(t *testing.T) {
	service, _ := modelPreferenceService(t)
	route := serviceModelRoute()
	for _, agent := range []string{"pi", "codex", "claude"} {
		target := route
		target.EngineID = agent
		p := service.ModelPreference(target)
		temperature := .4
		p.Options.Generation.Temperature = &temperature
		if _, err := service.SaveModelPreference(t.Context(), target, p); !errors.Is(err, engine.ErrUnsupported) {
			t.Fatal("unsupported generation edit accepted", agent, err)
		}
		if service.ModelPreference(target).Revision != 0 {
			t.Fatal("unsupported edit was published")
		}
	}
	for _, agent := range []string{"pi", "codex"} {
		target := route
		target.EngineID = agent
		p := service.ModelPreference(target)
		p.Options.SystemPrompt, p.Options.MaxTurns = "unsupported", 3
		if _, err := service.SaveModelPreference(t.Context(), target, p); !errors.Is(err, engine.ErrUnsupported) {
			t.Fatal("unsupported prompt/turn edit accepted", agent, err)
		}
	}
	claude := route
	claude.EngineID = "claude"
	p := service.ModelPreference(claude)
	p.Options.SystemPrompt, p.Options.MaxTurns = "explicit", 3
	if _, err := service.SaveModelPreference(t.Context(), claude, p); err != nil {
		t.Fatal("declared Claude prompt/turn configuration rejected", err)
	}
	p = service.ModelPreference(route)
	other := route
	other.AccountSource = "different-account"
	if _, err := service.SaveModelPreference(t.Context(), other, p); err == nil {
		t.Fatal("mismatched account/model preference accepted")
	}
}

func TestModelPreferenceMetadataKeepsUnchangedDefaultsAcrossCompatibleAgents(t *testing.T) {
	service, _ := modelPreferenceService(t)
	route := serviceModelRoute()
	value := service.ModelPreference(route)
	temperature := .4
	value.Options.Generation.Temperature = &temperature
	value.Options.SystemPrompt, value.Options.MaxTurns = "Alva defaults", 5
	if _, err := service.SaveModelPreference(t.Context(), route, value); err != nil {
		t.Fatal(err)
	}
	compatible := route
	compatible.EngineID = "pi"
	value = service.ModelPreference(compatible)
	value.Favorite, value.PreferredEngine = true, "pi"
	saved, err := service.SaveModelPreference(t.Context(), compatible, value)
	if err != nil || !saved.Favorite || saved.PreferredEngine != "pi" || *saved.Options.Generation.Temperature != .4 || saved.Options.SystemPrompt != "Alva defaults" {
		t.Fatal("metadata-only save rejected or erased another compatible Agent's defaults", saved, err)
	}
	changed := saved.Clone()
	*changed.Options.Generation.Temperature = .8
	if _, err := service.SaveModelPreference(t.Context(), compatible, changed); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal("unsupported Agent edited preserved generation defaults", err)
	}
	if actual := service.ModelPreference(route); actual.Revision != saved.Revision || *actual.Options.Generation.Temperature != .4 {
		t.Fatal("unsupported edit changed shared defaults", actual)
	}
}

func TestModelPreferenceServiceConcurrentCASDoesNotCrossModels(t *testing.T) {
	for _, same := range []bool{false, true} {
		name := "different-models"
		if same {
			name = "same-model"
		}
		t.Run(name, func(t *testing.T) {
			service, _ := modelPreferenceService(t)
			start, result := make(chan struct{}), make(chan error, 8)
			var ready sync.WaitGroup
			ready.Add(8)
			for i := range 8 {
				route := serviceModelRoute()
				if !same {
					route.ModelID = string(rune('a' + i))
				}
				value := service.ModelPreference(route)
				value.Favorite = true
				go func() {
					ready.Done()
					<-start
					_, err := service.SaveModelPreference(t.Context(), route, value)
					result <- err
				}()
			}
			ready.Wait()
			close(start)
			wins, conflicts := 0, 0
			for range 8 {
				err := <-result
				if err == nil {
					wins++
				} else if errors.Is(err, hub.ErrModelPreferenceConflict) {
					conflicts++
				} else {
					t.Fatal(err)
				}
			}
			want := 8
			if same {
				want = 1
			}
			if wins != want || conflicts != 8-want {
				t.Fatal("owner CAS crossed models or accepted competing revisions", wins, conflicts)
			}
		})
	}
}

func TestModelPreferenceServiceCloseCancelsAndJoinsActualStorage(t *testing.T) {
	for _, lateCommit := range []bool{false, true} {
		name := "canceled-write"
		if lateCommit {
			name = "late-real-commit"
		}
		t.Run(name, func(t *testing.T) {
			service, backend := modelPreferenceService(t)
			backend.entered, backend.canceled, backend.finish = make(chan struct{}), make(chan struct{}), make(chan struct{})
			backend.ignoreCancel = lateCommit
			finish := sync.OnceFunc(func() { close(backend.finish) })
			defer finish()
			route := serviceModelRoute()
			value := service.ModelPreference(route)
			value.Favorite = true
			result := make(chan error, 1)
			go func() { _, err := service.SaveModelPreference(t.Context(), route, value); result <- err }()
			<-backend.entered
			short, cancel := context.WithTimeout(t.Context(), 15*time.Millisecond)
			defer cancel()
			if err := service.Close(short); !errors.Is(err, context.DeadlineExceeded) {
				t.Fatal("Close claimed unfinished storage was complete", err)
			}
			<-backend.canceled
			if _, err := service.SaveModelPreference(t.Context(), route, value); !errors.Is(err, storage.ErrClosed) {
				t.Fatal("closed owner admitted another preference write", err)
			}
			finish()
			err := <-result
			if lateCommit && err != nil || !lateCommit && !errors.Is(err, context.Canceled) {
				t.Fatal("actual provider completion was lost", err)
			}
			if err := service.Close(t.Context()); err != nil {
				t.Fatal(err)
			}
			actual := service.ModelPreference(route)
			if lateCommit && (!actual.Favorite || actual.Revision != 1) || !lateCommit && (actual.Favorite || actual.Revision != 0) {
				t.Fatal("shutdown cache diverged from actual commit", actual)
			}
		})
	}
}

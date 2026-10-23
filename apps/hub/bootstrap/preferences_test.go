// INPUT: Opaque program-selected product profile and real private SQLite stores.
// OUTPUT: Preference port wiring, cold application startup and failed-read cleanup regressions.
// POS: [L7 composition] Appearance uses the selected component without fallback or session creation.
package bootstrap

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
)

type preferenceProfile struct {
	ProductStorage
	readError             error
	reads, writes, closes int
}

func (s *preferenceProfile) Preferences(ctx context.Context) (hub.Preferences, error) {
	s.reads++
	if s.readError != nil {
		return hub.Preferences{}, s.readError
	}
	return s.ProductStorage.Preferences(ctx)
}

func (s *preferenceProfile) UpdatePreferences(ctx context.Context, update hub.PreferenceUpdate) (hub.Preferences, error) {
	s.writes++
	return s.ProductStorage.UpdatePreferences(ctx, update)
}

func (s *preferenceProfile) Close() error {
	s.closes++
	return s.ProductStorage.Close()
}

func TestPreferencesUseSelectedProfileWithoutSessions(t *testing.T) {
	for _, failRead := range []bool{false, true} {
		name := "cold shared owner"
		if failRead {
			name = "read failure closes selected component"
		}
		t.Run(name, func(t *testing.T) {
			directory := filepath.Join(t.TempDir(), "program")
			failure := errors.New("selected preferences unavailable")
			var selected *preferenceProfile
			factory := func(ctx context.Context, path string) (Stores, error) {
				result, err := LocalStores(ctx, filepath.Join(path, "chosen"))
				if err != nil {
					return result, err
				}
				selected = &preferenceProfile{ProductStorage: result.Product}
				if failRead {
					selected.readError = failure
				}
				result.Product = selected
				return result, nil
			}
			app, err := Open(t.Context(), Config{DataDirectory: directory, Demo: true, OpenStores: factory})
			if failRead {
				if app != nil || !errors.Is(err, failure) || selected == nil || selected.closes != 1 {
					t.Fatalf("failed preference read lost cleanup/error: app=%v selected=%+v err=%v", app, selected, err)
				}
				// Actual locks/resources must be released; no alternate backend is opened.
				reopened, err := LocalStores(t.Context(), filepath.Join(directory, "chosen"))
				if err != nil {
					t.Fatal(err)
				}
				if err = errors.Join(reopened.Product.Close(), reopened.Sessions.Close()); err != nil {
					t.Fatal(err)
				}
			} else {
				if err != nil {
					t.Fatal(err)
				}
				t.Cleanup(func() { closeApp(t, app) })
				initial := app.Service.Preferences.Snapshot()
				theme := "dark"
				saved, err := app.Service.Preferences.Update(t.Context(), hub.PreferenceUpdate{ExpectedRevision: initial.Revision, Theme: &theme})
				if err != nil || saved.Theme != theme || selected.reads != 1 || selected.writes != 1 {
					t.Fatalf("selected preference port bypassed: selected=%+v saved=%+v err=%v", selected, saved, err)
				}
				page, err := app.Store.PageSessions(t.Context(), "", hub.SessionCursor{}, 8)
				if err != nil || len(page.Sessions) != 0 || app.Runtime.HasActive() {
					t.Fatalf("appearance created session/execution: page=%+v err=%v", page, err)
				}
			}
			if _, err := os.Stat(filepath.Join(directory, "product", "product.sqlite")); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("implicit fallback database: %v", err)
			}
		})
	}
}

type blockedPreferenceProfile struct {
	ProductStorage
	entered, canceled, release chan struct{}
	closes                     atomic.Int32
}

func (s *blockedPreferenceProfile) UpdatePreferences(ctx context.Context, update hub.PreferenceUpdate) (hub.Preferences, error) {
	close(s.entered)
	<-ctx.Done()
	close(s.canceled)
	<-s.release
	// A selected backend may finish an already-admitted commit after cancellation.
	return s.ProductStorage.UpdatePreferences(context.WithoutCancel(ctx), update)
}

func (s *blockedPreferenceProfile) Close() error {
	s.closes.Add(1)
	return s.ProductStorage.Close()
}

func TestPreferencesCloseRetainsStoresUntilActualUpdateReturns(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "private")
	var selected *blockedPreferenceProfile
	app, err := Open(t.Context(), Config{DataDirectory: directory, Demo: true, OpenStores: func(ctx context.Context, path string) (Stores, error) {
		result, err := LocalStores(ctx, path)
		if err != nil {
			return result, err
		}
		selected = &blockedPreferenceProfile{ProductStorage: result.Product, entered: make(chan struct{}), canceled: make(chan struct{}), release: make(chan struct{})}
		result.Product = selected
		return result, nil
	}})
	if err != nil {
		t.Fatal(err)
	}
	var release sync.Once
	defer func() {
		release.Do(func() { close(selected.release) })
		closeApp(t, app)
	}()
	done := make(chan error, 1)
	theme := "dark"
	go func() {
		_, err := app.Service.Preferences.Update(t.Context(), hub.PreferenceUpdate{ExpectedRevision: 1, Theme: &theme})
		done <- err
	}()
	select {
	case <-selected.entered:
	case <-time.After(3 * time.Second):
		t.Fatal("update did not enter selected backend")
	}
	cleanup, cancel := context.WithTimeout(t.Context(), 30*time.Millisecond)
	err = app.Close(cleanup)
	cancel()
	if !errors.Is(err, context.DeadlineExceeded) || app.closed || selected.closes.Load() != 0 {
		t.Fatalf("application closed stores under an actual write: closed=%v calls=%d err=%v", app.closed, selected.closes.Load(), err)
	}
	select {
	case <-selected.canceled:
	case <-time.After(3 * time.Second):
		t.Fatal("application did not cancel actual backend context")
	}
	// Stop admission is permanent; closing again must never replay the write.
	if _, err = app.Service.Preferences.Read(t.Context()); err == nil {
		t.Fatal("closing owner admitted another read")
	}
	release.Do(func() { close(selected.release) })
	select {
	case err = <-done:
		if err != nil {
			t.Fatalf("actual late commit was hidden: %v", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("actual late commit did not return")
	}
	if got := app.Service.Preferences.Snapshot(); got.Revision != 2 || got.Theme != theme {
		t.Fatalf("actual commit lost: %+v", got)
	}
	if err = app.Close(t.Context()); err != nil || !app.closed || selected.closes.Load() != 1 {
		t.Fatalf("actual cleanup retry failed: %v", err)
	}
	reopened, err := LocalStores(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := errors.Join(reopened.Product.Close(), reopened.Sessions.Close()); err != nil {
			t.Error(err)
		}
	}()
	if got, err := reopened.Product.Preferences(t.Context()); err != nil || got.Revision != 2 || got.Theme != theme {
		t.Fatalf("late write duplicated or lost on reopen: %+v %v", got, err)
	}
}

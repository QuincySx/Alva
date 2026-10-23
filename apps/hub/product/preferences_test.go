// INPUT: Controlled preference storage ports, contexts and concurrent observers.
// OUTPUT: Nonblocking committed snapshots, serialized I/O and joined cancellation/shutdown regressions.
// POS: [L7 product tests] Verifies one shared preference owner without opening a backend or runtime.
package product

import (
	"context"
	"errors"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/platform/storage"
)

type preferencePort struct {
	read   func(context.Context) (hub.Preferences, error)
	update func(context.Context, hub.PreferenceUpdate) (hub.Preferences, error)
}

func (p *preferencePort) Preferences(ctx context.Context) (hub.Preferences, error) {
	return p.read(ctx)
}

func (p *preferencePort) UpdatePreferences(ctx context.Context, update hub.PreferenceUpdate) (hub.Preferences, error) {
	return p.update(ctx, update)
}

type preferenceResult struct {
	value hub.Preferences
	err   error
}

func receivePreferences(t *testing.T, results <-chan preferenceResult) preferenceResult {
	t.Helper()
	select {
	case result := <-results:
		return result
	case <-time.After(2 * time.Second):
		t.Fatal("preference operation did not finish")
		return preferenceResult{}
	}
}

func TestPreferencesSnapshotDoesNotWaitForStorageAndQueuedReadCancels(t *testing.T) {
	committed := hub.DefaultPreferences()
	entered, release := make(chan struct{}), make(chan struct{})
	var reads, active atomic.Int32
	port := &preferencePort{
		read: func(context.Context) (hub.Preferences, error) {
			reads.Add(1)
			if active.Add(1) != 1 {
				active.Add(-1)
				return hub.Preferences{}, errors.New("overlapping preference I/O")
			}
			defer active.Add(-1)
			return committed, nil
		},
		update: func(ctx context.Context, update hub.PreferenceUpdate) (hub.Preferences, error) {
			if active.Add(1) != 1 {
				active.Add(-1)
				return hub.Preferences{}, errors.New("overlapping preference I/O")
			}
			defer active.Add(-1)
			close(entered)
			select {
			case <-ctx.Done():
				return hub.Preferences{}, ctx.Err()
			case <-release:
			}
			next, err := committed.Apply(update)
			if err == nil {
				committed = next
			}
			return next, err
		},
	}
	owner, err := NewPreferencesService(t.Context(), port)
	if err != nil {
		t.Fatal(err)
	}
	theme := "dark"
	updated := make(chan preferenceResult, 1)
	go func() {
		value, err := owner.Update(t.Context(), hub.PreferenceUpdate{ExpectedRevision: 1, Theme: &theme})
		updated <- preferenceResult{value, err}
	}()
	select {
	case <-entered:
	case <-time.After(2 * time.Second):
		t.Fatal("update did not reach the backend")
	}
	snapshots := make(chan preferenceResult, 1)
	go func() { snapshots <- preferenceResult{value: owner.Snapshot()} }()
	if got := receivePreferences(t, snapshots); got.value != hub.DefaultPreferences() {
		t.Fatalf("uncommitted snapshot: %+v", got)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 50*time.Millisecond)
	defer cancel()
	canceled := make(chan preferenceResult, 1)
	go func() {
		value, err := owner.Read(ctx)
		canceled <- preferenceResult{value, err}
	}()
	if got := receivePreferences(t, canceled); !errors.Is(got.err, context.DeadlineExceeded) || got.value != (hub.Preferences{}) || reads.Load() != 1 {
		t.Fatalf("queued read reached storage or ignored cancellation: %+v reads=%d", got, reads.Load())
	}
	refreshed := make(chan preferenceResult, 1)
	go func() {
		value, err := owner.Read(t.Context())
		refreshed <- preferenceResult{value, err}
	}()
	close(release)
	for _, results := range []<-chan preferenceResult{updated, refreshed} {
		if got := receivePreferences(t, results); got.err != nil || got.value.Revision != 2 || got.value.Theme != theme {
			t.Fatalf("serialized result: %+v", got)
		}
	}
	if owner.Snapshot() != committed || reads.Load() != 2 {
		t.Fatal("committed update was lost")
	}
}

func TestPreferencesFailuresNeverReplaceCommittedSnapshot(t *testing.T) {
	baseline := hub.Preferences{Revision: 7, Theme: "dark", Density: "compact", FontSize: 16}
	backendFailure := errors.New("selected backend failed")
	stale := baseline
	stale.Revision--
	inconsistent := baseline
	inconsistent.Theme = "light"
	invalid := baseline
	invalid.FontSize = 99
	for _, test := range []struct {
		name  string
		value hub.Preferences
		err   error
	}{
		{"backend failure", hub.Preferences{}, backendFailure},
		{"older revision", stale, nil},
		{"same revision changed values", inconsistent, nil},
		{"invalid stored value", invalid, nil},
	} {
		for _, operation := range []string{"read", "update"} {
			t.Run(test.name+"/"+operation, func(t *testing.T) {
				port := &preferencePort{read: func(context.Context) (hub.Preferences, error) { return baseline, nil }}
				owner, err := NewPreferencesService(t.Context(), port)
				if err != nil {
					t.Fatal(err)
				}
				port.read = func(context.Context) (hub.Preferences, error) { return test.value, test.err }
				port.update = func(context.Context, hub.PreferenceUpdate) (hub.Preferences, error) { return test.value, test.err }
				var result hub.Preferences
				if operation == "read" {
					result, err = owner.Read(t.Context())
				} else {
					theme := "light"
					result, err = owner.Update(t.Context(), hub.PreferenceUpdate{ExpectedRevision: baseline.Revision, Theme: &theme})
				}
				if err == nil || result != (hub.Preferences{}) || owner.Snapshot() != baseline {
					t.Fatalf("failure replaced committed cache: result=%+v snapshot=%+v err=%v", result, owner.Snapshot(), err)
				}
				if test.err != nil && !errors.Is(err, test.err) {
					t.Fatalf("backend error lost: %v", err)
				}
				if test.err == nil && errors.Is(err, hub.ErrInvalidPreferences) {
					t.Fatal("backend snapshot failure was classified as invalid caller input")
				}
			})
		}
	}
}

func TestPreferencesCancellationReachesAdmittedStorage(t *testing.T) {
	for _, operation := range []string{"read", "update"} {
		t.Run(operation, func(t *testing.T) {
			baseline := hub.DefaultPreferences()
			port := &preferencePort{read: func(context.Context) (hub.Preferences, error) { return baseline, nil }}
			owner, err := NewPreferencesService(t.Context(), port)
			if err != nil {
				t.Fatal(err)
			}
			entered := make(chan struct{})
			wait := func(ctx context.Context) (hub.Preferences, error) {
				close(entered)
				<-ctx.Done()
				return hub.Preferences{}, ctx.Err()
			}
			port.read = wait
			port.update = func(ctx context.Context, _ hub.PreferenceUpdate) (hub.Preferences, error) { return wait(ctx) }
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			results := make(chan preferenceResult, 1)
			go func() {
				var value hub.Preferences
				var err error
				if operation == "read" {
					value, err = owner.Read(ctx)
				} else {
					value, err = owner.Update(ctx, hub.PreferenceUpdate{})
				}
				results <- preferenceResult{value, err}
			}()
			select {
			case <-entered:
			case <-time.After(2 * time.Second):
				t.Fatal("operation did not reach selected storage")
			}
			cancel()
			if got := receivePreferences(t, results); !errors.Is(got.err, context.Canceled) || got.value != (hub.Preferences{}) || owner.Snapshot() != baseline {
				t.Fatalf("canceled operation changed cache: %+v", got)
			}
		})
	}
}

func TestPreferencesConstructionRequiresSuccessfulSelectedStore(t *testing.T) {
	var missing *preferencePort
	if owner, err := NewPreferencesService(t.Context(), missing); owner != nil || err == nil {
		t.Fatal("typed-nil storage accepted")
	}
	failure := errors.New("selected storage unavailable")
	port := &preferencePort{read: func(context.Context) (hub.Preferences, error) { return hub.Preferences{}, failure }}
	if owner, err := NewPreferencesService(t.Context(), port); owner != nil || !errors.Is(err, failure) {
		t.Fatal("failed selected storage was replaced by defaults", owner, err)
	}
	port.read = func(context.Context) (hub.Preferences, error) { return hub.Preferences{}, nil }
	if owner, err := NewPreferencesService(t.Context(), port); owner != nil || err == nil {
		t.Fatal("invalid initial storage snapshot accepted")
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	port.read = func(context.Context) (hub.Preferences, error) {
		t.Fatal("canceled construction reached storage")
		return hub.Preferences{}, nil
	}
	if owner, err := NewPreferencesService(ctx, port); owner != nil || !errors.Is(err, context.Canceled) {
		t.Fatal("construction ignored cancellation", owner, err)
	}
}

func TestPreferencesCloseCancelsAndJoinsAdmittedStorage(t *testing.T) {
	for _, operation := range []string{"read", "update"} {
		t.Run(operation, func(t *testing.T) {
			baseline := hub.DefaultPreferences()
			port := &preferencePort{read: func(context.Context) (hub.Preferences, error) { return baseline, nil }}
			owner, err := NewPreferencesService(t.Context(), port)
			if err != nil {
				t.Fatal(err)
			}
			entered := make(chan struct{})
			wait := func(ctx context.Context) (hub.Preferences, error) {
				close(entered)
				<-ctx.Done()
				return hub.Preferences{}, ctx.Err()
			}
			port.read = wait
			port.update = func(ctx context.Context, _ hub.PreferenceUpdate) (hub.Preferences, error) { return wait(ctx) }
			results := make(chan preferenceResult, 1)
			go func() {
				var value hub.Preferences
				var err error
				if operation == "read" {
					value, err = owner.Read(t.Context())
				} else {
					value, err = owner.Update(t.Context(), hub.PreferenceUpdate{})
				}
				results <- preferenceResult{value, err}
			}()
			select {
			case <-entered:
			case <-time.After(2 * time.Second):
				t.Fatal("operation did not reach selected storage")
			}
			ctx, cancel := context.WithTimeout(t.Context(), time.Second)
			defer cancel()
			if err := owner.Close(ctx); err != nil {
				t.Fatal("Close did not join canceled storage", err)
			}
			if got := receivePreferences(t, results); !errors.Is(got.err, context.Canceled) || got.value != (hub.Preferences{}) || owner.Snapshot() != baseline {
				t.Fatalf("closed operation changed cache: %+v", got)
			}
			if _, err := owner.Read(t.Context()); !errors.Is(err, storage.ErrClosed) {
				t.Fatal("closed owner admitted a read", err)
			}
			if _, err := owner.Update(t.Context(), hub.PreferenceUpdate{}); !errors.Is(err, storage.ErrClosed) {
				t.Fatal("closed owner admitted an update", err)
			}
			if err := owner.Close(t.Context()); err != nil {
				t.Fatal("completed Close was not idempotent", err)
			}
		})
	}
}

func TestPreferencesCloseTimeoutRetainsActualWriteUntilRetry(t *testing.T) {
	baseline := hub.DefaultPreferences()
	theme := "dark"
	update := hub.PreferenceUpdate{ExpectedRevision: baseline.Revision, Theme: &theme}
	committed, err := baseline.Apply(update)
	if err != nil {
		t.Fatal(err)
	}
	entered := make(chan context.Context, 1)
	release := make(chan struct{})
	defer close(release)
	var reads, writes atomic.Int32
	port := &preferencePort{
		read: func(context.Context) (hub.Preferences, error) { reads.Add(1); return baseline, nil },
		update: func(ctx context.Context, _ hub.PreferenceUpdate) (hub.Preferences, error) {
			writes.Add(1)
			entered <- ctx
			// This backend completes a physical commit even after cancellation.
			// The service must retain and join it, without replaying the write.
			<-release
			return committed, nil
		},
	}
	owner, err := NewPreferencesService(t.Context(), port)
	if err != nil {
		t.Fatal(err)
	}
	updated := make(chan preferenceResult, 1)
	go func() {
		value, err := owner.Update(t.Context(), update)
		updated <- preferenceResult{value, err}
	}()
	var admitted context.Context
	select {
	case admitted = <-entered:
	case <-time.After(2 * time.Second):
		t.Fatal("write did not reach storage")
	}
	queued := make(chan preferenceResult, 1)
	go func() {
		value, err := owner.Read(t.Context())
		queued <- preferenceResult{value, err}
	}()
	for attempt := range 2 {
		ctx, cancel := context.WithTimeout(t.Context(), 25*time.Millisecond)
		err := owner.Close(ctx)
		cancel()
		if !errors.Is(err, context.DeadlineExceeded) || !errors.Is(admitted.Err(), context.Canceled) {
			t.Fatalf("close attempt %d lost or failed to cancel actual write: %v backend=%v", attempt, err, admitted.Err())
		}
		if _, err = owner.Read(t.Context()); !errors.Is(err, storage.ErrClosed) {
			t.Fatal("timed-out Close admitted another read", err)
		}
		if _, err = owner.Update(t.Context(), update); !errors.Is(err, storage.ErrClosed) {
			t.Fatal("timed-out Close replayed an update", err)
		}
		if owner.Snapshot() != baseline || reads.Load() != 1 || writes.Load() != 1 {
			t.Fatal("unfinished write changed snapshot or new I/O was admitted")
		}
	}
	if got := receivePreferences(t, queued); !errors.Is(got.err, storage.ErrClosed) || got.value != (hub.Preferences{}) {
		t.Fatalf("queued observer did not leave on Close: %+v", got)
	}
	// Sending releases exactly this physical write, while deferred close also
	// makes a failed assertion release the backend instead of leaking its owner.
	release <- struct{}{}
	if got := receivePreferences(t, updated); got.err != nil || got.value != committed || owner.Snapshot() != committed {
		t.Fatalf("late committed success was hidden or lost: %+v snapshot=%+v", got, owner.Snapshot())
	}
	if err := owner.Close(t.Context()); err != nil {
		t.Fatal("Close retry did not join completed write", err)
	}
	if err := owner.Close(t.Context()); err != nil || reads.Load() != 1 || writes.Load() != 1 {
		t.Fatal("Close replayed physical work", err)
	}
}

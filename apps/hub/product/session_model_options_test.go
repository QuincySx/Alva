// INPUT: Real product SQLite, explicit fifth-engine descriptors and delayed or forged save receipts.
// OUTPUT: Declared capability enforcement, explicit unavailable-owner errors and joined close regressions.
// POS: [L7 product service tests] Session edits do not execute, silently default or outlive their owner.
package product

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage"
)

type sessionOptionsFaultStore struct {
	ServiceStore
	entered, finish chan struct{}
	forge           bool
}

func (s *sessionOptionsFaultStore) SaveSessionModelOptions(ctx context.Context, edit hub.SessionModelOptionsUpdate) (hub.SessionModelOptionsSnapshot, error) {
	if s.entered != nil {
		close(s.entered)
		<-s.finish
		if err := ctx.Err(); err != nil {
			return hub.SessionModelOptionsSnapshot{}, err
		}
	}
	value, err := s.ServiceStore.SaveSessionModelOptions(ctx, edit)
	if s.forge && err == nil {
		value.Entries = nil
	}
	return value, err
}

func sessionOptionsService(t *testing.T) (*HubService, hub.Session, hub.Target) {
	t.Helper()
	s, _ := fixture(t)
	var err error
	s.Connections, err = NewConnectionsService(t.Context(), s.Store.(hub.ConnectionsStore))
	if err != nil {
		t.Fatal(err)
	}
	owner := s.Connections
	t.Cleanup(func() { _ = owner.Close(context.Background()) })
	d, _ := s.Runtime.Descriptor("alva")
	d.Config = engine.ConfigSurface{ProviderProtocol: "fixture", SupportsGeneration: true, SupportsSystemPrompt: true, SupportsMaxTurns: true}
	if err = s.Runtime.RegisterDescriptor("fifth-engine", d); err != nil {
		t.Fatal(err)
	}
	route := s.DefaultTarget
	route.EngineID = "fifth-engine"
	s.Targets = append(s.Targets, route)
	session, err := s.Create(t.Context(), "session options", "")
	if err != nil {
		t.Fatal(err)
	}
	return s, session, route
}

func TestSessionModelOptionsServiceUsesActualFifthEngineAndRejectsUnknownReceipt(t *testing.T) {
	s, session, target := sessionOptionsService(t)
	temperature := .7
	options := hub.ModelOptions{Generation: engine.GenerationOptions{Temperature: &temperature}, SystemPrompt: "session only", MaxTurns: 2}
	edit := hub.SessionModelOptionsUpdate{SessionID: session.ID, BranchID: session.ActiveBranchID, Target: target, Options: &options}
	value, err := s.SaveSessionModelOptions(t.Context(), edit)
	if err != nil || value.Revision != 1 {
		t.Fatal("fifth engine declaration ignored", value, err)
	}
	d, _ := s.Runtime.Descriptor("alva")
	d.Config = engine.ConfigSurface{ProviderProtocol: "fixture"}
	if err = s.Runtime.RegisterDescriptor("unsupported-engine", d); err != nil {
		t.Fatal(err)
	}
	unsupported := target
	unsupported.EngineID = "unsupported-engine"
	s.Targets = append(s.Targets, unsupported)
	edit.Target, edit.ExpectedRevision = unsupported, value.Revision
	if _, err = s.SaveSessionModelOptions(t.Context(), edit); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal("undeclared options saved", err)
	}
	if s.Runtime.HasActive() {
		t.Fatal("save executed an engine")
	}
	edit.Target, edit.ExpectedRevision = target, 0
	if _, err = s.SaveSessionModelOptions(t.Context(), edit); !errors.Is(err, hub.ErrSessionModelOptionsConflict) {
		t.Fatal("stale revision accepted", err)
	}
	edit.ExpectedRevision = value.Revision
	s.Store = &sessionOptionsFaultStore{ServiceStore: s.Store, forge: true}
	options.SystemPrompt = "committed but receipt unknown"
	if _, err = s.SaveSessionModelOptions(t.Context(), edit); err == nil {
		t.Fatal("forged receipt reported success")
	}
	reloaded, err := s.SessionModelOptions(t.Context(), session.ID)
	if err != nil || reloaded.Revision != 2 {
		t.Fatal("explicit reload lost committed result", reloaded, err)
	}
	s.Connections = nil
	edit.Options, edit.ExpectedRevision = nil, reloaded.Revision
	if _, err = s.SaveSessionModelOptions(t.Context(), edit); err == nil {
		t.Fatal("missing owner silently cleared override")
	}
}

func TestSessionModelOptionsServiceCloseWaitsForPhysicalSave(t *testing.T) {
	s, session, target := sessionOptionsService(t)
	backend := &sessionOptionsFaultStore{ServiceStore: s.Store, entered: make(chan struct{}), finish: make(chan struct{})}
	s.Store = backend
	options := hub.ModelOptions{SystemPrompt: "pending"}
	result := make(chan error, 1)
	go func() {
		_, err := s.SaveSessionModelOptions(context.Background(), hub.SessionModelOptionsUpdate{SessionID: session.ID, BranchID: session.ActiveBranchID, Target: target, Options: &options})
		result <- err
	}()
	<-backend.entered
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Millisecond)
	defer cancel()
	if err := s.Connections.Close(ctx); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("close released an unfinished save", err)
	}
	if _, err := s.SessionModelOptions(t.Context(), session.ID); !errors.Is(err, storage.ErrClosed) {
		t.Fatal("closed owner accepted a read", err)
	}
	close(backend.finish)
	if err := <-result; !errors.Is(err, context.Canceled) {
		t.Fatal("close did not cancel saved operation", err)
	}
	if err := s.Connections.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
}

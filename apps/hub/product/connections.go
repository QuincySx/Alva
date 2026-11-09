// INPUT: Explicit connection storage, shared connection policy and native provider constructors.
// OUTPUT: Shared committed snapshots, request-bound create/recovery, CRUD, exact route resolution with its declared context window and joined shutdown.
// POS: [L7 product service] Owns connection configuration, explicit catalog probes and joined lifecycle without account discovery.
package product

import (
	"context"
	"errors"
	"strings"
	"sync"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/platform/storage"
)

type ConnectionsService struct {
	store      hub.ConnectionsStore
	access     chan struct{}
	mu         sync.RWMutex
	current    []hub.ModelConnection
	modelPrefs hub.ModelPreferencesSnapshot
	closed     bool
	stopped    chan struct{}
	cancel     context.CancelFunc
	done       chan struct{}
}

type connectionStorageError struct {
	operation string
	cause     error
}

func (e connectionStorageError) Error() string {
	return e.operation + " model connection storage failed"
}
func (e connectionStorageError) Unwrap() error { return e.cause }
func connectionError(operation string, err error) error {
	if err == nil {
		return nil
	}
	switch {
	case errors.Is(err, hub.ErrConnectionConflict):
		return hub.ErrConnectionConflict
	case errors.Is(err, hub.ErrConnectionCreationConflict):
		return hub.ErrConnectionCreationConflict
	case errors.Is(err, hub.ErrModelPreferenceConflict):
		return hub.ErrModelPreferenceConflict
	case errors.Is(err, hub.ErrNotFound):
		return hub.ErrNotFound
	case errors.Is(err, context.Canceled):
		return context.Canceled
	case errors.Is(err, context.DeadlineExceeded):
		return context.DeadlineExceeded
	}
	return connectionStorageError{operation, err}
}

func NewConnectionsService(ctx context.Context, store hub.ConnectionsStore) (*ConnectionsService, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if storage.Missing(store) {
		return nil, errors.New("application model connection storage required")
	}
	value, err := store.Connections(ctx)
	if err != nil {
		return nil, connectionError("read", err)
	}
	prefs, err := store.ModelPreferences(ctx)
	if err != nil {
		return nil, connectionError("read model preferences", err)
	}
	if err = prefs.Validate(); err != nil {
		return nil, err
	}
	s := &ConnectionsService{store: store, access: make(chan struct{}, 1), stopped: make(chan struct{})}
	s.modelPrefs = prefs.Clone()
	s.access <- struct{}{}
	if err = s.publish(value); err != nil {
		return nil, err
	}
	return s, nil
}
func (s *ConnectionsService) Snapshot() []hub.ModelConnection {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return cloneConnections(s.current)
}
func cloneConnections(value []hub.ModelConnection) []hub.ModelConnection {
	out := make([]hub.ModelConnection, len(value))
	for i, c := range value {
		out[i] = c.Clone()
	}
	return out
}
func (s *ConnectionsService) Connection(id string) (hub.ModelConnection, bool) {
	for _, c := range s.Snapshot() {
		if c.ID == id {
			return c, true
		}
	}
	return hub.ModelConnection{}, false
}
func (s *ConnectionsService) publish(value []hub.ModelConnection) error {
	if len(value) > 128 {
		return errors.New("connection store returned too many entries")
	}
	seen := map[string]bool{}
	for _, c := range value {
		if c.Validate() != nil || seen[c.ID] {
			return errors.New("connection store returned an invalid snapshot")
		}
		seen[c.ID] = true
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, previous := range s.current {
		for _, next := range value {
			if previous.ID == next.ID && next.Revision < previous.Revision {
				return errors.New("connection store returned a stale snapshot")
			}
		}
	}
	s.current = cloneConnections(value)
	return nil
}
func (s *ConnectionsService) Read(ctx context.Context) ([]hub.ModelConnection, error) {
	ctx, err := s.acquire(ctx)
	if err != nil {
		return nil, err
	}
	defer s.release()
	value, err := s.store.Connections(ctx)
	if err != nil {
		return nil, connectionError("read", err)
	}
	if err = s.publish(value); err != nil {
		return nil, err
	}
	return cloneConnections(value), nil
}
func connectionEdit(c hub.ModelConnection) hub.ConnectionUpdate {
	return hub.ConnectionUpdate{ID: c.ID, ExpectedRevision: c.Revision, Name: c.Name, Protocol: c.Protocol, Endpoint: c.Endpoint, CatalogURL: c.CatalogURL, Auth: c.Auth, Enabled: c.Enabled, Models: append([]hub.ConnectionModel(nil), c.Models...)}
}
func (s *ConnectionsService) Save(ctx context.Context, u hub.ConnectionUpdate) (hub.ModelConnection, error) {
	ctx, err := s.acquire(ctx)
	if err != nil {
		return hub.ModelConnection{}, err
	}
	defer s.release()
	return s.save(ctx, u)
}

// Create retains a caller-owned identity across uncertain commits. It never
// falls back to non-idempotent creation when the selected storage lacks the port.
func (s *ConnectionsService) Create(ctx context.Context, request hub.ConnectionCreate) (hub.ModelConnection, error) {
	if err := request.Validate(); err != nil {
		return hub.ModelConnection{}, err
	}
	store, ok := s.store.(hub.ConnectionCreationStore)
	if !ok || storage.Missing(store) {
		return hub.ModelConnection{}, errors.New("recoverable connection creation is unavailable for the selected storage")
	}
	ctx, err := s.acquire(ctx)
	if err != nil {
		return hub.ModelConnection{}, err
	}
	defer s.release()
	return s.saveWith(ctx, request.Update, func(ctx context.Context, _ hub.ConnectionUpdate) (hub.ModelConnection, error) {
		return store.CreateConnection(ctx, request)
	})
}

// ResolveCreation observes the exact original receipt and its current identity.
// It never matches names, recreates a deleted entry or retries a write.
func (s *ConnectionsService) ResolveCreation(ctx context.Context, requestID string) (hub.ConnectionCreationReceipt, error) {
	store, ok := s.store.(hub.ConnectionCreationStore)
	if !ok || storage.Missing(store) {
		return hub.ConnectionCreationReceipt{}, errors.New("connection creation recovery is unavailable for the selected storage")
	}
	ctx, err := s.acquire(ctx)
	if err != nil {
		return hub.ConnectionCreationReceipt{}, err
	}
	defer s.release()
	receipt, err := store.ConnectionCreation(ctx, requestID)
	if err != nil {
		return hub.ConnectionCreationReceipt{}, connectionError("resolve creation", err)
	}
	if err = receipt.Validate(); err != nil || receipt.RequestID != requestID {
		return hub.ConnectionCreationReceipt{}, errors.New("connection storage returned an invalid creation receipt")
	}
	values := s.Snapshot()
	out := values[:0]
	for _, value := range values {
		if value.ID != receipt.Created.ID {
			out = append(out, value)
		}
	}
	if receipt.Current != nil {
		out = append(out, receipt.Current.Clone())
	}
	if err = s.publish(out); err != nil {
		return hub.ConnectionCreationReceipt{}, err
	}
	return receipt.Clone(), nil
}

func (s *ConnectionsService) save(ctx context.Context, u hub.ConnectionUpdate) (hub.ModelConnection, error) {
	return s.saveWith(ctx, u, s.store.SaveConnection)
}

func (s *ConnectionsService) saveWith(ctx context.Context, u hub.ConnectionUpdate, commit func(context.Context, hub.ConnectionUpdate) (hub.ModelConnection, error)) (hub.ModelConnection, error) {
	var current hub.ModelConnection
	var secret hub.ConnectionSecrets
	var err error
	if u.ID != "" {
		var found bool
		current, found = s.Connection(u.ID)
		if !found {
			return hub.ModelConnection{}, hub.ErrNotFound
		}
		if current.Revision != u.ExpectedRevision {
			return hub.ModelConnection{}, hub.ErrConnectionConflict
		}
		secret, err = s.store.ConnectionSecrets(ctx, u.ID, u.ExpectedRevision)
		if err != nil {
			return hub.ModelConnection{}, connectionError("read", err)
		}
	} else {
		if u.ExpectedRevision != 0 {
			return hub.ModelConnection{}, hub.ErrConnectionConflict
		}
		current.ID = "connection-validation"
	}
	check := u
	if check.ID == "" {
		check.ID = current.ID
	}
	validated, secret, err := current.Apply(check, secret)
	if err != nil {
		return hub.ModelConnection{}, err
	}
	models := validated.Models
	if len(models) == 0 {
		models = []hub.ConnectionModel{{ID: "connection-validation"}}
	}
	for _, model := range models {
		if _, err = provider.Build(provider.Kind(validated.Protocol), provider.Config{Endpoint: validated.ModelEndpoint(model.ID), Model: model.ID, APIKey: secret.APIKey, Headers: secret.Headers}); err != nil {
			return hub.ModelConnection{}, errors.New("model connection transport configuration is invalid")
		}
	}
	saved, err := commit(ctx, u)
	if err != nil {
		return hub.ModelConnection{}, connectionError("save", err)
	}
	if saved.Validate() != nil || saved.ID == "" || saved.Revision != validated.Revision || u.ID != "" && saved.ID != u.ID || !saved.SameConfiguration(validated) {
		return hub.ModelConnection{}, errors.New("connection store returned an invalid save receipt; reload before retrying")
	}
	values := s.Snapshot()
	replaced := false
	for i, c := range values {
		if c.ID == saved.ID {
			values[i] = saved
			replaced = true
			break
		}
	}
	if !replaced {
		values = append(values, saved)
	}
	if err = s.publish(values); err != nil {
		return hub.ModelConnection{}, err
	}
	return saved.Clone(), nil
}
func (s *ConnectionsService) SetEnabled(ctx context.Context, id string, expected uint64, enabled bool) (hub.ModelConnection, error) {
	ctx, err := s.acquire(ctx)
	if err != nil {
		return hub.ModelConnection{}, err
	}
	defer s.release()
	c, ok := s.Connection(id)
	if !ok {
		return hub.ModelConnection{}, hub.ErrNotFound
	}
	u := connectionEdit(c)
	u.ExpectedRevision = expected
	u.Enabled = enabled
	return s.save(ctx, u)
}
func (s *ConnectionsService) Delete(ctx context.Context, id string, expected uint64) error {
	ctx, err := s.acquire(ctx)
	if err != nil {
		return err
	}
	defer s.release()
	if err = s.store.DeleteConnection(ctx, id, expected); err != nil {
		return connectionError("delete", err)
	}
	values := s.Snapshot()
	out := values[:0]
	for _, c := range values {
		if c.ID != id {
			out = append(out, c)
		}
	}
	return s.publish(out)
}
func (s *ConnectionsService) Targets() []hub.Target {
	var targets []hub.Target
	for _, c := range s.Snapshot() {
		if c.Enabled {
			for _, m := range c.Models {
				if m.Enabled {
					targets = append(targets, c.Target(m))
				}
			}
		}
	}
	return targets
}

// ResolveTarget consumes the exact committed connection revision. Active attempts
// keep their immutable copy; later edits, disable or deletion affect the next attempt.
func (s *ConnectionsService) ResolveTarget(ctx context.Context, target hub.Target) (provider.Kind, provider.Config, bool, error) {
	ctx, err := s.acquire(ctx)
	if err != nil {
		return "", provider.Config{}, false, err
	}
	defer s.release()
	for _, c := range s.Snapshot() {
		for _, m := range c.Models {
			if hub.RouteKey(c.Target(m)) != hub.RouteKey(target) {
				continue
			}
			if !c.Enabled || !m.Enabled {
				return "", provider.Config{}, false, errors.New("model connection or model is disabled")
			}
			if target.ProviderID != c.Protocol {
				return "", provider.Config{}, false, errors.New("model connection protocol changed; select a current route")
			}
			secret, err := s.store.ConnectionSecrets(ctx, c.ID, c.Revision)
			if err != nil {
				return "", provider.Config{}, false, connectionError("resolve", err)
			}
			// Revalidate private storage values using the shared pure edit policy.
			// The actual consumer constructs the provider once from this recipe.
			if _, secret, err = c.Apply(connectionEdit(c), secret); err != nil {
				return "", provider.Config{}, false, errors.New("saved model connection configuration is invalid")
			}
			config := provider.Config{Endpoint: c.ModelEndpoint(m.ID), Model: m.ID, APIKey: secret.APIKey, Headers: secret.Headers}
			return provider.Kind(c.Protocol), config, m.ImageCapable, nil
		}
	}
	return "", provider.Config{}, false, errors.New("saved model connection route changed or was deleted; select a current route")
}

// ResolveRoute revalidates a saved connection route against the newest
// connection: it must still exist, be enabled with the model enabled, and keep
// its protocol. Cosmetic edits and endpoint moves are followed, not refused.
func (s *ConnectionsService) ResolveRoute(ctx context.Context, target hub.Target) (hub.RouteFacts, error) {
	if !IsConnectionTarget(target) {
		return hub.RouteFacts{Target: target}, nil
	}
	if s == nil {
		return hub.RouteFacts{}, errors.New("model connection service is unavailable")
	}
	for _, c := range s.Snapshot() {
		for _, m := range c.Models {
			if hub.RouteKey(c.Target(m)) != hub.RouteKey(target) {
				continue
			}
			if !c.Enabled || !m.Enabled {
				return hub.RouteFacts{}, errors.New("model connection or model is disabled; select a current route")
			}
			if target.ProviderID != c.Protocol {
				return hub.RouteFacts{}, errors.New("model connection protocol changed; select a current route")
			}
			fresh := c.Target(m)
			fresh.AutomaticAuthorized = target.AutomaticAuthorized
			fresh.RequiredCapabilities = append([]string(nil), target.RequiredCapabilities...)
			return hub.RouteFacts{Target: fresh, Revision: c.Revision, Endpoint: fresh.BillingRoute}, nil
		}
	}
	return hub.RouteFacts{}, errors.New("saved model connection route was deleted or its model removed; select a current route")
}

// ContextWindow is the declared context window of an exact saved route: the
// catalog's explicit statement, else the operator's entry, else 0 (undeclared).
func (s *ConnectionsService) ContextWindow(target hub.Target) int {
	if s == nil {
		return 0
	}
	for _, c := range s.Snapshot() {
		for _, m := range c.Models {
			if hub.RouteKey(c.Target(m)) == hub.RouteKey(target) {
				return m.DeclaredContextWindow()
			}
		}
	}
	return 0
}

// AvailableTargets is a cold observation of configured and committed dynamic routes.
func (s *HubService) AvailableTargets() []hub.Target {
	out := append([]hub.Target(nil), s.Targets...)
	if s.Connections != nil {
		out = append(out, s.Connections.Targets()...)
	}
	return out
}
func IsConnectionTarget(target hub.Target) bool {
	return target.EngineID == "alva" && strings.HasPrefix(target.AccountSource, "connection:")
}

func (s *ConnectionsService) acquire(ctx context.Context) (context.Context, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-s.stopped:
		return nil, storage.ErrClosed
	case <-s.access:
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		s.access <- struct{}{}
		return nil, storage.ErrClosed
	}
	if err := ctx.Err(); err != nil {
		s.access <- struct{}{}
		return nil, err
	}
	owned, cancel := context.WithCancel(ctx)
	s.cancel = cancel
	s.done = make(chan struct{})
	return owned, nil
}
func (s *ConnectionsService) release() {
	s.mu.Lock()
	s.cancel()
	s.cancel = nil
	close(s.done)
	s.done = nil
	s.mu.Unlock()
	s.access <- struct{}{}
}
func (s *ConnectionsService) Close(ctx context.Context) error {
	s.mu.Lock()
	if !s.closed {
		s.closed = true
		close(s.stopped)
	}
	cancel, done := s.cancel, s.done
	s.mu.Unlock()
	if cancel == nil {
		return nil
	}
	cancel()
	select {
	case <-done:
		return nil
	default:
	}
	select {
	case <-done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

// INPUT: Operator-frozen model routes, explicit local start consent and owned HTTP resources
// OUTPUT: Generation-bound local Gateway lifecycle, cold closed/stopping status and one-start-only Credentials
// POS: [L6 application] Embeddable loopback model API reuses the standalone Gateway without another Agent runtime.
package gateway

import (
	"context"
	"errors"
	"net"
	"net/http"
	"sort"
	"sync"
	"time"

	"github.com/QuincySx/Alva/platform/system/loopback"
)

// LocalSnapshot keeps Running true while a stopped-admission listener still
// owns unfinished cleanup. Generation identifies that exact owner and is zero
// when no listener remains; Closed permanently forbids another Start.
type LocalSnapshot struct {
	Generation uint64   `json:"generation"`
	Closed     bool     `json:"closed"`
	Stopping   bool     `json:"stopping"`
	Running    bool     `json:"running"`
	Address    string   `json:"address,omitempty"`
	Models     []string `json:"models"`
}

// Credentials are returned only by explicit Start. Status and stored settings
// never expose the bearer; a restart creates a fresh unrelated secret.
type Credentials struct {
	Generation uint64 `json:"generation"`
	Address    string `json:"address"`
	Token      string `json:"token"`
}
type localRun struct {
	gateway    *Server
	http       *http.Server
	done       chan error
	address    string
	generation uint64
	stopping   bool
}
type Local struct {
	mu         sync.Mutex
	control    chan struct{}
	routes     map[string]Route
	current    *localRun
	closed     bool
	generation uint64
}

func NewLocal(routes map[string]Route) (*Local, error) {
	// Validate a frozen copy without listening or sending a model request.
	owned := make(map[string]Route, len(routes))
	for alias, v := range routes {
		headers := make(map[string]string, len(v.Provider.Headers))
		for k, value := range v.Provider.Headers {
			headers[k] = value
		}
		v.Provider.Headers = headers
		owned[alias] = v
	}
	token, err := loopback.NewToken()
	if err != nil {
		return nil, err
	}
	probe, err := New(Config{Authority: "127.0.0.1:1", Token: token, Routes: owned})
	if err != nil {
		return nil, err
	}
	if err = probe.Close(context.Background()); err != nil {
		return nil, err
	}
	return &Local{routes: owned, control: make(chan struct{}, 1)}, nil
}
func (l *Local) Snapshot() LocalSnapshot {
	l.mu.Lock()
	defer l.mu.Unlock()
	value := LocalSnapshot{Closed: l.closed, Models: make([]string, 0, len(l.routes))}
	for alias := range l.routes {
		value.Models = append(value.Models, alias)
	}
	sort.Strings(value.Models)
	if l.current != nil {
		value.Running = true
		value.Generation = l.current.generation
		value.Stopping = l.current.stopping
		value.Address = l.current.address
	}
	return value
}
func (l *Local) Start(ctx context.Context, approved bool) (Credentials, error) {
	if !approved {
		return Credentials{}, errors.New("explicit local model API/account usage consent required")
	}
	if err := ctx.Err(); err != nil {
		return Credentials{}, err
	}
	if err := l.enter(ctx); err != nil {
		return Credentials{}, err
	}
	defer func() { <-l.control }()
	l.mu.Lock()
	unavailable := l.closed || l.current != nil
	l.mu.Unlock()
	if unavailable {
		return Credentials{}, errors.New("local gateway is closed or already running")
	}
	token, err := loopback.NewToken()
	if err != nil {
		return Credentials{}, err
	}
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		return Credentials{}, errors.New("could not open numeric loopback gateway")
	}
	address := listener.Addr().String()
	handler, err := New(Config{Authority: address, Token: token, Routes: l.routes})
	if err != nil {
		listener.Close()
		return Credentials{}, err
	}
	httpServer := &http.Server{Handler: handler, ReadHeaderTimeout: 5 * time.Second, ReadTimeout: 30 * time.Second, IdleTimeout: 30 * time.Second, MaxHeaderBytes: 32 << 10}
	run := &localRun{gateway: handler, http: httpServer, done: make(chan error, 1), address: "http://" + address}
	l.mu.Lock()
	if l.closed {
		l.mu.Unlock()
		listener.Close()
		handler.Close(context.Background())
		return Credentials{}, errors.New("local gateway closed during admission")
	}
	l.generation++
	run.generation = l.generation
	l.current = run
	l.mu.Unlock()
	go func() { run.done <- httpServer.Serve(listener); close(run.done) }()
	return Credentials{Generation: run.generation, Address: run.address, Token: token}, nil
}
func (l *Local) stop(ctx context.Context, generation uint64) error {
	l.mu.Lock()
	run := l.current
	if generation != 0 && (run == nil || run.generation != generation) {
		l.mu.Unlock()
		return errors.New("local gateway listener changed; observe the current listener before stopping")
	}
	if run != nil {
		run.stopping = true
	}
	l.mu.Unlock()
	if run == nil {
		return nil
	}
	// Stop actual upstream handlers before HTTP draining. A timeout retains the
	// exact owner for retry; it never reopens admission or replays a model call.
	if err := run.gateway.Close(ctx); err != nil {
		return err
	}
	if err := run.http.Shutdown(ctx); err != nil {
		return err
	}
	select {
	case err := <-run.done:
		l.mu.Lock()
		if l.current == run {
			l.current = nil
		}
		l.mu.Unlock()
		if err != nil && !errors.Is(err, http.ErrServerClosed) {
			return errors.New("local gateway listener ended unexpectedly")
		}
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}
func (l *Local) Stop(ctx context.Context) error {
	if err := l.enter(ctx); err != nil {
		return err
	}
	defer func() { <-l.control }()
	return l.stop(ctx, 0)
}

// StopGeneration joins only the listener observed by the caller. A delayed UI
// action cannot close admission or cancel work belonging to a replacement.
func (l *Local) StopGeneration(ctx context.Context, generation uint64) error {
	if generation == 0 {
		return errors.New("explicit local gateway generation required")
	}
	if err := l.enter(ctx); err != nil {
		return err
	}
	defer func() { <-l.control }()
	return l.stop(ctx, generation)
}

func (l *Local) Close(ctx context.Context) error {
	l.mu.Lock()
	l.closed = true
	l.mu.Unlock()
	if err := l.enter(ctx); err != nil {
		return err
	}
	defer func() { <-l.control }()
	return l.stop(ctx, 0)
}
func (l *Local) enter(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	select {
	case l.control <- struct{}{}:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

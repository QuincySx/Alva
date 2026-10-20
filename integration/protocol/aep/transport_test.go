// INPUT: Explicit owned transport factories, native protocol helper and incomplete cleanup fixtures.
// OUTPUT: Exclusive transport selection, real handshake/shutdown and retained partial owner regressions.
// POS: [integration / external protocol] Transport injection preserves lifecycle ownership without protocol fallback.
package aep

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/platform/system/process"
)

func TestOwnedTransportDoesNotFallbackOrRunBeforeValidation(t *testing.T) {
	want := errors.New("explicit transport failed")
	starts := 0
	factory := func(context.Context) (Peer, error) { starts++; return nil, want }
	config := Config{Name: "fixture", PluginVersion: "1", Start: factory, Process: process.Config{Command: "must-not-run"}}
	if _, err := Open(t.Context(), config, nil, nil); err == nil || starts != 0 {
		t.Fatal("ambiguous execution was admitted", err, starts)
	}
	config.Process = process.Config{}
	config.PluginVersion = ""
	if _, err := Open(t.Context(), config, nil, nil); err == nil || starts != 0 {
		t.Fatal("factory preceded identity validation", err, starts)
	}
	config.PluginVersion = "1"
	if _, err := Open(t.Context(), config, nil, nil); !errors.Is(err, want) || starts != 1 {
		t.Fatal("factory failure was replaced/replayed", err, starts)
	}
	config.Start = func(context.Context) (Peer, error) { return (*incompletePeer)(nil), nil }
	if _, err := Open(t.Context(), config, nil, nil); err == nil {
		t.Fatal("typed nil transport accepted")
	}
}

func TestExplicitTransportRunsActualProtocolAndGracefulJoin(t *testing.T) {
	config := fixtureConfig(t, "normal")
	native := config.Process
	config.Process = process.Config{}
	starts := 0
	var peer *process.Process
	config.RequestedCapabilities = nil
	config.Start = func(context.Context) (Peer, error) {
		starts++
		var err error
		peer, err = process.Start(native)
		if err != nil {
			return nil, err
		}
		return nativePeer{peer}, nil
	}
	// The helper's manifest grants must match the explicit host ceiling.
	config.RequestedCapabilities = []string{"host:log"}
	client, err := Open(t.Context(), config, HandlerFunc(func(context.Context, string, json.RawMessage) (json.RawMessage, error) {
		return json.RawMessage(`null`), nil
	}), nil)
	if err != nil {
		t.Fatal(err)
	}
	ctx, stop := context.WithTimeout(context.Background(), 5*time.Second)
	defer stop()
	defer client.Close(ctx)
	if starts != 1 || client.Handshake().Plugin.Name != "fixture" {
		t.Fatal("actual transport handshake missing")
	}
	if err = client.Shutdown(ctx); err != nil || !client.CleanupComplete() || !peer.CleanupComplete() {
		t.Fatal("actual execution not joined", err)
	}
}

type incompletePeer struct {
	mu       sync.Mutex
	complete bool
	closes   int
}

func (*incompletePeer) Input() io.WriteCloser { return nil }
func (*incompletePeer) Output() io.ReadCloser { return nil }
func (*incompletePeer) Errors() io.ReadCloser { return nil }
func (*incompletePeer) BeginClose()           {}
func (p *incompletePeer) Close(context.Context) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.closes++
	if !p.complete {
		return context.DeadlineExceeded
	}
	return nil
}
func (*incompletePeer) Wait(context.Context) error { return nil }
func (*incompletePeer) ExitCode() (int, bool)      { return 0, false }
func (p *incompletePeer) CleanupComplete() bool    { p.mu.Lock(); defer p.mu.Unlock(); return p.complete }

func TestOpenRetainsOwnerUntilIncompleteTransportActuallyCloses(t *testing.T) {
	for _, typed := range []bool{false, true} {
		t.Run(map[bool]string{false: "nil-streams", true: "typed-nil-stream"}[typed], func(t *testing.T) {
			peer := &incompletePeer{}
			var endpoint Peer = peer
			if typed {
				endpoint = typedStreamPeer{peer}
			}
			client, err := Open(t.Context(), Config{Name: "fixture", PluginVersion: "1", Start: func(context.Context) (Peer, error) { return endpoint, nil }}, nil, nil)
			if client == nil || !errors.Is(err, context.DeadlineExceeded) || client.CleanupComplete() {
				t.Fatal("partial cleanup lost its owner", err)
			}
			peer.mu.Lock()
			peer.complete = true
			peer.mu.Unlock()
			if err = client.Close(t.Context()); err != nil || !client.CleanupComplete() {
				t.Fatal("retained owner could not finish cleanup", err)
			}
			peer.mu.Lock()
			defer peer.mu.Unlock()
			if peer.closes != 2 {
				t.Fatal("cleanup was not explicitly retried", peer.closes)
			}
		})
	}
}

type typedStreamPeer struct{ *incompletePeer }

func (typedStreamPeer) Input() io.WriteCloser { return (*io.PipeWriter)(nil) }
func (typedStreamPeer) Output() io.ReadCloser { return io.NopCloser(strings.NewReader("")) }
func (typedStreamPeer) Errors() io.ReadCloser { return io.NopCloser(strings.NewReader("")) }

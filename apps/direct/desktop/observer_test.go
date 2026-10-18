// INPUT:  context, testing, host.Controller and provider-independent test runners
// OUTPUT: observer detachment and expired-replay reconciliation regression tests
// POS: [L6 application]    Verifies that native observation never owns or replays execution
package desktop

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/platform/host"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type testRunner func(context.Context, []wire.Message, func(abi.Event) error) error

func (r testRunner) Run(ctx context.Context, input []wire.Message, emit func(abi.Event) error) error {
	return r(ctx, input, emit)
}

func TestNativeObserverDetachIsNotStop(t *testing.T) {
	started := make(chan struct{})
	controller := host.NewController(testRunner(func(ctx context.Context, _ []wire.Message, _ func(abi.Event) error) error {
		close(started)
		<-ctx.Done()
		return ctx.Err()
	}), 4)
	id, err := controller.StartPrompt(context.Background(), "hello")
	if err != nil {
		t.Fatal(err)
	}
	<-started
	observerCtx, detach := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() {
		defer close(done)
		observe(observerCtx, controller, id, func(host.RunSnapshot) {}, func(error) {})
	}()
	detach()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("observer did not detach")
	}
	snapshot, err := controller.Snapshot(id)
	if err != nil || snapshot.Status != host.RunRunning {
		t.Fatal("detachment cancelled execution", snapshot, err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if err := controller.Close(ctx); err != nil {
		t.Fatal(err)
	}
}

func TestNativeObserverResnapshotsExpiredReplayWithoutReexecution(t *testing.T) {
	done := make(chan struct{})
	controller := host.NewController(testRunner(func(ctx context.Context, _ []wire.Message, emit func(abi.Event) error) error {
		defer close(done)
		for i := 0; i < 10; i++ {
			if err := emit(abi.Event{Kind: abi.EventTextDelta, Text: "x"}); err != nil {
				return err
			}
		}
		return nil
	}), 2)
	id, err := controller.StartPrompt(context.Background(), "hello")
	if err != nil {
		t.Fatal(err)
	}
	<-done
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	var final host.RunSnapshot
	var failure error
	observe(ctx, controller, id, func(snapshot host.RunSnapshot) { final = snapshot }, func(err error) { failure = err })
	if failure != nil || final.Status != host.RunCompleted || final.Text != strings.Repeat("x", 10) {
		t.Fatal("expired event window was not reconciled", final, failure)
	}
	if err := controller.Close(ctx); err != nil {
		t.Fatal(err)
	}
}

func TestNativeObserverCapacityFailureStillObservesRealTerminal(t *testing.T) {
	for _, name := range []string{"completion", "explicit stop", "observer detach"} {
		t.Run(name, func(t *testing.T) {
			started, release := make(chan struct{}), make(chan struct{})
			controller := host.NewController(testRunner(func(ctx context.Context, _ []wire.Message, _ func(abi.Event) error) error {
				close(started)
				select {
				case <-release:
					return nil
				case <-ctx.Done():
					return ctx.Err()
				}
			}), 4)
			defer controller.Close(context.Background())
			id, err := controller.StartPrompt(context.Background(), "hello")
			if err != nil {
				t.Fatal(err)
			}
			<-started
			for i := 0; i < 8; i++ {
				_, unsubscribe, err := controller.Subscribe(id, 0)
				if err != nil {
					t.Fatal(err)
				}
				defer unsubscribe()
			}
			ctx, detach := context.WithCancel(context.Background())
			defer detach()
			rejected, terminal, done := make(chan error, 1), make(chan host.RunSnapshot, 1), make(chan struct{})
			go func() {
				defer close(done)
				observe(ctx, controller, id, func(snapshot host.RunSnapshot) {
					if snapshot.Status != host.RunRunning {
						terminal <- snapshot
					}
				}, func(err error) { rejected <- err })
			}()
			select {
			case <-rejected:
			case <-time.After(5 * time.Second):
				t.Fatal("expected subscription capacity failure")
			}
			if snapshot, _ := controller.Snapshot(id); snapshot.Status != host.RunRunning {
				t.Fatal("subscription failure stopped execution")
			}
			if name == "observer detach" {
				detach()
				select {
				case <-done:
				case <-time.After(5 * time.Second):
					t.Fatal("snapshot fallback did not detach")
				}
				if snapshot, _ := controller.Snapshot(id); snapshot.Status != host.RunRunning {
					t.Fatal("fallback detachment stopped execution")
				}
				return
			}
			want := host.RunCompleted
			if name == "explicit stop" {
				want = host.RunCancelled
				if err := controller.Stop(id); err != nil {
					t.Fatal(err)
				}
			} else {
				close(release)
			}
			select {
			case snapshot := <-terminal:
				if snapshot.Status != want {
					t.Fatalf("wanted actual terminal %s, got %+v", want, snapshot)
				}
			case <-time.After(5 * time.Second):
				t.Fatal("subscription failure left UI without terminal recovery")
			}
			select {
			case <-done:
			case <-time.After(5 * time.Second):
				t.Fatal("terminal observer did not exit")
			}
		})
	}
}

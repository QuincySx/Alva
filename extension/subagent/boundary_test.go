// INPUT: Explicit child factory ownership returned alongside a preparation error and actual parent invocation.
// OUTPUT: Partial child setup is closed once before returning its original/cleanup errors.
// POS: [L4 extension] An error does not discard a returned resource owner or start child execution.
package subagent

import (
	"context"
	"errors"
	"testing"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestChildFactoryErrorStillClosesReturnedOwner(t *testing.T) {
	p, _, execution := setup(t, func(context.Context, []wire.Message, func(abi.Event) error) error {
		t.Error("failed setup executed child")
		return nil
	}, nil)
	prepareErr, closeErr := errors.New("partial setup failed"), errors.New("partial cleanup failed")
	closed := 0
	p.config.Factory = ChildFactoryFunc(func(context.Context, ChildRequest) (Child, error) {
		return Child{Close: func(context.Context) error { closed++; return closeErr }}, prepareErr
	})
	_, err := execute(t, p, execution, `{"task":"work"}`)
	if closed != 1 || !errors.Is(err, prepareErr) || !errors.Is(err, closeErr) {
		t.Fatal("partial child ownership/error was lost", closed, err)
	}
}

func TestFailedChildCleanupRemainsOwnedForRetry(t *testing.T) {
	p, _, execution := setup(t, nil, nil)
	closed := 0
	p.config.Factory = ChildFactoryFunc(func(context.Context, ChildRequest) (Child, error) {
		return Child{Close: func(context.Context) error {
			closed++
			if closed < 3 {
				return context.DeadlineExceeded
			}
			return nil
		}}, errors.New("setup failed")
	})
	_, err := execute(t, p, execution, `{"task":"work"}`)
	if !errors.Is(err, ErrChildCleanup) || closed != 1 {
		t.Fatal(err, closed)
	}
	if err := p.OnRunEnd(context.Background(), abi.RuntimeContext{RunID: execution.RunID}, err); !errors.Is(err, ErrChildCleanup) || closed != 2 {
		t.Fatal(err, closed)
	}
	if p.CleanupComplete() {
		t.Fatal("unfinished child forgotten")
	}
	if err := p.OnRunStart(context.Background(), abi.RuntimeContext{RunID: "next", State: p.state}); err == nil {
		t.Fatal("new run admitted over pending cleanup")
	}
	if err := p.Close(context.Background()); err != nil || !p.CleanupComplete() || closed != 3 {
		t.Fatal("cleanup retry lost", err, closed)
	}
	if err := p.Close(context.Background()); err != nil || closed != 3 {
		t.Fatal("completed cleanup repeated", err, closed)
	}
}

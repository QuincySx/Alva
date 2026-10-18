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

// INPUT: errors, io/fs, testing, kernel/abi
// OUTPUT: Recoverable classification regressions for denials, path problems and joined faults
// POS: [L3 capability library] Only a top-level, unjoined error becomes model-visible; a joined tree may hide a fault and aborts.
package security

import (
	"context"
	"errors"
	"io/fs"
	"testing"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

func TestRecoverableOnlyTrustsTopLevelErrors(t *testing.T) {
	fault := errors.New("store write failed")
	notExist := &fs.PathError{Op: "open", Path: "x", Err: fs.ErrNotExist}
	if got := Recoverable(notExist); !errors.As(got, new(*abi.ToolFailedError)) {
		t.Fatalf("plain missing path must be recoverable: %v", got)
	}
	if got := Recoverable(errors.Join(&DeniedError{Reason: "no"}, nil)); errors.As(got, new(*abi.ToolBlockedError)) {
		t.Fatal("a Join-wrapped denial must not be converted")
	}
	for name, err := range map[string]error{
		"join":      errors.Join(notExist, fault),
		"wrapped":   errors.Join(errors.New("ctx"), notExist),
		"cancel":    errors.Join(notExist, context.Canceled),
		"session":   &abi.SessionError{Operation: "append", Err: notExist},
		"deep join": &wrapErr{errors.Join(notExist, fault)},
	} {
		if got := Recoverable(err); got != err {
			t.Errorf("%s was converted: %v", name, got)
		}
	}
}

type wrapErr struct{ err error }

func (w *wrapErr) Error() string { return w.err.Error() }
func (w *wrapErr) Unwrap() error { return w.err }

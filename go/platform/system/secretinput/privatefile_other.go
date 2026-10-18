// INPUT: Unimplemented private input file ownership platform
// OUTPUT: Explicit OpenPrivateFile unsupported result instead of chmod-based privacy claims
// POS: [platform / OS primitive] Missing native ownership proof fails closed.
//go:build !darwin && !linux && !windows

package secretinput

import (
	"errors"
	"os"
)

func OpenPrivateFile(string) (*os.File, error) {
	return nil, errors.New("private input file ownership is unsupported on this platform")
}

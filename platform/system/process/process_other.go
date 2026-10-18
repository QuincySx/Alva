// INPUT: os/exec, errors
// OUTPUT: Explicit unsupported-platform result
// POS: [platform / OS primitive] Refuse unsafe managed subprocesses until platform ownership is implemented
//go:build !unix && !windows

package process

import (
	"errors"
	"os/exec"
)

func configure(*exec.Cmd) error {
	return errors.New("owned process groups are not implemented on this platform")
}
func terminate(*exec.Cmd) error { return nil }

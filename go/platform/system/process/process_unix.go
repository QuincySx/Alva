// INPUT: os/exec, syscall
// OUTPUT: Unix owned group configuration and termination
// POS: [platform / OS primitive] Kill owned process groups before the leader can be reaped or its PID reused
//go:build unix && !darwin

package process

import (
	"errors"
	"os/exec"
	"syscall"
)

func configure(cmd *exec.Cmd) error {
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	return nil
}
func terminate(cmd *exec.Cmd) error {
	err := syscall.Kill(-cmd.Process.Pid, syscall.SIGKILL)
	if errors.Is(err, syscall.ESRCH) {
		return nil
	}
	return err
}

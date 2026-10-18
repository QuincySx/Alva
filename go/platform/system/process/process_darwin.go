// INPUT: os/exec, syscall, time, x/sys/unix process-group metadata
// OUTPUT: Darwin owned group configuration and checked termination
// POS: [platform / OS primitive] Distinguish XNU zombie-only killpg EPERM from actual live-member permission failures
//go:build darwin

package process

import (
	"errors"
	"os/exec"
	"syscall"
	"time"

	"golang.org/x/sys/unix"
)

func configure(cmd *exec.Cmd) error {
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	return nil
}
func terminate(cmd *exec.Cmd) error {
	pid := cmd.Process.Pid
	err := syscall.Kill(-pid, syscall.SIGKILL)
	if errors.Is(err, syscall.ESRCH) {
		return nil
	}
	if !errors.Is(err, syscall.EPERM) {
		return err
	}
	// XNU killpg1 snapshots members before proc_find. An exiting member may
	// already be unfindable but not yet visible to waitid(WEXITED), so nfound=0
	// can report EPERM before the zombie-only state is observable. Do not send
	// another signal or reap: our still-owned leader reserves the PID/group.
	return observeDarwinEPERM(err, 100*time.Millisecond, func() (bool, error) {
		return darwinOwnedGroupExited(pid)
	})
}

// Called only while BeginClose owns the unreaped child under observe.Lock.
// A true result needs BOTH a child-specific WNOWAIT exit observation and an
// independently inspected group containing no live member; neither EOF nor
// an empty group snapshot alone is proof that this leader has actually exited.
func darwinOwnedGroupExited(pid int) (bool, error) {
	ended, waitErr := exited(pid)
	if waitErr != nil || !ended {
		return false, waitErr
	}
	members, inspectErr := unix.SysctlKinfoProcSlice("kern.proc.pgrp", pid)
	if inspectErr != nil {
		return false, inspectErr
	}
	for _, member := range members {
		if member.Proc.P_stat != 5 {
			return false, nil
		}
	}
	return true, nil
}

// Retry only observations across the short XNU exit transition. Any inspection
// failure or persistent live member retains the ORIGINAL permission failure.
// This function never signals or waits/reaps a PID, including after timeout.
func observeDarwinEPERM(permissionErr error, budget time.Duration, probe func() (bool, error)) error {
	deadline := time.Now().Add(budget)
	for {
		ended, err := probe()
		if err != nil {
			return errors.Join(permissionErr, err)
		}
		if ended {
			return nil
		}
		remaining := time.Until(deadline)
		if remaining <= 0 {
			return permissionErr
		}
		time.Sleep(min(time.Millisecond, remaining))
	}
}

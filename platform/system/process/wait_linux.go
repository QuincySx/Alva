// INPUT: golang.org/x/sys/unix waitid with WNOWAIT
// OUTPUT: Non-reaping Linux leader-exit observation
// POS: [platform / OS primitive] Keep leader PID reserved while detecting natural exit before group cleanup
//go:build linux

package process

import "golang.org/x/sys/unix"

func exited(pid int) (bool, error) {
	var info unix.Siginfo
	err := unix.Waitid(unix.P_PID, pid, &info, unix.WEXITED|unix.WNOWAIT|unix.WNOHANG, nil)
	return info.Signo != 0, err
}

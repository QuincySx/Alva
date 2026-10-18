// INPUT: purego libc waitid and golang.org/x/sys/unix wait flags
// OUTPUT: Non-reaping Darwin leader-exit observation
// POS: [platform / OS primitive] Preserve group ownership until zombie detection before kill-and-reap
//go:build darwin

package process

import (
	"errors"
	"sync"

	"github.com/ebitengine/purego"
	"golang.org/x/sys/unix"
)

// Darwin SDK sys/signal.h defines six 32-bit fields followed by three native
// words and seven reserved words. waitid(WNOWAIT) observes only our direct child;
// unlike kern.proc sysctl it does not require process-inspection entitlement.
type siginfo struct {
	Signo, Errno, Code, Pid int32
	Uid                     uint32
	Status                  int32
	Addr, Value, Band       uintptr
	Pad                     [7]uintptr
}

var waitInit sync.Once
var waitFunc func(int32, uint32, *siginfo, int32) int32
var waitErr error

func exited(pid int) (bool, error) {
	waitInit.Do(func() {
		lib, err := purego.Dlopen("/usr/lib/libSystem.B.dylib", purego.RTLD_NOW|purego.RTLD_LOCAL)
		if err != nil {
			waitErr = err
			return
		}
		symbol, err := purego.Dlsym(lib, "waitid")
		if err != nil {
			waitErr = err
			return
		}
		purego.RegisterFunc(&waitFunc, symbol)
	})
	if waitErr != nil {
		return false, waitErr
	}
	var info siginfo
	if waitFunc(1, uint32(pid), &info, unix.WEXITED|unix.WNOWAIT|unix.WNOHANG) != 0 {
		return false, errors.New("owned child waitid failed")
	}
	return info.Signo != 0, nil
}

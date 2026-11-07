// INPUT: The actual unreaped owned child, Darwin libproc rusage_info_v0 and the existing process observation lock.
// OUTPUT: MemoryBytes for only this owned PID, using max(current resident size, physical footprint).
// POS: [platform / OS primitive] Observes decoder memory without accepting arbitrary PIDs or changing system resource policy.
//go:build darwin

package process

import (
	"errors"
	"os"
	"sync"

	"github.com/ebitengine/purego"
)

// ABI from the macOS SDK sys/resource.h rusage_info_v0 (96 bytes).
// libproc.h proc_pid_rusage is available since macOS 10.9.
type ownedRusage struct {
	UUID                                                                                                             [16]byte
	UserTime, SystemTime, PackageWakeups, InterruptWakeups, Pageins, Wired, Resident, Footprint, StartTime, ExitTime uint64
}

var usageOnce sync.Once
var usageCall func(int32, int32, *ownedRusage) int32
var usageError error

// MemoryBytes is observational, not a kernel-enforced heap cap. The existing
// observation lock keeps the leader unreaped throughout the PID-specific probe;
// callers cannot inspect an unrelated/recycled process through this method.
func (p *Process) MemoryBytes() (uint64, error) {
	p.observe.Lock()
	defer p.observe.Unlock()
	select {
	case <-p.closing:
		return 0, os.ErrClosed
	default:
	}
	usageOnce.Do(func() {
		library, err := purego.Dlopen("/usr/lib/libproc.dylib", purego.RTLD_NOW|purego.RTLD_LOCAL)
		if err != nil {
			usageError = err
			return
		}
		symbol, err := purego.Dlsym(library, "proc_pid_rusage")
		if err != nil {
			usageError = err
			return
		}
		purego.RegisterFunc(&usageCall, symbol)
	})
	if usageError != nil {
		return 0, errors.New("owned process memory observation is unavailable")
	}
	var usage ownedRusage
	if usageCall(int32(p.cmd.Process.Pid), 0, &usage) != 0 {
		return 0, errors.New("owned process memory observation failed")
	}
	return max(usage.Resident, usage.Footprint), nil
}

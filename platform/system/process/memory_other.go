// INPUT: A process owner on platforms without the validated Darwin libproc usage probe.
// OUTPUT: Explicitly unsupported owned-process memory observation.
// POS: [platform / OS primitive] Does not infer memory usage or inspect arbitrary process IDs.
//go:build !darwin

package process

import "errors"

func (p *Process) MemoryBytes() (uint64, error) {
	return 0, errors.New("owned process memory observation is unsupported on this platform")
}

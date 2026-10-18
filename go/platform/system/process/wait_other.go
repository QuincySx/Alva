// INPUT: errors
// OUTPUT: Explicit unsupported non-reaping wait result
// POS: [platform / OS primitive] Do not infer successful process exit from pipe EOF on unsupported platforms
//go:build !darwin && !linux && !windows

package process

import "errors"

func exited(int) (bool, error) {
	return false, errors.New("non-reaping process wait unsupported on this platform")
}

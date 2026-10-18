// INPUT: Explicit canonical decoder directory and Windows owner-only DACL validation
// OUTPUT: Actual Windows private decoder root checks independent of Unix chmod bits
// POS: [platform / media owner] Input transport and execution budgets remain owned by the media runner.
//go:build windows

package media

import (
	"github.com/QuincySx/Alva/platform/system/privatefs"
	"os"
)

func checkDecoderDirectory(path string, _ os.FileInfo) error { return privatefs.CheckDir(path) }

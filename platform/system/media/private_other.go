// INPUT: Explicit canonical decoder directory metadata on non-Windows hosts
// OUTPUT: Private permission validation without modifying existing decoder directories
// POS: [platform / media owner] Keep Unix access modes separate from Windows security descriptors.
//go:build !windows

package media

import (
	"errors"
	"os"
)

func checkDecoderDirectory(_ string, info os.FileInfo) error {
	if info.Mode().Perm()&0077 != 0 {
		return errors.New("decoder directory must be private to its owner")
	}
	return nil
}

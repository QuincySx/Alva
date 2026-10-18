// INPUT: Explicit open regular file descriptors and Unix fcntl access-mode inspection
// OUTPUT: Read-only inheritance validation before any owned subprocess launch
// POS: [platform / OS primitive] Descriptor grants cannot silently inherit write authority under a read-file capability
//go:build unix

package process

import (
	"errors"
	"golang.org/x/sys/unix"
	"os"
)

func validateReadFile(file *os.File) error {
	flags, err := unix.FcntlInt(file.Fd(), unix.F_GETFL, 0)
	if err != nil || flags&unix.O_ACCMODE != unix.O_RDONLY {
		return errors.New("granted regular file descriptor must be read-only")
	}
	return nil
}

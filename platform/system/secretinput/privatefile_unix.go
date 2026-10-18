// INPUT: Explicit caller-selected credential file and Unix non-following open/current-owner metadata
// OUTPUT: OpenPrivateFile returns one pinned private read-only regular file without discovering a path
// POS: [platform / OS primitive] Credential file transport never creates files, follows final symlinks or reads ambient configuration.
//go:build darwin || linux

package secretinput

import (
	"errors"
	"os"
	"syscall"

	"golang.org/x/sys/unix"
)

func OpenPrivateFile(path string) (*os.File, error) {
	if path == "" {
		return nil, errors.New("explicit private input file required")
	}
	fd, err := unix.Open(path, unix.O_RDONLY|unix.O_NOFOLLOW|unix.O_NONBLOCK|unix.O_CLOEXEC, 0)
	if err != nil {
		return nil, errors.New("private input file unavailable")
	}
	file := os.NewFile(uintptr(fd), "explicit-private-input")
	if file == nil {
		unix.Close(fd)
		return nil, errors.New("private input descriptor unavailable")
	}
	info, err := file.Stat()
	if err == nil {
		stat, ok := info.Sys().(*syscall.Stat_t)
		if !ok || !info.Mode().IsRegular() || info.Mode().Perm()&0077 != 0 || stat.Uid != uint32(os.Getuid()) || stat.Nlink != 1 {
			err = errors.New("private input must be a current-user owner-only regular file without hardlinks")
		}
	}
	if err != nil {
		file.Close()
		return nil, errors.New("private input ownership validation failed")
	}
	return file, nil
}

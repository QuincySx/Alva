// INPUT: Exclusive borrowed pipe, cancellation context, bounded bytes and Unix poll/read syscalls
// OUTPUT: Cancellable reads of inherited fd0 without deadlines, descriptor mutation or detached readers
// POS: [platform / OS primitive] Preserve Go file ownership while polling actual native command stdin.
//go:build darwin || linux

package secretinput

import (
	"context"
	"errors"
	"os"

	"golang.org/x/sys/unix"
)

func readNativePipe(ctx context.Context, file *os.File, maximum int) ([]byte, error) {
	connection, err := file.SyscallConn()
	if err != nil {
		return nil, errors.New("secret input pipe unavailable")
	}
	var data []byte
	var buffer [4096]byte
	for {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		chunk := buffer[:min(len(buffer), maximum+1-len(data))]
		var readErr error
		n, ready := 0, false
		// Control pins the actual descriptor against Close/reuse for this single
		// bounded poll/read. The caller grants exclusive reads: after POLLIN,
		// nobody else can consume readiness before the one pipe read. Do not use
		// File.Read here: inherited stdin need not belong to Go's deadline poller.
		controlErr := connection.Control(func(fd uintptr) {
			if uint64(fd) > uint64(^uint32(0)>>1) {
				readErr = errors.New("secret input descriptor out of range")
				return
			}
			poll := []unix.PollFd{{Fd: int32(fd), Events: unix.POLLIN}}
			var count int
			count, readErr = unix.Poll(poll, 50)
			if readErr != nil || count == 0 {
				return
			}
			if poll[0].Revents&unix.POLLNVAL != 0 {
				readErr = errors.New("secret input descriptor unavailable")
				return
			}
			if poll[0].Revents&(unix.POLLIN|unix.POLLHUP|unix.POLLERR) != 0 {
				ready = true
				n, readErr = unix.Read(int(fd), chunk)
			}
		})
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if controlErr != nil {
			return nil, errors.New("secret input pipe unavailable")
		}
		if errors.Is(readErr, unix.EINTR) || errors.Is(readErr, unix.EAGAIN) || errors.Is(readErr, unix.EWOULDBLOCK) {
			continue
		}
		if readErr != nil {
			return nil, errors.New("secret input pipe read failed")
		}
		if !ready {
			continue
		}
		data = append(data, chunk[:n]...)
		if len(data) > maximum {
			return nil, errors.New("secret input exceeds byte ceiling")
		}
		if n == 0 {
			return data, nil
		}
	}
}

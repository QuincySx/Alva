// INPUT: Exclusive inherited Windows stdin pipe, cancellation context and native PeekNamedPipe/ReadFile
// OUTPUT: Bounded cancellable secret input without relying on inherited-handle deadlines
// POS: [platform / OS primitive] Keep the caller's handle and flags unchanged while reading only observed available bytes.
//go:build windows

package secretinput

import (
	"context"
	"errors"
	"os"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

var peekNamedPipe = windows.NewLazySystemDLL("kernel32.dll").NewProc("PeekNamedPipe")

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
		var available, n uint32
		var readErr error
		eof := false
		controlErr := connection.Control(func(fd uintptr) {
			ok, _, err := peekNamedPipe.Call(fd, 0, 0, 0, uintptr(unsafe.Pointer(&available)), 0)
			if ok == 0 {
				readErr = err
				return
			}
			if available == 0 {
				return
			}
			chunk := buffer[:int(min(uint32(len(buffer)), available, uint32(maximum+1-len(data))))]
			readErr = windows.ReadFile(windows.Handle(fd), chunk, &n, nil)
		})
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if controlErr != nil {
			return nil, errors.New("secret input pipe unavailable")
		}
		if errors.Is(readErr, windows.ERROR_BROKEN_PIPE) || errors.Is(readErr, windows.ERROR_PIPE_NOT_CONNECTED) {
			eof = true
			readErr = nil
		}
		if readErr != nil && !(errors.Is(readErr, windows.ERROR_MORE_DATA) && n > 0) {
			return nil, errors.New("secret input pipe read failed")
		}
		data = append(data, buffer[:n]...)
		if len(data) > maximum {
			return nil, errors.New("secret input exceeds byte ceiling")
		}
		if eof {
			return data, nil
		}
		if available == 0 {
			timer := time.NewTimer(20 * time.Millisecond)
			select {
			case <-ctx.Done():
				timer.Stop()
				return nil, ctx.Err()
			case <-timer.C:
			}
		}
	}
}

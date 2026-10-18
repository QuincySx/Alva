// INPUT: Sole-reader borrowed Windows file/pipe, context and explicit line byte ceiling
// OUTPUT: ReadLine without readahead, borrowed-handle changes or detached readers
// POS: [platform / OS primitive] File transport only; unverified cooked console cancellation is rejected before reading.
//go:build windows

package lineinput

import (
	"context"
	"errors"
	"io"
	"os"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

var peekNamedPipe = windows.NewLazySystemDLL("kernel32.dll").NewProc("PeekNamedPipe")

func ReadLine(ctx context.Context, file *os.File, maximum int) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	if file == nil || maximum < 1 || maximum > 64<<10 {
		return "", errors.New("explicit input file and1..64KiB line budget required")
	}
	connection, err := file.SyscallConn()
	if err != nil {
		return "", err
	}
	var kind uint32
	var typeErr error
	if err := connection.Control(func(fd uintptr) { kind, typeErr = windows.GetFileType(windows.Handle(fd)) }); err != nil {
		return "", err
	}
	if typeErr != nil || (kind != windows.FILE_TYPE_DISK && kind != windows.FILE_TYPE_PIPE) {
		return "", errors.New("cancellable Windows input requires a regular file or pipe; interactive console is unsupported")
	}
	var data []byte
	var one [1]byte
	for {
		if err := ctx.Err(); err != nil {
			return "", err
		}
		var n, available uint32
		var readErr error
		controlErr := connection.Control(func(fd uintptr) {
			if kind == windows.FILE_TYPE_PIPE {
				ok, _, err := peekNamedPipe.Call(fd, 0, 0, 0, uintptr(unsafe.Pointer(&available)), 0)
				if ok == 0 {
					readErr = err
					return
				}
				if available == 0 {
					return
				}
			}
			readErr = windows.ReadFile(windows.Handle(fd), one[:], &n, nil)
		})
		if err := ctx.Err(); err != nil {
			return "", err
		}
		if controlErr != nil {
			return "", controlErr
		}
		eof := errors.Is(readErr, windows.ERROR_BROKEN_PIPE) || errors.Is(readErr, windows.ERROR_PIPE_NOT_CONNECTED) || errors.Is(readErr, windows.ERROR_HANDLE_EOF) || kind == windows.FILE_TYPE_DISK && n == 0 && readErr == nil
		if readErr != nil && !eof && !(errors.Is(readErr, windows.ERROR_MORE_DATA) && n > 0) {
			return "", readErr
		}
		if n > 0 {
			data = append(data, one[0])
			// Match CLI line budgets: maximum content bytes may be followed by
			// one LF delimiter; it does not consume content capacity.
			if one[0] == '\n' {
				return string(data), nil
			}
			if len(data) > maximum {
				return "", errors.New("input line exceeds byte ceiling")
			}
		}
		if eof {
			if len(data) > 0 {
				return string(data), nil
			}
			return "", io.EOF
		}
		if kind == windows.FILE_TYPE_PIPE && available == 0 {
			timer := time.NewTimer(20 * time.Millisecond)
			select {
			case <-ctx.Done():
				timer.Stop()
				return "", ctx.Err()
			case <-timer.C:
			}
		}
	}
}

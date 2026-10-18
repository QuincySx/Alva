// INPUT: Private DACL root, bounded selected bytes and explicit Windows file handles
// OUTPUT: Delete-on-close snapshots attenuated to read-only and seekable stdin/handle decoder binding
// POS: [platform / OS primitive] No Unix fd assumption, pathname reopen, input copy goroutine or inherited writable handle reaches the decoder.
//go:build windows

package media

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"io"
	"os"
	"path/filepath"

	"github.com/QuincySx/Alva/platform/system/process"
	"golang.org/x/sys/windows"
)

// FFmpeg's documented Windows handle protocol defaults to STD_INPUT_HANDLE
// and preserves regular-file seek support. Old builds without this protocol
// fail explicitly; do not fall back to original paths or unseekable pipes.
const decoderInputURL = "handle:"
const decoderProtocols = "handle,pipe"

func bindDecoderInput(config *process.Config, file *os.File) { config.StdinFile = file }
func inputSnapshot(directory, prefix string, data []byte) (output *os.File, result error) {
	if len(data) > MaxInput {
		return nil, errors.New("media input exceeds20MiB")
	}
	var id [16]byte
	if _, err := rand.Read(id[:]); err != nil {
		return nil, err
	}
	name := filepath.Join(directory, prefix+hex.EncodeToString(id[:]))
	path, err := windows.UTF16PtrFromString(name)
	if err != nil {
		return nil, err
	}
	// The owner-only parent supplies the inherited DACL. CREATE_NEW prevents
	// replacing any existing object. Deletion belongs to this exact object,
	// not a later path lookup; no writable handle is inherited by the child.
	handle, err := windows.CreateFile(path, windows.GENERIC_READ|windows.GENERIC_WRITE|windows.DELETE, windows.FILE_SHARE_READ|windows.FILE_SHARE_DELETE, nil, windows.CREATE_NEW, windows.FILE_ATTRIBUTE_TEMPORARY|windows.FILE_FLAG_DELETE_ON_CLOSE, 0)
	if err != nil {
		return nil, err
	}
	file := os.NewFile(uintptr(handle), name)
	if file == nil {
		windows.CloseHandle(handle)
		return nil, errors.New("media snapshot handle unavailable")
	}
	var read *os.File
	defer func() {
		result = errors.Join(result, file.Close())
		if result != nil && read != nil {
			result = errors.Join(result, read.Close())
			output = nil
		}
	}()
	if n, err := file.Write(data); err != nil || n != len(data) {
		return nil, errors.Join(err, io.ErrShortWrite)
	}
	if err := file.Sync(); err != nil {
		return nil, err
	}
	var readHandle windows.Handle
	if err := windows.DuplicateHandle(windows.CurrentProcess(), handle, windows.CurrentProcess(), &readHandle, windows.GENERIC_READ|windows.SYNCHRONIZE, false, 0); err != nil {
		return nil, err
	}
	read = os.NewFile(uintptr(readHandle), name)
	if read == nil {
		windows.CloseHandle(readHandle)
		return nil, errors.New("read-only media handle unavailable")
	}
	if _, err := read.Seek(0, io.SeekStart); err != nil {
		return nil, err
	}
	return read, nil
}

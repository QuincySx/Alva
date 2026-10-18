// INPUT: Go Root confinement and Windows handle-relative single-leaf NT file operations
// OUTPUT: Windows nativeOpenFlags/nativeOpen for real WASI/script regular file IO and atomic no-follow
// POS: [platform / WASM host] Root owns traversal; explicit no-follow never falls back to a racy pathname check or truncates a reparse target.
//go:build windows

package host

import (
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"unicode/utf8"
	"unsafe"

	"golang.org/x/sys/windows"
)

func nativeOpenFlags(flags int, _ bool) (int, error) {
	const known = os.O_WRONLY | os.O_RDWR | os.O_APPEND | os.O_CREATE | os.O_EXCL | os.O_SYNC | os.O_TRUNC
	if flags & ^known != 0 || flags&3 == 3 {
		return 0, fs.ErrInvalid
	}
	// Windows Root refuses device names, ADS and out-of-root reparse traversal;
	// named pipe namespaces cannot be regular relative files within that Root.
	// Unix O_NONBLOCK is therefore neither available nor silently emulated.
	return flags, nil
}
func nativeOpen(root *os.Root, name string, flags int, mode os.FileMode, noFollow bool) (*os.File, error) {
	if _, err := nativeOpenFlags(flags, noFollow); err != nil {
		return nil, err
	}
	if !noFollow {
		return root.OpenFile(name, flags, mode)
	}
	if !utf8.ValidString(name) || !filepath.IsLocal(name) || strings.IndexByte(name, 0) >= 0 {
		return nil, fs.ErrPermission
	}
	name = filepath.Clean(name)
	if name == "." {
		if flags != os.O_RDONLY {
			return nil, fs.ErrPermission
		}
		return root.Open(".")
	}
	// Root resolves/contains all parent links. NtCreateFile receives only one
	// leaf name relative to that already-open parent, so OPEN_REPARSE_POINT is
	// sufficient even on Windows versions without OBJ_DONT_REPARSE support.
	parent, err := root.OpenRoot(filepath.Dir(name))
	if err != nil {
		return nil, err
	}
	defer parent.Close()
	directory, err := parent.Open(".")
	if err != nil {
		return nil, err
	}
	defer directory.Close()
	leaf, err := windows.NewNTUnicodeString(filepath.Base(name))
	if err != nil {
		return nil, err
	}
	access := uint32(windows.FILE_GENERIC_READ)
	if flags&3 == os.O_WRONLY {
		access = windows.FILE_GENERIC_WRITE
	}
	if flags&3 == os.O_RDWR {
		access |= windows.FILE_GENERIC_WRITE
	}
	if flags&(os.O_CREATE|os.O_TRUNC) != 0 {
		access |= windows.FILE_GENERIC_WRITE
	}
	access |= windows.FILE_READ_ATTRIBUTES | windows.SYNCHRONIZE
	if flags&os.O_APPEND != 0 {
		access |= windows.FILE_APPEND_DATA
		if flags&os.O_TRUNC == 0 {
			access &^= windows.FILE_WRITE_DATA
		}
	}
	disposition := uint32(windows.FILE_OPEN)
	if flags&os.O_CREATE != 0 {
		disposition = windows.FILE_OPEN_IF
		if flags&os.O_EXCL != 0 {
			disposition = windows.FILE_CREATE
		}
	}
	options := uint32(windows.FILE_SYNCHRONOUS_IO_NONALERT | windows.FILE_OPEN_REPARSE_POINT)
	if flags&(os.O_WRONLY|os.O_RDWR|os.O_CREATE|os.O_TRUNC|os.O_APPEND) != 0 {
		options |= windows.FILE_NON_DIRECTORY_FILE
	}
	if flags&os.O_SYNC != 0 {
		options |= windows.FILE_WRITE_THROUGH
	}
	attributes := uint32(windows.FILE_ATTRIBUTE_NORMAL)
	if flags&os.O_CREATE != 0 && mode.Perm()&0200 == 0 {
		attributes = windows.FILE_ATTRIBUTE_READONLY
	}
	connection, err := directory.SyscallConn()
	if err != nil {
		return nil, err
	}
	var handle windows.Handle
	var openErr error
	err = connection.Control(func(parentHandle uintptr) {
		object := windows.OBJECT_ATTRIBUTES{Length: uint32(unsafe.Sizeof(windows.OBJECT_ATTRIBUTES{})), RootDirectory: windows.Handle(parentHandle), ObjectName: leaf, Attributes: windows.OBJ_CASE_INSENSITIVE}
		openErr = windows.NtCreateFile(&handle, access, &object, &windows.IO_STATUS_BLOCK{}, nil, attributes, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE, disposition, options, 0, 0)
	})
	runtime.KeepAlive(leaf)
	if err != nil || openErr != nil {
		if handle != 0 && handle != windows.InvalidHandle {
			windows.CloseHandle(handle)
		}
		if status, ok := openErr.(windows.NTStatus); ok {
			openErr = status.Errno()
		}
		return nil, errors.Join(err, openErr)
	}
	var info windows.ByHandleFileInformation
	if err := windows.GetFileInformationByHandle(handle, &info); err != nil || info.FileAttributes&windows.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
		windows.CloseHandle(handle)
		return nil, fs.ErrPermission
	}
	file := os.NewFile(uintptr(handle), name)
	if file == nil {
		windows.CloseHandle(handle)
		return nil, fs.ErrInvalid
	}
	// OPEN_IF never truncates. Validate the exact opened object BEFORE the
	// explicit truncation, so a concurrent symlink swap cannot alter its target.
	if flags&os.O_TRUNC != 0 {
		if err := file.Truncate(0); err != nil {
			file.Close()
			return nil, err
		}
	}
	if flags&(os.O_APPEND|os.O_TRUNC) == os.O_APPEND|os.O_TRUNC {
		// os.NewFile cannot set Go's private append mode. Kernel append-only
		// access guarantees EOF writes even after a caller seeks backward.
		var appendHandle windows.Handle
		if err := windows.DuplicateHandle(windows.CurrentProcess(), handle, windows.CurrentProcess(), &appendHandle, access&^windows.FILE_WRITE_DATA, false, 0); err != nil {
			file.Close()
			return nil, err
		}
		if err := file.Close(); err != nil {
			windows.CloseHandle(appendHandle)
			return nil, err
		}
		file = os.NewFile(uintptr(appendHandle), name)
		if file == nil {
			windows.CloseHandle(appendHandle)
			return nil, fs.ErrInvalid
		}
	}
	return file, nil
}

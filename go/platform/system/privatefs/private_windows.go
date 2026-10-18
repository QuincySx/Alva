// INPUT: Explicit local filesystem paths and Windows current-process token/security/file APIs
// OUTPUT: EnsureDir, CheckDir, CheckFile, OpenFile and OpenRead with private DACL and handle identity validation
// POS: [platform / OS primitive] Atomic owner-only creation never changes permissions on existing user objects.
//go:build windows

package privatefs

import (
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"unicode/utf8"
	"unsafe"

	"golang.org/x/sys/windows"
)

const fileAllAccess = 0x001f01ff

func localPath(path string) (string, error) {
	if path == "" || !utf8.ValidString(path) || strings.ContainsRune(path, 0) {
		return "", errors.New("explicit local private path required")
	}
	absolute, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	volume := filepath.VolumeName(absolute)
	if len(volume) != 2 || volume[1] != ':' || len(absolute) > 16<<10 {
		return "", errors.New("private storage requires a bounded local drive path; UNC/device paths are unsupported")
	}
	for _, part := range strings.Split(strings.TrimPrefix(absolute[len(volume):], `\`), `\`) {
		if part == "" {
			continue
		}
		if !filepath.IsLocal(part) || strings.ContainsAny(part, ":*?\"<>|") || strings.TrimRight(part, ". ") != part {
			return "", errors.New("private path aliases and alternate data streams are rejected")
		}
		base := strings.ToUpper(strings.SplitN(part, ".", 2)[0])
		if base == "CON" || base == "PRN" || base == "AUX" || base == "NUL" || (len(base) == 4 && (strings.HasPrefix(base, "COM") || strings.HasPrefix(base, "LPT")) && base[3] >= '1' && base[3] <= '9') {
			return "", errors.New("private path contains a reserved device name")
		}
	}
	root, _ := windows.UTF16PtrFromString(volume + `\`)
	if kind := windows.GetDriveType(root); kind != windows.DRIVE_FIXED && kind != windows.DRIVE_REMOVABLE {
		return "", errors.New("private storage requires a local fixed or removable drive")
	}
	var flags uint32
	if err := windows.GetVolumeInformation(root, nil, 0, nil, nil, &flags, nil, 0); err != nil || flags&windows.FILE_PERSISTENT_ACLS == 0 {
		return "", errors.New("private storage requires a local filesystem with persistent ACLs")
	}
	return absolute, nil
}

func nativePath(path string) *uint16 {
	// Only called after localPath validation. The internal extended prefix
	// preserves the already canonical local drive path on long-path hosts.
	p, _ := windows.UTF16PtrFromString(`\\?\` + path)
	return p
}

func descriptor(directory bool) (*windows.SECURITY_DESCRIPTOR, error) {
	user, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil {
		return nil, err
	}
	inherit := ""
	if directory {
		inherit = "OICI"
	}
	return windows.SecurityDescriptorFromString("O:" + user.User.Sid.String() + "D:P(A;" + inherit + ";FA;;;" + user.User.Sid.String() + ")")
}

func checkSecurity(handle windows.Handle, directory bool) error {
	info := windows.ByHandleFileInformation{}
	if err := windows.GetFileInformationByHandle(handle, &info); err != nil {
		return err
	}
	if info.FileAttributes&windows.FILE_ATTRIBUTE_REPARSE_POINT != 0 || (info.FileAttributes&windows.FILE_ATTRIBUTE_DIRECTORY != 0) != directory || (!directory && info.NumberOfLinks != 1) {
		return errors.New("private object must have its exact regular identity without reparse points or hardlinks")
	}
	sd, err := windows.GetSecurityInfo(handle, windows.SE_FILE_OBJECT, windows.OWNER_SECURITY_INFORMATION|windows.DACL_SECURITY_INFORMATION)
	if err != nil {
		return err
	}
	if !sd.IsValid() {
		return errors.New("invalid private security descriptor")
	}
	user, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil {
		return err
	}
	owner, _, err := sd.Owner()
	if err != nil || owner == nil || !owner.Equals(user.User.Sid) {
		return errors.New("private object is not owned by the current user")
	}
	control, _, err := sd.Control()
	if err != nil || (directory && control&windows.SE_DACL_PROTECTED == 0) {
		return errors.New("private directory must reject ancestor ACL inheritance")
	}
	dacl, _, err := sd.DACL()
	if err != nil || dacl == nil || dacl.AceCount == 0 || dacl.AceCount > 64 {
		return errors.New("private object requires a bounded non-null owner-only DACL")
	}
	var access windows.ACCESS_MASK
	for i := uint32(0); i < uint32(dacl.AceCount); i++ {
		var ace *windows.ACCESS_ALLOWED_ACE
		if err := windows.GetAce(dacl, i, &ace); err != nil {
			return err
		}
		if ace.Header.AceType != windows.ACCESS_ALLOWED_ACE_TYPE || ace.Header.AceSize < 12 {
			return errors.New("unsupported private ACL entry")
		}
		sid := (*windows.SID)(unsafe.Pointer(&ace.SidStart))
		if !sid.IsValid() || !sid.Equals(user.User.Sid) {
			return errors.New("private object grants access to another principal")
		}
		if ace.Header.AceFlags&0x08 == 0 {
			access |= ace.Mask
		} // INHERIT_ONLY_ACE is not effective on this object.
	}
	if access&fileAllAccess != fileAllAccess && access&windows.GENERIC_ALL == 0 {
		return errors.New("private owner requires full object control")
	}
	runtime.KeepAlive(sd)
	return nil
}

func openChecked(path string, directory bool) (windows.Handle, error) {
	p := nativePath(path)
	handle, err := windows.CreateFile(p, windows.READ_CONTROL|windows.FILE_READ_ATTRIBUTES, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE, nil, windows.OPEN_EXISTING, windows.FILE_FLAG_OPEN_REPARSE_POINT|windows.FILE_FLAG_BACKUP_SEMANTICS, 0)
	if err != nil {
		return windows.InvalidHandle, err
	}
	if err := checkSecurity(handle, directory); err != nil {
		windows.CloseHandle(handle)
		return windows.InvalidHandle, err
	}
	return handle, nil
}

// walk rejects reparse ancestors without requiring OS/user ancestor directories
// to have our private DACL. Only newly created and final directories are private.
func walk(absolute string, create bool) error {
	volume := filepath.VolumeName(absolute)
	current := volume + `\`
	parts := strings.Split(strings.TrimPrefix(absolute[len(volume):], `\`), `\`)
	for _, part := range parts {
		if part == "" {
			continue
		}
		current = filepath.Join(current, part)
		p := nativePath(current)
		if create {
			sd, err := descriptor(true)
			if err != nil {
				return err
			}
			sa := windows.SecurityAttributes{Length: uint32(unsafe.Sizeof(windows.SecurityAttributes{})), SecurityDescriptor: sd}
			err = windows.CreateDirectory(p, &sa)
			runtime.KeepAlive(sd)
			if err != nil && !errors.Is(err, windows.ERROR_ALREADY_EXISTS) {
				return err
			}
		}
		handle, err := windows.CreateFile(p, windows.FILE_READ_ATTRIBUTES, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE, nil, windows.OPEN_EXISTING, windows.FILE_FLAG_OPEN_REPARSE_POINT|windows.FILE_FLAG_BACKUP_SEMANTICS, 0)
		if err != nil {
			return err
		}
		var info windows.ByHandleFileInformation
		err = windows.GetFileInformationByHandle(handle, &info)
		windows.CloseHandle(handle)
		if err != nil {
			return err
		}
		if info.FileAttributes&windows.FILE_ATTRIBUTE_DIRECTORY == 0 || info.FileAttributes&windows.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
			return errors.New("private directory ancestors must be actual directories without reparse points")
		}
	}
	return nil
}

func EnsureDir(path string) (string, error) {
	absolute, err := localPath(path)
	if err != nil {
		return "", err
	}
	if err := walk(absolute, true); err != nil {
		return "", err
	}
	handle, err := openChecked(absolute, true)
	if err != nil {
		return "", err
	}
	return absolute, windows.CloseHandle(handle)
}

func CheckDir(path string) error {
	absolute, err := localPath(path)
	if err != nil {
		return err
	}
	if err := walk(absolute, false); err != nil {
		return err
	}
	handle, err := openChecked(absolute, true)
	if err != nil {
		return err
	}
	return windows.CloseHandle(handle)
}

func CheckFile(path string) error {
	absolute, err := localPath(path)
	if err != nil {
		return err
	}
	if err := walk(filepath.Dir(absolute), false); err != nil {
		return err
	}
	handle, err := openChecked(absolute, false)
	if err != nil {
		return err
	}
	return windows.CloseHandle(handle)
}

// OpenFile creates a private read/write file or validates an existing one on
// the same handle before returning it. It never truncates an existing object.
func OpenFile(path string) (*os.File, error) {
	absolute, err := localPath(path)
	if err != nil {
		return nil, err
	}
	if err := CheckDir(filepath.Dir(absolute)); err != nil {
		return nil, err
	}
	sd, err := descriptor(false)
	if err != nil {
		return nil, err
	}
	sa := windows.SecurityAttributes{Length: uint32(unsafe.Sizeof(windows.SecurityAttributes{})), SecurityDescriptor: sd}
	p := nativePath(absolute)
	handle, err := windows.CreateFile(p, windows.GENERIC_READ|windows.GENERIC_WRITE|windows.READ_CONTROL, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE, &sa, windows.OPEN_ALWAYS, windows.FILE_ATTRIBUTE_NORMAL|windows.FILE_FLAG_OPEN_REPARSE_POINT, 0)
	runtime.KeepAlive(sd)
	if err != nil {
		return nil, err
	}
	if err := checkSecurity(handle, false); err != nil {
		windows.CloseHandle(handle)
		return nil, err
	}
	file := os.NewFile(uintptr(handle), absolute)
	if file == nil {
		windows.CloseHandle(handle)
		return nil, errors.New("private file handle unavailable")
	}
	return file, nil
}

// OpenRead pins an existing private regular file and denies concurrent writers
// or deletion while borrowed. It cannot create a missing credential file.
func OpenRead(path string) (*os.File, error) {
	absolute, err := localPath(path)
	if err != nil {
		return nil, err
	}
	if err := walk(filepath.Dir(absolute), false); err != nil {
		return nil, err
	}
	handle, err := windows.CreateFile(nativePath(absolute), windows.GENERIC_READ|windows.READ_CONTROL, windows.FILE_SHARE_READ, nil, windows.OPEN_EXISTING, windows.FILE_FLAG_OPEN_REPARSE_POINT, 0)
	if err != nil {
		return nil, err
	}
	if err := checkSecurity(handle, false); err != nil {
		windows.CloseHandle(handle)
		return nil, err
	}
	file := os.NewFile(uintptr(handle), absolute)
	if file == nil {
		windows.CloseHandle(handle)
		return nil, errors.New("private read handle unavailable")
	}
	return file, nil
}

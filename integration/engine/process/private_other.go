// INPUT: Actual private native JSONL file metadata on non-Windows platforms
// OUTPUT: Native Pi file permission validation without changing existing files
// POS: [engine product bridge] File privacy is separate from exact session owner/cwd/header binding.
//go:build !windows

package process

import (
	"errors"
	"os"
)

func checkNativePrivateFile(_ string, info os.FileInfo) error {
	if info.Mode().Perm()&0077 != 0 {
		return errors.New("Pi native session must be a private regular file")
	}
	return nil
}

func reservedPlatformHome(string) bool               { return false }
func platformHome(string) (map[string]string, error) { return nil, nil }

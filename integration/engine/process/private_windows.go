// INPUT: Exact native JSONL path, pinned file identity and Windows owner/DACL checks
// OUTPUT: Windows private Pi file validation without assuming Unix permission bits
// POS: [engine product bridge] Native file security does not grant another product or workspace resume rights.
//go:build windows

package process

import (
	"errors"
	"github.com/QuincySx/Alva/platform/system/privatefs"
	"os"
	"path/filepath"
	"strings"
)

func checkNativePrivateFile(path string, info os.FileInfo) error {
	if err := privatefs.CheckFile(path); err != nil {
		return err
	}
	current, err := os.Lstat(path)
	if err != nil || !os.SameFile(info, current) {
		return errors.New("private native session changed during ACL validation")
	}
	return nil
}

func reservedPlatformHome(key string) bool {
	switch strings.ToUpper(key) {
	case "HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "TEMP", "TMP", "HOMEDRIVE", "HOMEPATH", "CODEX_HOME", "CLAUDE_CONFIG_DIR", "PI_CODING_AGENT_DIR":
		return true
	}
	return false
}

func platformHome(home string) (map[string]string, error) {
	appData, err := privatefs.EnsureDir(filepath.Join(home, "appdata"))
	if err != nil {
		return nil, err
	}
	temp, err := privatefs.EnsureDir(filepath.Join(home, "tmp"))
	if err != nil {
		return nil, err
	}
	volume := filepath.VolumeName(home)
	return map[string]string{"USERPROFILE": home, "APPDATA": appData, "LOCALAPPDATA": appData, "TEMP": temp, "TMP": temp, "HOMEDRIVE": volume, "HOMEPATH": strings.TrimPrefix(home, volume)}, nil
}

// INPUT: Explicit source/interpreter configuration and product-approved canonical bundle digest
// OUTPUT: AEPBundle and pure bounded snapshot validation, never manifest discovery or process launch
// POS: [L7 Hub product] Product next-turn settings; approval is not an OS sandbox or a hash of the interpreter binary
package hub

import (
	"encoding/hex"
	"errors"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

type AEPBundle struct {
	ID                 string            `json:"id"`
	SourceDirectory    string            `json:"source_directory"`
	Interpreter        string            `json:"interpreter"`
	InterpreterArgs    []string          `json:"interpreter_args,omitempty"`
	Env                map[string]string `json:"env,omitempty"`
	Name               string            `json:"name,omitempty"`
	Version            string            `json:"version,omitempty"`
	SHA256             string            `json:"sha256,omitempty"`
	Enabled            bool              `json:"enabled"`
	HostAccessApproved bool              `json:"host_access_approved"`
}

func ValidateAEP(entries []AEPBundle) error {
	if len(entries) > 8 {
		return errors.New("at most 8 explicit AEP bundles per product session")
	}
	seen := make(map[string]bool, len(entries))
	pluginNames := make(map[string]bool, len(entries))
	bytes := 0
	for _, entry := range entries {
		if entry.ID == "" || len(entry.ID) > 128 || !utf8.ValidString(entry.ID) || strings.ContainsAny(entry.ID, "\x00\r\n") || seen[entry.ID] {
			return errors.New("AEP bundle identity must be unique and bounded")
		}
		seen[entry.ID] = true
		paths := []string{entry.SourceDirectory}
		if entry.Interpreter != "" {
			paths = append(paths, entry.Interpreter)
		} else if len(entry.InterpreterArgs) != 0 || len(entry.Env) != 0 {
			return errors.New("embedded WASM AEP does not accept interpreter arguments or environment")
		}
		for _, path := range paths {
			if !filepath.IsAbs(path) || len(path) > 4096 || !utf8.ValidString(path) || strings.ContainsRune(path, 0) {
				return errors.New("explicit absolute AEP source and interpreter paths required")
			}
			bytes += len(path)
		}
		if len(entry.InterpreterArgs) > 128 || len(entry.Env) > 64 || len(entry.Name) > 64 || len(entry.Version) > 64 || !utf8.ValidString(entry.Name+entry.Version) {
			return errors.New("AEP snapshot arguments/environment/metadata exceed bounds")
		}
		if entry.Name != "" {
			if pluginNames[entry.Name] {
				return errors.New("AEP manifest names must be unique within one product session")
			}
			pluginNames[entry.Name] = true
		}
		for _, arg := range entry.InterpreterArgs {
			if strings.ContainsRune(arg, 0) || !utf8.ValidString(arg) {
				return errors.New("invalid explicit AEP interpreter argument")
			}
			bytes += len(arg)
		}
		for key, value := range entry.Env {
			if key == "" || len(key) > 128 || strings.ContainsAny(key, "=\x00") || len(value) > 8192 || strings.ContainsRune(value, 0) || !utf8.ValidString(key+value) {
				return errors.New("invalid explicit AEP environment")
			}
			bytes += len(key) + len(value)
		}
		if entry.SHA256 != "" {
			pin, err := hex.DecodeString(entry.SHA256)
			if err != nil || len(pin) != 32 {
				return errors.New("AEP approved pin must be a SHA-256 digest")
			}
		}
		if entry.Enabled && (!entry.HostAccessApproved || entry.SHA256 == "" || entry.Name == "" || entry.Version == "") {
			return errors.New("AEP enablement requires an inspected pin and separate explicit host access approval")
		}
	}
	if bytes > 64<<10 {
		return errors.New("AEP process snapshots exceed 64 KiB total budget")
	}
	return nil
}

// INPUT: bounded TOML Plugin manifest and protocol capability vocabulary
// OUTPUT: Manifest, ParseManifest and Validate
// POS: [L4 extension] Declarative external bundle contract, not discovery, approval or executable code
package loader

import (
	"bytes"
	"encoding/hex"
	"errors"
	"io/fs"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/pelletier/go-toml/v2"
)

type Manifest struct {
	Name                  string   `toml:"name" json:"name"`
	Version               string   `toml:"version" json:"version"`
	Description           *string  `toml:"description" json:"description"`
	Author                *string  `toml:"author" json:"author"`
	Runtime               string   `toml:"runtime" json:"runtime"`
	Entry                 string   `toml:"entry" json:"entry"`
	RequestedCapabilities []string `toml:"requested_capabilities" json:"requested_capabilities"`
	ContentSHA256         *string  `toml:"content_sha256" json:"content_sha256"`
}

func ParseManifest(data []byte) (Manifest, error) {
	var manifest Manifest
	if len(data) > 64<<10 || !utf8.Valid(data) {
		return manifest, errors.New("Plugin manifest exceeds 64 KiB or is not UTF-8")
	}
	decoder := toml.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&manifest); err != nil {
		return manifest, errors.New("invalid Plugin TOML manifest")
	}
	return manifest, manifest.Validate()
}
func (m Manifest) Validate() error {
	if m.Name == "" || len(m.Name) > 64 || m.Version == "" || len(m.Version) > 64 || strings.IndexFunc(m.Version, unicode.IsControl) >= 0 {
		return errors.New("invalid Plugin identity")
	}
	for _, char := range m.Name {
		if !(char >= 'a' && char <= 'z' || char >= 'A' && char <= 'Z' || char >= '0' && char <= '9' || strings.ContainsRune("-_.", char)) {
			return errors.New("Plugin name must use ASCII letters, numbers, dash, underscore or dot")
		}
	}
	if m.Entry == "." || len(m.Entry) > 512 || !fs.ValidPath(m.Entry) || strings.Contains(m.Entry, "\\") {
		return errors.New("Plugin entry must be confined relative path")
	}
	if m.Runtime == "python" {
		if !strings.HasSuffix(m.Entry, ".py") {
			return errors.New("Python Plugin entry must be .py")
		}
	} else if m.Runtime == "javascript" {
		if !strings.HasSuffix(m.Entry, ".js") && !strings.HasSuffix(m.Entry, ".mjs") && !strings.HasSuffix(m.Entry, ".cjs") {
			return errors.New("JavaScript Plugin entry extension invalid")
		}
	} else if m.Runtime == "wasm" {
		if !strings.HasSuffix(m.Entry, ".wasm") {
			return errors.New("WASM Plugin entry must be .wasm")
		}
	} else {
		return errors.New("unsupported Plugin runtime")
	}
	if len(m.RequestedCapabilities) > 32 {
		return errors.New("Plugin capability limit exceeded")
	}
	seen := map[string]bool{}
	for _, capability := range m.RequestedCapabilities {
		if len(capability) > 128 || !strings.HasPrefix(capability, "host:") || len(capability) <= 5 || seen[capability] {
			return errors.New("invalid or duplicated Plugin capability")
		}
		for _, char := range capability {
			if !(char >= 'a' && char <= 'z' || char >= 'A' && char <= 'Z' || char >= '0' && char <= '9' || strings.ContainsRune(":.-_", char)) {
				return errors.New("invalid Plugin capability characters")
			}
		}
		seen[capability] = true
	}
	if m.ContentSHA256 != nil {
		data, err := hex.DecodeString(*m.ContentSHA256)
		if err != nil || len(data) != 32 {
			return errors.New("Plugin pin must be a SHA-256 hex digest")
		}
	}
	if m.Description != nil && len(*m.Description) > 16<<10 || m.Author != nil && len(*m.Author) > 1024 {
		return errors.New("Plugin metadata exceeds bounds")
	}
	return nil
}

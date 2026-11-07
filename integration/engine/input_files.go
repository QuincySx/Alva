// INPUT: Trusted host-captured immutable file identities and bounded local paths.
// OUTPUT: InputFile validation/copy for engine-neutral explicit attachments.
// POS: [L4.5 engine contract] Carries selected file facts without reading files or granting filesystem permissions.
package engine

import (
	"encoding/hex"
	"errors"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

type InputFile struct {
	ID         string `json:"id"`
	Name       string `json:"name"`
	URI        string `json:"uri"`
	Path       string `json:"path"`
	MediaType  string `json:"media_type"`
	SHA256     string `json:"sha256"`
	Bytes      int64  `json:"bytes"`
	TextURI    string `json:"text_uri,omitempty"`
	TextPath   string `json:"text_path,omitempty"`
	TextSHA256 string `json:"text_sha256,omitempty"`
	TextBytes  int64  `json:"text_bytes,omitempty"`
}

func CloneInputFiles(files []InputFile) []InputFile { return append([]InputFile(nil), files...) }

func ValidateInputFiles(files []InputFile) error {
	if len(files) > 8 {
		return errors.New("at most eight explicit engine input files")
	}
	text := func(s string, max int) bool {
		return s != "" && len(s) <= max && utf8.ValidString(s) && !strings.ContainsRune(s, 0)
	}
	hash := func(s string) bool {
		b, err := hex.DecodeString(s)
		return err == nil && len(b) == 32 && s == strings.ToLower(s)
	}
	seen := make(map[string]bool)
	for _, f := range files {
		if !text(f.ID, 128) || seen[f.ID] || !text(f.Name, 512) || !text(f.URI, 1024) || !text(f.Path, 4096) || !filepath.IsAbs(f.Path) || !text(f.MediaType, 256) || !hash(f.SHA256) || f.Bytes < 0 || f.Bytes > 1<<40 {
			return errors.New("invalid explicit engine input file")
		}
		seen[f.ID] = true
		if f.TextPath == "" {
			if f.TextURI != "" || f.TextSHA256 != "" || f.TextBytes != 0 {
				return errors.New("text derivative has no captured path")
			}
		} else if !text(f.TextPath, 4096) || !filepath.IsAbs(f.TextPath) || !text(f.TextURI, 1024) || !hash(f.TextSHA256) || f.TextBytes < 0 || f.TextBytes > 1<<30 {
			return errors.New("invalid captured text derivative")
		}
	}
	return nil
}

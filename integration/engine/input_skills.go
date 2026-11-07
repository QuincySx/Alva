// INPUT: Explicit host-selected Skill names, canonical source paths and immutable source hashes.
// OUTPUT: InputSkill validation and copy for exact next-Turn Skill admission.
// POS: [L4.5 engine contract] Carries selection facts without discovering Skills, reading host files or changing execution policy.
package engine

import (
	"encoding/hex"
	"errors"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

type InputSkill struct {
	Name   string `json:"name"`
	Path   string `json:"path"`
	SHA256 string `json:"sha256"`
}

func CloneInputSkills(skills []InputSkill) []InputSkill { return append([]InputSkill(nil), skills...) }

func ValidateInputSkills(skills []InputSkill) error {
	if len(skills) > 8 {
		return errors.New("at most eight explicit engine input Skills")
	}
	seen := make(map[string]bool, len(skills))
	for _, s := range skills {
		hash, err := hex.DecodeString(s.SHA256)
		if s.Name == "" || len(s.Name) > 128 || !utf8.ValidString(s.Name+s.Path) || strings.ContainsAny(s.Name, "/\\\x00\r\n") || seen[s.Name] || len(s.Path) > 4096 || strings.ContainsRune(s.Path, 0) || !filepath.IsAbs(s.Path) || err != nil || len(hash) != 32 || strings.ToLower(s.SHA256) != s.SHA256 {
			return errors.New("invalid or duplicate explicit engine input Skill")
		}
		seen[s.Name] = true
	}
	return nil
}

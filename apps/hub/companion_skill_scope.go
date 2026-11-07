// INPUT: Explicit pinned Skill identities from a companion's saved source selection.
// OUTPUT: Bounded, cloneable next-Turn allowlist, with nil distinct from an explicitly empty scope.
// POS: [L7 Hub product] Product assembly constraint only; no SDK policy or file access.
package hub

import (
	"encoding/hex"
	"errors"
)

// CompanionSkillScope restricts the actual repository exposed to a companion.
// Nil means an ordinary session; a non-nil empty scope permits no Skills.
type CompanionSkillScope struct {
	Skills []CompanionSkill `json:"skills"`
}

func (s *CompanionSkillScope) Clone() *CompanionSkillScope {
	if s == nil {
		return nil
	}
	return &CompanionSkillScope{Skills: append([]CompanionSkill{}, s.Skills...)}
}

func (s *CompanionSkillScope) Validate() error {
	if s == nil {
		return nil
	}
	if len(s.Skills) > 8 {
		return errors.New("at most 8 pinned companion Skills")
	}
	seen := map[string]bool{}
	for _, entry := range s.Skills {
		if !companionText(entry.SourceSessionID, 128) || !companionText(entry.SourceBranchID, 128) || !companionText(entry.Name, 128) || len(entry.SHA256) != 64 || seen[entry.Name] {
			return errors.New("invalid or duplicate scoped companion Skill")
		}
		if _, err := hex.DecodeString(entry.SHA256); err != nil {
			return errors.New("scoped Skill SHA256 must be hexadecimal")
		}
		seen[entry.Name] = true
	}
	return nil
}

// INPUT: Explicit per-model favorites, Agent preference and optional generation defaults.
// OUTPUT: Stable account/model keys, shared validation and an independent optimistic storage port.
// POS: [L7 Hub product] Preferences do not mutate connection authority or choose an execution route.
package hub

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/integration/engine"
)

var ErrModelPreferenceConflict = errors.New("model preference changed; reload before saving")

type ModelOptions struct {
	Generation   engine.GenerationOptions `json:"generation"`
	SystemPrompt string                   `json:"system_prompt,omitempty"`
	MaxTurns     int                      `json:"max_turns,omitempty"`
}

func (v ModelOptions) Validate() error {
	if err := v.Generation.Validate(); err != nil {
		return err
	}
	if v.MaxTurns < 0 || v.MaxTurns > 1024 || len(v.SystemPrompt) > 8<<10 || !utf8.ValidString(v.SystemPrompt) || strings.ContainsRune(v.SystemPrompt, 0) {
		return errors.New("model options exceed supported prompt or iteration bounds")
	}
	return nil
}
func (v ModelOptions) Clone() ModelOptions { v.Generation = v.Generation.Clone(); return v }

// ValidateTarget rejects unsupported defaults before a Turn is admitted. The
// actual registered adapter still verifies its own declared capabilities.
func (v ModelOptions) ValidateTarget(target Target) error {
	if err := v.Validate(); err != nil {
		return err
	}
	if (target.EngineID == "claude" || target.EngineID == "codex" || target.EngineID == "pi") && !v.Generation.Empty() {
		return engine.ErrUnsupported
	}
	if (target.EngineID == "codex" || target.EngineID == "pi") && (v.SystemPrompt != "" || v.MaxTurns != 0) {
		return engine.ErrUnsupported
	}
	return nil
}

type ModelPreference struct {
	Key             string       `json:"key"`
	Revision        uint64       `json:"revision"`
	Favorite        bool         `json:"favorite"`
	PreferredEngine string       `json:"preferred_engine,omitempty"`
	Options         ModelOptions `json:"options"`
}

func (v ModelPreference) Validate() error {
	x, e := hex.DecodeString(v.Key)
	if e != nil || len(x) != sha256.Size || len(v.PreferredEngine) > 64 || strings.ContainsAny(v.PreferredEngine, "\x00\r\n") || !utf8.ValidString(v.PreferredEngine) {
		return errors.New("invalid model preference identity")
	}
	return v.Options.Validate()
}
func (v ModelPreference) Clone() ModelPreference { v.Options = v.Options.Clone(); return v }

type ModelPreferencesSnapshot struct {
	Revision uint64            `json:"revision"`
	Entries  []ModelPreference `json:"entries"`
}

func (v ModelPreferencesSnapshot) Clone() ModelPreferencesSnapshot {
	v.Entries = append([]ModelPreference(nil), v.Entries...)
	for i := range v.Entries {
		v.Entries[i] = v.Entries[i].Clone()
	}
	return v
}
func (v ModelPreferencesSnapshot) Validate() error {
	if v.Revision == 0 || len(v.Entries) > 512 {
		return errors.New("invalid model preference snapshot")
	}
	seen := map[string]bool{}
	for _, p := range v.Entries {
		if p.Revision == 0 || p.Validate() != nil || seen[p.Key] {
			return errors.New("invalid or duplicated model preference")
		}
		seen[p.Key] = true
	}
	return nil
}
func ModelPreferenceKey(target Target) string {
	account := target.AccountSource
	identity := []string{target.ProviderID, target.ModelID, account, target.BillingRoute}
	if strings.HasPrefix(account, "connection:") {
		if i := strings.LastIndexByte(account, '@'); i > len("connection:") {
			account = account[:i]
			identity = []string{"connection", account, target.ModelID}
		}
	}
	data, _ := json.Marshal(identity)
	h := sha256.Sum256(data)
	return hex.EncodeToString(h[:])
}

type ModelPreferencesStore interface {
	ModelPreferences(context.Context) (ModelPreferencesSnapshot, error)
	SaveModelPreference(context.Context, ModelPreference) (ModelPreferencesSnapshot, error)
}

// INPUT: Explicit account/model identities and optional engine generation values.
// OUTPUT: Stable preference identity, isolated clones and invalid-value regressions.
// POS: [L7 Hub product testing] Pure values without storage, provider or execution dependencies.
package hub

import (
	"errors"
	"math"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/integration/engine"
)

func preferenceFloat(v float64) *float64 { return &v }
func preferenceTokens(v uint32) *uint32  { return &v }

func TestModelPreferenceKeyRetainsConnectionAndAccountIdentity(t *testing.T) {
	base := Target{EngineID: "alva", ModelID: "exact-model", ProviderID: "openai_chat", AccountSource: "connection:work@1", BillingRoute: "https://example.invalid/chat"}
	changed := base
	changed.AccountSource, changed.ProviderID, changed.BillingRoute, changed.EngineID = "connection:work@9", "openai_responses", "https://example.invalid/responses", "pi"
	if ModelPreferenceKey(base) != ModelPreferenceKey(changed) {
		t.Fatal("connection revision, transport or compatible Agent changed the model preference identity")
	}
	changed.AccountSource = "connection:personal@9"
	if ModelPreferenceKey(base) == ModelPreferenceKey(changed) {
		t.Fatal("different connection accounts shared preferences")
	}
	base.AccountSource = "explicit-static-account"
	for _, field := range []string{"account", "billing", "provider", "model"} {
		changed = base
		switch field {
		case "account":
			changed.AccountSource = "other-static-account"
		case "billing":
			changed.BillingRoute = "other-billing"
		case "provider":
			changed.ProviderID = "other-provider"
		case "model":
			changed.ModelID = "other-model"
		}
		if ModelPreferenceKey(base) == ModelPreferenceKey(changed) {
			t.Fatalf("static %s boundary disappeared", field)
		}
	}
	changed = base
	changed.EngineID = "claude"
	if ModelPreferenceKey(base) != ModelPreferenceKey(changed) {
		t.Fatal("compatible Agent changed the same static account/model identity")
	}
}

func TestModelPreferenceClonesDoNotShareGenerationPointers(t *testing.T) {
	p := ModelPreference{Key: ModelPreferenceKey(Target{ModelID: "m"}), Revision: 1, Options: ModelOptions{Generation: engine.GenerationOptions{Temperature: preferenceFloat(.3), MaxTokens: preferenceTokens(640)}, MaxTurns: 4}}
	cloned := p.Clone()
	*cloned.Options.Generation.Temperature, *cloned.Options.Generation.MaxTokens = .9, 999
	if *p.Options.Generation.Temperature != .3 || *p.Options.Generation.MaxTokens != 640 {
		t.Fatal("preference clone shared optional numeric pointers")
	}
	snapshot := ModelPreferencesSnapshot{Revision: 4, Entries: []ModelPreference{p}}
	copy := snapshot.Clone()
	copy.Entries[0].Key = "changed"
	*copy.Entries[0].Options.Generation.Temperature = 1.2
	if snapshot.Entries[0].Key != p.Key || *p.Options.Generation.Temperature != .3 {
		t.Fatal("snapshot clone shared entries or generation pointers")
	}
}

func TestModelOptionsZeroValuesKeepSettingsValid(t *testing.T) {
	options := ModelOptions{}
	if err := options.Validate(); err != nil || !options.Generation.Empty() {
		t.Fatal("zero options no longer preserve inherited defaults", err)
	}
	settings := Settings{Version: 1, SessionID: "fixture"}
	if err := settings.Validate(); err != nil {
		t.Fatal("adding optional model defaults broke existing empty settings", err)
	}
	settings.ModelOptions.MaxTurns = -1
	if err := settings.Validate(); err == nil {
		t.Fatal("settings validation ignored invalid model options")
	}
}

func TestModelOptionsValidateTargetRejectsUnsupportedDefaults(t *testing.T) {
	generation := ModelOptions{Generation: engine.GenerationOptions{Temperature: preferenceFloat(.4), MaxTokens: preferenceTokens(800), ReasoningEffort: "low"}}
	prompt := ModelOptions{SystemPrompt: "Explicit instruction", MaxTurns: 4}
	for _, test := range []struct {
		name, agent string
		options     ModelOptions
		unsupported bool
	}{
		{"alva-generation", "alva", generation, false},
		{"alva-prompt", "alva", prompt, false},
		{"claude-prompt", "claude", prompt, false},
		{"claude-generation", "claude", generation, true},
		{"pi-generation", "pi", generation, true},
		{"pi-prompt", "pi", prompt, true},
		{"codex-generation", "codex", generation, true},
		{"codex-prompt", "codex", prompt, true},
		{"pi-inheritance", "pi", ModelOptions{}, false},
		{"codex-inheritance", "codex", ModelOptions{}, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			err := test.options.ValidateTarget(Target{EngineID: test.agent})
			if test.unsupported && !errors.Is(err, engine.ErrUnsupported) || !test.unsupported && err != nil {
				t.Fatal("target support was not enforced before admission", err)
			}
		})
	}
	invalid := ModelOptions{MaxTurns: -1}
	if err := invalid.ValidateTarget(Target{EngineID: "alva"}); err == nil {
		t.Fatal("target validation accepted invalid option values")
	}
}

func TestModelPreferenceRejectsInvalidValuesAndSnapshotShapes(t *testing.T) {
	base := ModelPreference{Key: ModelPreferenceKey(Target{ModelID: "m"}), Revision: 1}
	if err := base.Validate(); err != nil {
		t.Fatal(err)
	}
	for _, test := range []struct {
		name    string
		options ModelOptions
	}{
		{"negative-temperature", ModelOptions{Generation: engine.GenerationOptions{Temperature: preferenceFloat(-1)}}},
		{"nan-temperature", ModelOptions{Generation: engine.GenerationOptions{Temperature: preferenceFloat(math.NaN())}}},
		{"infinite-temperature", ModelOptions{Generation: engine.GenerationOptions{Temperature: preferenceFloat(math.Inf(1))}}},
		{"zero-output", ModelOptions{Generation: engine.GenerationOptions{MaxTokens: preferenceTokens(0)}}},
		{"unknown-reasoning", ModelOptions{Generation: engine.GenerationOptions{ReasoningEffort: "invented"}}},
		{"negative-turns", ModelOptions{MaxTurns: -1}},
		{"unbounded-turns", ModelOptions{MaxTurns: 1025}},
		{"unbounded-system", ModelOptions{SystemPrompt: strings.Repeat("a", 8<<10+1)}},
		{"nul-system", ModelOptions{SystemPrompt: "before\x00after"}},
		{"invalid-utf8-system", ModelOptions{SystemPrompt: string([]byte{255})}},
	} {
		t.Run(test.name, func(t *testing.T) {
			value := base.Clone()
			value.Options = test.options
			if err := value.Validate(); err == nil {
				t.Fatal("invalid options were accepted")
			}
		})
	}
	for _, key := range []string{"", "not-hex", strings.Repeat("a", 62)} {
		value := base
		value.Key = key
		if err := value.Validate(); err == nil {
			t.Fatal("invalid model key accepted")
		}
	}
	for _, name := range []string{strings.Repeat("a", 65), "bad\nengine", string([]byte{255})} {
		value := base
		value.PreferredEngine = name
		if err := value.Validate(); err == nil {
			t.Fatal("invalid Agent preference accepted")
		}
	}
	for _, snapshot := range []ModelPreferencesSnapshot{
		{}, {Revision: 1, Entries: []ModelPreference{{Key: base.Key}}},
		{Revision: 1, Entries: []ModelPreference{base, base}},
		{Revision: 1, Entries: make([]ModelPreference, 513)},
	} {
		if err := snapshot.Validate(); err == nil {
			t.Fatal("invalid model preference snapshot accepted")
		}
	}
	if err := (ModelPreferencesSnapshot{Revision: 1, Entries: []ModelPreference{base}}).Validate(); err != nil {
		t.Fatal(err)
	}
}

// The preference key follows the route key: a legacy "@revision" target, the
// same route saved now, and the same route after an endpoint move all share one
// preference; another connection or model does not.
func TestModelPreferenceKeySurvivesLegacySuffixAndEndpointMove(t *testing.T) {
	c := ModelConnection{ID: "connection-x", Revision: 3, Protocol: "openai_chat", Endpoint: "https://a.example/v1"}
	m := ConnectionModel{ID: "gpt"}
	legacy := c.Target(m)
	legacy.AccountSource += "@3"
	current := c.Target(m)
	c.Endpoint = "https://b.example/v1"
	moved := c.Target(m)
	if ModelPreferenceKey(legacy) != ModelPreferenceKey(current) || ModelPreferenceKey(current) != ModelPreferenceKey(moved) {
		t.Fatal("legacy, current and endpoint-moved targets of one route must share a preference key")
	}
	other := current
	other.AccountSource = "connection:other"
	otherModel := current
	otherModel.ModelID = "other"
	if ModelPreferenceKey(other) == ModelPreferenceKey(current) || ModelPreferenceKey(otherModel) == ModelPreferenceKey(current) {
		t.Fatal("another connection or model collapsed into the same preference")
	}
}

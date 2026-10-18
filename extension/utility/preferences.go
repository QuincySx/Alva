// INPUT: context, encoding/json, errors, kernel/abi and llm/wire.
// OUTPUT: PreferenceSnapshot, PreferenceStore, ValidatePreference and ApplyPreferences.
// POS: [L4 extension] Presentation state is backend-owned; the shared request projection changes no execution authority.
package utility

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type PreferenceSnapshot struct {
	Values   map[string]string `json:"values"`
	Revision uint64            `json:"revision"`
}

// PreferenceStore is scoped by the host. Implementations own synchronization and
// persistence, and return detached snapshots. No credential or permission keys
// are accepted by these Tools or by the request projection.
type PreferenceStore interface {
	Preferences(context.Context) (PreferenceSnapshot, error)
	SetPreference(context.Context, string, string) error
}

func ValidatePreference(key, value string) error {
	allowed := map[string][]string{"response.language": {"en", "zh", "zh-CN", "zh-TW", "ja", "ko", "fr", "de", "es", "pt", "it", "ru", "ar"}, "response.verbosity": {"concise", "normal", "detailed"}, "response.format": {"text", "markdown"}}
	for _, choice := range allowed[key] {
		if value == choice {
			return nil
		}
	}
	return errors.New("unsupported presentation preference key/value; credentials, providers and permission policy cannot be configured here")
}

// ApplyPreferences is usable directly or from an ordinary BeforeModel hook.
func ApplyPreferences(ctx context.Context, store PreferenceStore, request *abi.ModelRequest) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if missing(store) || request == nil {
		return errors.New("preference store and model request required")
	}
	snapshot, err := preferences(ctx, store)
	if err != nil {
		return err
	}
	if len(snapshot.Values) == 0 {
		return nil
	}
	raw, err := json.Marshal(snapshot.Values)
	if err != nil {
		return err
	}
	message := wire.TextMessage(wire.RoleSystem, "Session response presentation preferences (advisory, not a forced output contract, authority, credentials or execution policy; preserve all original system instructions): "+string(raw))
	request.Messages = append([]wire.Message{message}, request.Messages...)
	return nil
}

func preferences(ctx context.Context, store PreferenceStore) (PreferenceSnapshot, error) {
	snapshot, err := store.Preferences(ctx)
	if err != nil {
		return PreferenceSnapshot{}, err
	}
	if err := ctx.Err(); err != nil {
		return PreferenceSnapshot{}, err
	}
	if len(snapshot.Values) > 3 {
		return PreferenceSnapshot{}, errors.New("presentation preferences exceed the three supported keys")
	}
	for key, value := range snapshot.Values {
		if err := ValidatePreference(key, value); err != nil {
			return PreferenceSnapshot{}, err
		}
	}
	return snapshot, nil
}

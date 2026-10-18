// INPUT: Native/operator tool configuration JSON and independent session capability choices
// OUTPUT: SaveToolsDraft and SetToolEnabled with strict bounded parsing and next-turn policy
// POS: [L7 Hub product] Explicit product tool editor; saving configurations starts no resources.
package product

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/QuincySx/Alva/go/platform/system/httpjson"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func (s *HubService) SaveToolsDraft(ctx context.Context, id, raw string) error {
	var value hub.ToolSettings
	if len(raw) > 64<<10 || !wire.ValidJSON([]byte(raw)) || httpjson.ValidateObject([]byte(raw), &value) != nil {
		return errors.New("strict tool configuration object within 64 KiB required")
	}
	if err := json.Unmarshal([]byte(raw), &value); err != nil {
		return err
	}
	store, ok := s.Store.(hub.ToolSettingsStore)
	if !ok {
		return errors.New("tool settings store unavailable")
	}
	return store.ReplaceTools(ctx, id, value)
}
func (s *HubService) SetToolEnabled(ctx context.Context, id, kind string, enabled, approved bool) error {
	store, ok := s.Store.(hub.ToolSettingsStore)
	if !ok {
		return errors.New("tool settings store unavailable")
	}
	return store.SetToolEnabled(ctx, id, kind, enabled, approved)
}

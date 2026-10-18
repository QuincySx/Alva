// INPUT: Native/operator-only explicit browser JSON and the claimed product session
// OUTPUT: SaveBrowserDraft with disabled imported flags and strict bounded shape
// POS: [L7 Hub product] Not an HTTP host-path import, browser discovery, profile migration or launch endpoint
package product

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"

	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func (s *HubService) SaveBrowserDraft(ctx context.Context, id, raw string) error {
	if len(raw) > 48<<10 || !wire.ValidJSON([]byte(raw)) || len(bytes.TrimSpace([]byte(raw))) == 0 || bytes.TrimSpace([]byte(raw))[0] != '{' {
		return errors.New("browser draft requires one explicit JSON object within 48 KiB")
	}
	var value hub.BrowserSettings
	d := json.NewDecoder(bytes.NewBufferString(raw))
	d.DisallowUnknownFields()
	if d.Decode(&value) != nil || d.Decode(new(any)) != io.EOF {
		return errors.New("invalid explicit browser configuration fields")
	}
	value.Enabled, value.HostAccessApproved = false, false
	return s.Store.ReplaceBrowser(ctx, id, value)
}

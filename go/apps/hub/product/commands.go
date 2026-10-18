// INPUT: Explicit native/operator command draft JSON and product session authority
// OUTPUT: SaveCommandsDraft disabled snapshots with no discovery, file reads or executable startup
// POS: [L7 Hub product] Host command configuration editor, not an HTTP arbitrary-command import endpoint
package product

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"

	"github.com/QuincySx/Alva/go/apps/hub"
)

func (s *HubService) SaveCommandsDraft(ctx context.Context, id, raw string) error {
	if len(raw) > 64<<10 {
		return errors.New("native command draft exceeds 64 KiB")
	}
	if trimmed := bytes.TrimSpace([]byte(raw)); len(trimmed) == 0 || trimmed[0] != '{' {
		return errors.New("native command configuration must be an explicit object, not null")
	}
	decoder := json.NewDecoder(bytes.NewBufferString(raw))
	decoder.DisallowUnknownFields()
	var value hub.CommandSettings
	if err := decoder.Decode(&value); err != nil {
		return err
	}
	if decoder.Decode(new(any)) != io.EOF {
		return errors.New("trailing native command configuration")
	}
	return s.Store.ReplaceCommands(ctx, id, value)
}

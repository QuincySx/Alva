// INPUT: Actual product handoff metadata and the shared known-credential JSON redactor
// OUTPUT: Redacted bounded handoff snapshots before durability and engine delivery
// POS: [L7 Hub product] L7 product policy reuses existing sanitization without SDK dependencies or copied regex rules
package sqlite

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/platform/storage/redaction"
)

func protectHandoff(value *hub.Handoff) (*hub.Handoff, error) {
	data, err := json.Marshal(value)
	if err != nil {
		return nil, err
	}
	data, err = redaction.RedactJSON(data)
	if err != nil {
		return nil, err
	}
	var result hub.Handoff
	d := json.NewDecoder(bytes.NewReader(data))
	d.DisallowUnknownFields()
	if d.Decode(&result) != nil || d.Decode(new(any)) != io.EOF {
		return nil, errors.New("handoff redaction shape failed")
	}
	if _, err = hub.EncodeHandoff(&result); err != nil {
		return nil, err
	}
	return &result, nil
}

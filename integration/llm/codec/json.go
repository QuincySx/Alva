// INPUT: Bounded native JSON bytes with own wire Unicode validation.
// OUTPUT: Private strict validation rejecting duplicate keys, excessive nesting and trailing values.
// POS: [integration / protocol codec] Portable native JSON gate prevents ambiguous data being collapsed before validation or conversion.
package codec

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func validateNativeJSON(raw []byte, maximum int) error {
	if len(raw) == 0 || len(raw) > maximum || !wire.ValidJSON(raw) {
		return errors.New("native JSON exceeds bounds or contains invalid Unicode")
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.UseNumber()
	var visit func(int) error
	visit = func(depth int) error {
		if depth > 128 {
			return errors.New("native JSON nesting exceeds 128")
		}
		token, err := decoder.Token()
		if err != nil {
			return errors.New("invalid native JSON token")
		}
		delim, ok := token.(json.Delim)
		if !ok {
			return nil
		}
		switch delim {
		case '{':
			keys := map[string]bool{}
			for decoder.More() {
				keyToken, err := decoder.Token()
				if err != nil {
					return errors.New("invalid native JSON key")
				}
				key, ok := keyToken.(string)
				if !ok || keys[key] {
					return errors.New("native JSON contains duplicate object key")
				}
				keys[key] = true
				if err = visit(depth + 1); err != nil {
					return err
				}
			}
		case '[':
			for decoder.More() {
				if err := visit(depth + 1); err != nil {
					return err
				}
			}
		default:
			return errors.New("invalid native JSON delimiter")
		}
		_, err = decoder.Token()
		return err
	}
	if err := visit(0); err != nil {
		return err
	}
	if _, err := decoder.Token(); !errors.Is(err, io.EOF) {
		return errors.New("native JSON contains trailing data")
	}
	return nil
}

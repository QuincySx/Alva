// INPUT: bytes, encoding/json, errors, io, llm/wire
// OUTPUT: Encode, Decode
// POS: [L0 foundation] Deterministic bounded strict-UTF8 JSON boundary shared by native and guest capability proxies.
package abi

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type boundedBuffer struct {
	bytes.Buffer
	limit int
}

func (buffer *boundedBuffer) Write(data []byte) (int, error) {
	if len(data) > buffer.limit-buffer.Len() {
		return 0, errors.New("sandbox JSON byte budget exceeded")
	}
	return buffer.Buffer.Write(data)
}
func Encode(value any, limit int) ([]byte, error) {
	if limit < 1 || limit > MaxResponseBytes {
		return nil, errors.New("invalid sandbox encode limit")
	}
	buffer := boundedBuffer{limit: limit}
	if err := json.NewEncoder(&buffer).Encode(value); err != nil {
		return nil, err
	}
	if !wire.ValidJSON(buffer.Bytes()) {
		return nil, errors.New("invalid sandbox JSON")
	}
	return buffer.Bytes(), nil
}
func Decode(data []byte, limit int, value any) error {
	if limit < 1 || limit > MaxResponseBytes {
		return errors.New("invalid sandbox decode limit")
	}
	if len(data) < 2 || len(data) > limit || !wire.ValidJSON(data) {
		return errors.New("invalid or oversized sandbox JSON")
	}
	if err := uniqueJSON(json.NewDecoder(bytes.NewReader(data)), 0); err != nil {
		return err
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(value); err != nil {
		return err
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return errors.New("trailing sandbox JSON")
	}
	return nil
}

func uniqueJSON(decoder *json.Decoder, depth int) error {
	if depth > 64 {
		return errors.New("sandbox JSON nesting limit exceeded")
	}
	token, err := decoder.Token()
	if err != nil {
		return err
	}
	delim, compound := token.(json.Delim)
	if !compound {
		return nil
	}
	if delim == '{' {
		seen := map[string]bool{}
		for decoder.More() {
			key, err := decoder.Token()
			if err != nil {
				return err
			}
			name, ok := key.(string)
			if !ok || seen[name] {
				return errors.New("duplicate sandbox JSON field")
			}
			seen[name] = true
			if err := uniqueJSON(decoder, depth+1); err != nil {
				return err
			}
		}
	} else if delim == '[' {
		for decoder.More() {
			if err := uniqueJSON(decoder, depth+1); err != nil {
				return err
			}
		}
	} else {
		return errors.New("invalid sandbox JSON delimiter")
	}
	_, err = decoder.Token()
	return err
}

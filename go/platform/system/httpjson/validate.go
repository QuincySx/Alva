// INPUT: Bounded JSON bytes and an optional concrete transport DTO type
// OUTPUT: Validate and ValidateObject reject malformed Unicode, duplicate keys, excessive depth and ambiguous fields
// POS: [platform / OS primitive] Shared native JSON boundary independent of HTTP products, execution and model conversion
package httpjson

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"reflect"
	"strings"

	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func ValidateObject(data []byte, out any) error {
	if !wire.ValidJSON(data) {
		return errors.New("invalid JSON or Unicode")
	}

	t := reflect.TypeOf(out)
	if t == nil || t.Kind() != reflect.Pointer || t.Elem().Kind() != reflect.Struct {
		return errors.New("concrete object DTO required")
	}
	if len(data) == 0 || bytes.TrimSpace(data)[0] != '{' {
		return errors.New("object required")
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	if err := walkJSON(decoder, t.Elem(), 0); err != nil {
		return err
	}
	if _, err := decoder.Token(); err != io.EOF {
		return errors.New("trailing JSON")
	}
	return nil
}

func walkJSON(decoder *json.Decoder, t reflect.Type, depth int) error {
	if depth > 64 {
		return errors.New("JSON nesting exceeds transport limit")
	}
	for t != nil && t.Kind() == reflect.Pointer {
		t = t.Elem()
	}
	token, err := decoder.Token()
	if err != nil {
		return err
	}
	delim, ok := token.(json.Delim)
	if !ok {
		return nil
	}
	switch delim {
	case '{':
		var fields map[string]reflect.Type
		if t != nil && t.Kind() == reflect.Struct {
			fields = make(map[string]reflect.Type)
			for i := 0; i < t.NumField(); i++ {
				field := t.Field(i)
				if field.PkgPath != "" {
					continue
				}
				name := strings.Split(field.Tag.Get("json"), ",")[0]
				if name == "-" {
					continue
				}
				if name == "" {
					name = field.Name
				}
				fields[name] = field.Type
			}
		}
		seen := map[string]bool{}
		for decoder.More() {
			keyToken, err := decoder.Token()
			if err != nil {
				return err
			}
			key, ok := keyToken.(string)
			if !ok || seen[key] {
				return errors.New("duplicate JSON key")
			}
			seen[key] = true
			var child reflect.Type
			if fields != nil {
				var exists bool
				child, exists = fields[key]
				if !exists {
					return errors.New("unknown or incorrectly cased JSON key")
				}
			} else if t != nil && t.Kind() == reflect.Map {
				child = t.Elem()
			}
			if err = walkJSON(decoder, child, depth+1); err != nil {
				return err
			}
		}
		end, err := decoder.Token()
		if err != nil || end != json.Delim('}') {
			return errors.New("invalid JSON object")
		}
	case '[':
		var child reflect.Type
		if t != nil && (t.Kind() == reflect.Array || t.Kind() == reflect.Slice) {
			child = t.Elem()
		}
		for decoder.More() {
			if err := walkJSON(decoder, child, depth+1); err != nil {
				return err
			}
		}
		end, err := decoder.Token()
		if err != nil || end != json.Delim(']') {
			return errors.New("invalid JSON array")
		}
	default:
		return errors.New("unexpected JSON delimiter")
	}
	return nil
}

// Validate accepts an object-shaped protocol document without imposing a DTO
// schema. Unknown protocol fields stay available to the conversion-loss gate.
func Validate(data []byte) error {
	if !wire.ValidJSON(data) || len(bytes.TrimSpace(data)) == 0 || bytes.TrimSpace(data)[0] != '{' {
		return errors.New("valid JSON object required")
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	if err := walkJSON(decoder, nil, 0); err != nil {
		return err
	}
	if _, err := decoder.Token(); err != io.EOF {
		return errors.New("trailing JSON")
	}
	return nil
}

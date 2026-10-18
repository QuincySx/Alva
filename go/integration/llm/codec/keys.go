// INPUT: Raw native JSON and the exact private Magpie DTO schema before conversion.
// OUTPUT: Private noncanonical schema-key rejection without changing arbitrary tool input/schema business keys.
// POS: [integration / protocol codec] Go's case-insensitive struct matching cannot bypass exact native control and continuation guards.
package codec

import (
	"encoding/json"
	"errors"
	"reflect"
	"strings"
)

func checkDTOKeys(raw []byte, kind reflect.Type) error {
	for kind.Kind() == reflect.Pointer {
		kind = kind.Elem()
	}
	if kind == reflect.TypeFor[json.RawMessage]() || kind.Kind() == reflect.Interface {
		return nil
	}
	if kind.Kind() == reflect.Slice || kind.Kind() == reflect.Array {
		var values []json.RawMessage
		if json.Unmarshal(raw, &values) != nil {
			return nil
		}
		for _, value := range values {
			if err := checkDTOKeys(value, kind.Elem()); err != nil {
				return err
			}
		}
		return nil
	}
	if kind.Kind() != reflect.Struct {
		return nil
	}
	fields := map[string]reflect.Type{}
	var collect func(reflect.Type)
	collect = func(kind reflect.Type) {
		for i := 0; i < kind.NumField(); i++ {
			field := kind.Field(i)
			if field.PkgPath != "" {
				continue
			}
			tag := strings.Split(field.Tag.Get("json"), ",")[0]
			if tag == "-" {
				continue
			}
			if field.Anonymous && tag == "" {
				embedded := field.Type
				for embedded.Kind() == reflect.Pointer {
					embedded = embedded.Elem()
				}
				if embedded.Kind() == reflect.Struct {
					collect(embedded)
					continue
				}
			}
			if tag == "" {
				tag = field.Name
			}
			fields[tag] = field.Type
		}
	}
	collect(kind)
	var object map[string]json.RawMessage
	if json.Unmarshal(raw, &object) != nil {
		return nil
	}
	for key, value := range object {
		fieldType, ok := fields[key]
		if !ok {
			for canonical := range fields {
				if strings.EqualFold(key, canonical) {
					return errors.New("noncanonical native protocol key casing")
				}
			}
			continue
		}
		if err := checkDTOKeys(value, fieldType); err != nil {
			return err
		}
		// Native polymorphic message bodies are JSON/any in the external DTO.
		// Traverse only protocol-owned content containers, never input/args of
		// an actual tool or JSON schema/property/metadata business payload.
		if key == "content" || key == "contents" || key == "parts" || key == "summary" || key == "input" && kind.Name() == "rRequest" {
			if err := checkDynamicNativeKeys(value); err != nil {
				return err
			}
		}
	}
	return nil
}
func checkDynamicNativeKeys(raw []byte) error {
	var values []json.RawMessage
	if json.Unmarshal(raw, &values) == nil && values != nil {
		for _, value := range values {
			if err := checkDynamicNativeKeys(value); err != nil {
				return err
			}
		}
		return nil
	}
	var object map[string]json.RawMessage
	if json.Unmarshal(raw, &object) != nil {
		return nil
	}
	canonical := []string{"type", "role", "id", "call_id", "name", "arguments", "signature", "thoughtSignature", "thinking", "encrypted_content", "data", "text", "refusal", "content", "parts", "summary", "functionCall", "functionResponse"}
	for key, value := range object {
		for _, expected := range canonical {
			if key != expected && strings.EqualFold(key, expected) {
				return errors.New("noncanonical native content key casing")
			}
		}
		if key == "content" || key == "parts" || key == "summary" {
			if err := checkDynamicNativeKeys(value); err != nil {
				return err
			}
		}
	}
	return nil
}

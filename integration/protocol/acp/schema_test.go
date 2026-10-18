// INPUT: context, encoding/json, errors, fmt, math, os, reflect, regexp, strings, testing
// OUTPUT: Offline validation of real helper frames against pinned official ACP v1 schema projection
// POS: [integration / external protocol] Test-only structural JSON Schema subset, never a runtime schema loader or protocol converter.
package acp

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"os"
	"reflect"
	"regexp"
	"strings"
	"testing"
)

type schemaFixture struct{ defs map[string]any }

func loadSchema(t *testing.T) schemaFixture {
	t.Helper()
	data, err := os.ReadFile("testdata/schema-v1.json")
	if err != nil {
		t.Fatal(err)
	}
	var schema map[string]any
	if err := json.Unmarshal(data, &schema); err != nil {
		t.Fatal(err)
	}
	return schemaFixture{defs: schema["$defs"].(map[string]any)}
}
func (schema schemaFixture) validate(name string, value any) error {
	definition, ok := schema.defs[name]
	if !ok {
		return errors.New("missing official definition " + name)
	}
	return schema.node(definition, value, 0)
}
func (schema schemaFixture) node(raw, value any, depth int) error {
	if depth > 128 {
		return errors.New("schema test depth exceeded")
	}
	if allowed, ok := raw.(bool); ok {
		if allowed {
			return nil
		}
		return errors.New("schema forbids value")
	}
	spec, ok := raw.(map[string]any)
	if !ok {
		return errors.New("invalid fixture schema")
	}
	for key := range spec {
		switch key {
		case "$ref", "type", "properties", "required", "items", "additionalProperties", "allOf", "anyOf", "oneOf", "enum", "const", "format", "minimum", "maximum", "minItems", "maxItems", "minLength", "maxLength", "pattern":
		default:
			return errors.New("unsupported validation keyword in test subset: " + key)
		}
	}
	if ref, ok := spec["$ref"].(string); ok {
		definition, exists := schema.defs[strings.TrimPrefix(ref, "#/$defs/")]
		if !exists {
			return errors.New("schema reference missing")
		}
		if err := schema.node(definition, value, depth+1); err != nil {
			return err
		}
	}
	if expected, exists := spec["type"]; exists {
		check := func(kind string) bool {
			switch kind {
			case "object":
				_, ok := value.(map[string]any)
				return ok
			case "array":
				_, ok := value.([]any)
				return ok
			case "string":
				_, ok := value.(string)
				return ok
			case "number":
				_, ok := value.(float64)
				return ok
			case "integer":
				number, ok := value.(float64)
				return ok && number == math.Trunc(number)
			case "boolean":
				_, ok := value.(bool)
				return ok
			case "null":
				return value == nil
			}
			return false
		}
		valid := false
		if kind, ok := expected.(string); ok {
			valid = check(kind)
		} else {
			for _, kind := range expected.([]any) {
				valid = valid || check(kind.(string))
			}
		}
		if !valid {
			return fmt.Errorf("expected type %v, got %T", expected, value)
		}
	}
	for _, group := range []string{"allOf", "anyOf", "oneOf"} {
		if variants, ok := spec[group].([]any); ok {
			matches := 0
			for _, variant := range variants {
				if schema.node(variant, value, depth+1) == nil {
					matches++
				}
			}
			if group == "allOf" && matches != len(variants) || group == "anyOf" && matches == 0 || group == "oneOf" && matches != 1 {
				return fmt.Errorf("%s failed (%d matching)", group, matches)
			}
		}
	}
	if constant, exists := spec["const"]; exists && !reflect.DeepEqual(constant, value) {
		return errors.New("schema const mismatch")
	}
	if variants, ok := spec["enum"].([]any); ok {
		matched := false
		for _, variant := range variants {
			matched = matched || reflect.DeepEqual(variant, value)
		}
		if !matched {
			return errors.New("schema enum mismatch")
		}
	}
	if object, ok := value.(map[string]any); ok {
		if required, ok := spec["required"].([]any); ok {
			for _, key := range required {
				if _, exists := object[key.(string)]; !exists {
					return errors.New("missing required " + key.(string))
				}
			}
		}
		properties, _ := spec["properties"].(map[string]any)
		for key, field := range object {
			if definition, exists := properties[key]; exists {
				if err := schema.node(definition, field, depth+1); err != nil {
					return fmt.Errorf("%s: %w", key, err)
				}
			} else if additional, exists := spec["additionalProperties"]; exists {
				if err := schema.node(additional, field, depth+1); err != nil {
					return err
				}
			}
		}
	}
	if array, ok := value.([]any); ok {
		if min, ok := spec["minItems"].(float64); ok && len(array) < int(min) {
			return errors.New("array too short")
		}
		if max, ok := spec["maxItems"].(float64); ok && len(array) > int(max) {
			return errors.New("array too long")
		}
		if items, exists := spec["items"]; exists {
			for _, item := range array {
				if err := schema.node(items, item, depth+1); err != nil {
					return err
				}
			}
		}
	}
	if number, ok := value.(float64); ok {
		if min, ok := spec["minimum"].(float64); ok && number < min {
			return errors.New("number below minimum")
		}
		if max, ok := spec["maximum"].(float64); ok && number > max {
			return errors.New("number above maximum")
		}
	}
	if text, ok := value.(string); ok {
		if min, ok := spec["minLength"].(float64); ok && len([]rune(text)) < int(min) {
			return errors.New("text too short")
		}
		if max, ok := spec["maxLength"].(float64); ok && len([]rune(text)) > int(max) {
			return errors.New("text too long")
		}
		if pattern, ok := spec["pattern"].(string); ok {
			matched, err := regexp.MatchString(pattern, text)
			if err != nil || !matched {
				return errors.New("pattern mismatch")
			}
		}
	}
	// JSON Schema format is an annotation in the default 2020-12 vocabulary;
	// this structural fixture does not claim format assertion support.
	return nil
}

func TestOfficialPinnedSchemaValidatesRealACPFrames(t *testing.T) {
	schema := loadSchema(t)
	methods := map[string]string{"initialize": "Initialize", "session/new": "NewSession", "session/prompt": "Prompt", "authenticate": "Authenticate"}
	pending := map[string]string{}
	validated := 0
	observer := ObserverFunc(func(_ context.Context, event IOEvent) error {
		if event.Stage != "received" && event.Stage != "write_attempt" || event.Direction == "stderr" {
			return nil
		}
		var frame map[string]any
		if err := json.Unmarshal(event.Data, &frame); err != nil {
			return err
		}
		method, _ := frame["method"].(string)
		if method != "" {
			definition := ""
			if name, ok := methods[method]; ok {
				definition = name + "Request"
				pending[fmt.Sprint(frame["id"])] = name + "Response"
			}
			if method == "session/update" {
				definition = "SessionNotification"
			}
			if method == "session/request_permission" {
				definition = "RequestPermissionRequest"
				pending[fmt.Sprint(frame["id"])] = "RequestPermissionResponse"
			}
			if definition == "" {
				return fmt.Errorf("unexpected test method %s", method)
			}
			if err := schema.validate(definition, frame["params"]); err != nil {
				return fmt.Errorf("official %s: %w", definition, err)
			}
		} else {
			definition := pending[fmt.Sprint(frame["id"])]
			if err := schema.validate(definition, frame["result"]); err != nil {
				return fmt.Errorf("official %s: %w", definition, err)
			}
		}
		validated++
		return nil
	})
	c := openFixture(t, "permission", PermissionFunc(func(context.Context, PermissionRequest) (PermissionOutcome, error) {
		return PermissionOutcome{Outcome: "selected", OptionID: "yes"}, nil
	}), observer)
	if err := c.Authenticate(context.Background(), "local"); err != nil {
		t.Fatal(err)
	}
	s := newFixtureSession(t, c)
	if _, err := s.Prompt(context.Background(), []ContentBlock{Text("schema test")}); err != nil {
		t.Fatal(err)
	}
	if err := c.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	if validated < 10 {
		t.Fatal("did not validate complete exchange", validated)
	}
	if err := schema.validate("PromptRequest", map[string]any{"sessionId": "s", "prompt": []any{map[string]any{"type": "text"}}}); err == nil {
		t.Fatal("official schema accepted missing required text")
	}
	if err := schema.validate("PermissionResponse", map[string]any{}); err == nil {
		t.Fatal("schema unknown root accepted")
	}
}

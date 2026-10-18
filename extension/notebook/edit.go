// INPUT: bytes, encoding/json, errors, io, strings, unicode/utf8, llm/wire, platform/system/strictjson.
// OUTPUT: Strict bounded nbformat4.5 cell replacement/insertion/deletion preserving exact source and unknown fields.
// POS: [L4 extension] Notebook editing never executes cells and clears stale code outputs/execution metadata on changed source.
package notebook

import (
	"bytes"
	"encoding/json"
	"errors"
	strictjson "github.com/QuincySx/Alva/platform/system/strictjson"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"io"
	"strings"
	"unicode/utf8"
)

type editArguments struct {
	Path     string  `json:"notebook_path"`
	ID       string  `json:"cell_id"`
	Source   *string `json:"new_source"`
	CellType string  `json:"cell_type"`
	Mode     string  `json:"edit_mode"`
	Before   string  `json:"insert_before"`
}
type editResult struct {
	Data           []byte
	Index          int
	ClearedOutputs bool
	CellType       string
}

func validID(id string) bool {
	if len(id) < 1 || len(id) > 64 {
		return false
	}
	for _, c := range id {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '-' || c == '_') {
			return false
		}
	}
	return true
}
func decode(raw json.RawMessage) (editArguments, error) {
	var a editArguments
	if len(raw) > 256<<10 || !wire.ValidJSON(raw) {
		return a, errors.New("notebook arguments exceed 256 KiB UTF-8")
	}
	if err := strictjson.ValidateObject(raw, &a); err != nil {
		return a, err
	}
	var fields map[string]json.RawMessage
	if json.Unmarshal(raw, &fields) != nil || fields == nil {
		return a, errors.New("notebook input must be an object")
	}
	for _, key := range []string{"notebook_path", "cell_id", "edit_mode"} {
		if fields[key] == nil {
			return a, errors.New("notebook input missing required field")
		}
	}
	for _, value := range fields {
		if bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			return a, errors.New("notebook fields cannot be null")
		}
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.DisallowUnknownFields()
	if err := d.Decode(&a); err != nil {
		return a, err
	}
	if d.Decode(new(any)) != io.EOF {
		return a, errors.New("notebook requires one input object")
	}
	if a.Path == "" || len(a.Path) > 4096 || strings.ContainsRune(a.Path, 0) || !strings.HasSuffix(strings.ToLower(a.Path), ".ipynb") || !validID(a.ID) {
		return a, errors.New("explicit .ipynb path and nbformat cell ID required")
	}
	if a.CellType != "" && a.CellType != "code" && a.CellType != "markdown" {
		return a, errors.New("new cell type must be code/markdown")
	}
	if a.Source != nil && len(*a.Source) > 128<<10 {
		return a, errors.New("new notebook source exceeds 128 KiB")
	}
	switch a.Mode {
	case "insert", "replace":
		if a.Source == nil {
			return a, errors.New("new_source required for insert/replace")
		}
		if a.Mode == "replace" && a.Before != "" {
			return a, errors.New("insert_before is only for insert")
		}
		if a.Before != "" && !validID(a.Before) {
			return a, errors.New("invalid insert_before ID")
		}
	case "delete":
		if a.Source != nil || a.Before != "" || a.CellType != "" {
			return a, errors.New("delete does not consume source/type/position")
		}
	default:
		return a, errors.New("edit_mode insert/replace/delete required")
	}
	return a, nil
}
func transform(raw []byte, a editArguments) (editResult, error) {
	var result editResult
	if len(raw) > 20<<20 || !utf8.Valid(raw) || !wire.ValidJSON(raw) {
		return result, errors.New("UTF-8 notebook <=20 MiB required")
	}
	if err := strictjson.Validate(raw); err != nil {
		return result, err
	}
	var document map[string]json.RawMessage
	if json.Unmarshal(raw, &document) != nil || document == nil {
		return result, errors.New("notebook JSON object required")
	}
	var major, minor int
	if json.Unmarshal(document["nbformat"], &major) != nil || json.Unmarshal(document["nbformat_minor"], &minor) != nil || major != 4 || minor < 5 {
		return result, errors.New("explicit nbformat4.5+ notebook required; no silent format upgrade")
	}
	var metadata map[string]json.RawMessage
	if json.Unmarshal(document["metadata"], &metadata) != nil || metadata == nil {
		return result, errors.New("notebook metadata object required")
	}
	var cells []map[string]json.RawMessage
	if json.Unmarshal(document["cells"], &cells) != nil || cells == nil || len(cells) > 10000 {
		return result, errors.New("notebook cells array <=10000 required")
	}
	index, before := -1, -1
	seen := map[string]bool{}
	for i, cell := range cells {
		var id, kind string
		if json.Unmarshal(cell["id"], &id) != nil || !validID(id) || seen[id] {
			return result, errors.New("notebook cell IDs must be valid and unique")
		}
		seen[id] = true
		if id == a.ID {
			index = i
		}
		if id == a.Before {
			before = i
		}
		if json.Unmarshal(cell["cell_type"], &kind) != nil || kind == "" {
			return result, errors.New("cell_type string required")
		}
		var meta map[string]json.RawMessage
		if json.Unmarshal(cell["metadata"], &meta) != nil || meta == nil {
			return result, errors.New("cell metadata object required")
		}
		if err := validateSource(cell["source"]); err != nil {
			return result, err
		}
	}
	if a.Mode == "insert" {
		if index >= 0 {
			return result, errors.New("insert cell ID already exists")
		}
		if a.Before != "" && before < 0 {
			return result, errors.New("insert_before cell not found")
		}
		index = len(cells)
		if before >= 0 {
			index = before
		}
		cell := map[string]json.RawMessage{"id": encoded(a.ID), "metadata": json.RawMessage(`{}`)}
		kind := a.CellType
		if kind == "" {
			kind = "code"
		}
		cell["cell_type"] = encoded(kind)
		cell["source"] = encoded(*a.Source)
		if kind == "code" {
			cell["outputs"] = json.RawMessage(`[]`)
			cell["execution_count"] = json.RawMessage(`null`)
		}
		cells = append(cells, nil)
		copy(cells[index+1:], cells[index:])
		cells[index] = cell
		result.CellType = kind
	} else {
		if index < 0 {
			return result, errors.New("selected notebook cell not found")
		}
		cell := cells[index]
		var meta map[string]json.RawMessage
		json.Unmarshal(cell["metadata"], &meta)
		var editable, deletable *bool
		if raw := meta["editable"]; raw != nil {
			if json.Unmarshal(raw, &editable) != nil {
				return result, errors.New("invalid editable metadata")
			}
		}
		if raw := meta["deletable"]; raw != nil {
			if json.Unmarshal(raw, &deletable) != nil {
				return result, errors.New("invalid deletable metadata")
			}
		}
		if editable != nil && !*editable || a.Mode == "delete" && deletable != nil && !*deletable {
			return result, errors.New("notebook cell metadata disallows this edit")
		}
		if a.Mode == "delete" {
			cells = append(cells[:index], cells[index+1:]...)
		} else {
			kind := a.CellType
			if kind == "" {
				json.Unmarshal(cell["cell_type"], &kind)
			}
			if kind != "code" && kind != "markdown" {
				return result, errors.New("selected cell type unsupported for source replacement")
			}
			cell["source"] = encoded(*a.Source)
			cell["cell_type"] = encoded(kind)
			result.CellType = kind
			delete(meta, "execution")
			cell["metadata"] = encoded(meta)
			if kind == "code" {
				cell["outputs"] = json.RawMessage(`[]`)
				cell["execution_count"] = json.RawMessage(`null`)
				delete(cell, "attachments")
				result.ClearedOutputs = true
			} else {
				delete(cell, "outputs")
				delete(cell, "execution_count")
			}
		}
	}
	document["cells"] = encoded(cells)
	result.Data = encoded(document)
	if len(result.Data) > 20<<20 {
		return editResult{}, errors.New("edited notebook exceeds 20 MiB")
	}
	result.Index = index
	return result, nil
}

func validateSource(raw json.RawMessage) error {
	value := bytes.TrimSpace(raw)
	if len(value) > 0 && value[0] == '"' {
		var text string
		if json.Unmarshal(value, &text) == nil {
			return nil
		}
	}
	if len(value) > 0 && value[0] == '[' {
		var parts []json.RawMessage
		if json.Unmarshal(value, &parts) == nil && parts != nil {
			for _, part := range parts {
				part = bytes.TrimSpace(part)
				if len(part) == 0 || part[0] != '"' {
					return errors.New("each cell source array item must be a string, not null")
				}
				var text string
				if json.Unmarshal(part, &text) != nil {
					return errors.New("invalid cell source string")
				}
			}
			return nil
		}
	}
	return errors.New("cell source must be an actual string or string array, not null")
}
func encoded(value any) json.RawMessage { raw, _ := json.Marshal(value); return raw }

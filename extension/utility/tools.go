// INPUT: bytes, encoding/json, errors/io/reflect/utf8, strictjson and kernel ABI/wire.
// OUTPUT: Private shared strict argument, identity and bounded response helpers.
// POS: [L4 extension] Helpers carry no service, tool-name dispatch or runtime lifecycle.
package utility

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"reflect"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/system/strictjson"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func missing(value any) bool {
	if value == nil {
		return true
	}
	v := reflect.ValueOf(value)
	switch v.Kind() {
	case reflect.Pointer, reflect.Func, reflect.Map, reflect.Slice, reflect.Interface, reflect.Chan:
		return v.IsNil()
	}
	return false
}

func actual(tool abi.Tool, e abi.ToolExecutionContext, call wire.ToolCall) error {
	if e.ActualTool != tool || call.ID == "" || e.CallID != call.ID || call.Name != tool.Name() {
		return errors.New("utility requires actual dispatched tool/call identity")
	}
	return nil
}
func decode(raw json.RawMessage, out any) error {
	if len(raw) > 4096 || !utf8.Valid(raw) {
		return errors.New("utility input exceeds 4 KiB UTF-8")
	}
	if err := strictjson.ValidateObject(raw, out); err != nil {
		return err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(raw, &fields); err != nil {
		return err
	}
	for _, value := range fields {
		if bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			return errors.New("null utility field")
		}
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.DisallowUnknownFields()
	if err := d.Decode(out); err != nil {
		return err
	}
	if d.Decode(new(any)) != io.EOF {
		return errors.New("one utility input object required")
	}
	return nil
}
func respond(id string, body any, isError bool) (wire.ToolResult, error) {
	data, err := json.Marshal(body)
	if err != nil {
		return wire.ToolResult{}, err
	}
	if len(data) > 16<<10 {
		return wire.ToolResult{}, errors.New("utility result metadata exceeds 16 KiB")
	}
	return wire.ToolResult{CallID: id, IsError: isError, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(data)}}}, nil
}

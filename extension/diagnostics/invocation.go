// INPUT: Bounded diagnostic Tool or direct Manager call arguments and shared strict native JSON validation.
// OUTPUT: No exported symbols.
// POS: [L4 extension] Tool and direct Manager consumers agree on exact action/profile and unambiguous input.
package diagnostics

import (
	"bytes"
	"encoding/json"
	"errors"

	strictjson "github.com/QuincySx/Alva/platform/system/strictjson"
)

type diagnosticArguments struct {
	Action    string `json:"action"`
	ProfileID string `json:"profile_id"`
	After     string `json:"after"`
}

func decodeArguments(raw json.RawMessage) (diagnosticArguments, error) {
	var args diagnosticArguments
	if len(raw) > 4096 {
		return args, errors.New("diagnostics input exceeds 4 KiB")
	}
	if err := strictjson.ValidateObject(raw, &args); err != nil {
		return args, err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(raw, &fields); err != nil {
		return args, err
	}
	for _, field := range fields {
		if bytes.Equal(bytes.TrimSpace(field), []byte("null")) {
			return args, errors.New("diagnostics fields cannot be null")
		}
	}
	if err := json.Unmarshal(raw, &args); err != nil {
		return args, err
	}
	if args.Action != "list" && args.Action != "run" || args.Action == "list" && args.ProfileID != "" || args.Action == "run" && (args.ProfileID == "" || len(args.ProfileID) > 128 || args.After != "") || len(args.After) > 128 {
		return args, errors.New("invalid explicit diagnostics action/profile")
	}
	return args, nil
}

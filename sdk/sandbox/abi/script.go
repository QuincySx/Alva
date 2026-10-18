// INPUT: Explicit UTF-8 script source and bounded isolated-engine result metadata
// OUTPUT: Versioned ScriptRequest/ScriptResult validation and source/output byte ceilings
// POS: [L0 foundation] Capability data only; engine lifetime, files, network and permission remain host-owned
package abi

import "errors"

const MaxScriptBytes = 64 << 10
const MaxScriptResultBytes = 1 << 20

type ScriptRequest struct {
	Version uint32 `json:"version"`
	Script  string `json:"script"`
}

func (r ScriptRequest) Validate() error {
	if r.Version != Version || len(r.Script) == 0 || len(r.Script) > MaxScriptBytes || !Text(r.Script) {
		return errors.New("script requires 1..64KiB UTF-8 source and current ABI version")
	}
	return nil
}

type ScriptResult struct {
	Version uint32   `json:"version"`
	Result  string   `json:"result"`
	Output  []string `json:"output"`
	Error   string   `json:"error,omitempty"`
}

func (r ScriptResult) Validate() error {
	if r.Version != Version || !Text(r.Result) || !Text(r.Error) || len(r.Result) > 32<<10 || len(r.Error) > 32<<10 || len(r.Output) > 256 || r.Error != "" && r.Result != "" {
		return errors.New("invalid bounded script result")
	}
	size := 0
	for _, line := range r.Output {
		if !Text(line) {
			return errors.New("invalid UTF-8 script output")
		}
		size += len(line)
	}
	if size > 64<<10 {
		return errors.New("script output exceeds 64KiB")
	}
	return nil
}

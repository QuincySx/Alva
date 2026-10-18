// INPUT: context, encoding/json, os, runtime, testing, agent/context, kernel/abi, llm/wire, sandbox/abi
// OUTPUT: Native-target fail-closed guest tool regression
// POS: [platform / WASM host or guest] Prevents accidental unsandboxed native fallback when a caller registers WASI-only file tools.
package guest

import (
	"context"
	"encoding/json"
	"os"
	"runtime"
	"testing"

	agentcontext "github.com/QuincySx/Alva/go/sdk/agent/context"
	kernelabi "github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"github.com/QuincySx/Alva/go/sdk/sandbox/abi"
)

func TestGuestFileToolsNeverFallbackToNativeOSExecution(t *testing.T) {
	if runtime.GOOS == "wasip1" {
		t.Skip("native-only rejection regression")
	}
	dir := t.TempDir()
	refs, err := agentcontext.NewMemoryReferences("guest", 1024)
	if err != nil {
		t.Fatal(err)
	}
	config := abi.Context{SessionID: "guest", Workspace: dir, Policy: abi.Policy{AllowFileWrite: true}}
	for _, tool := range FileTools(config, refs) {
		if tool.Name() != "write_file" {
			continue
		}
		_, err := tool.Execute(context.Background(), kernelabi.ToolExecutionContext{SessionID: "guest"}, wire.ToolCall{ID: "call", Name: "write_file", Arguments: json.RawMessage(`{"path":"must-not-exist","content":"unsafe fallback"}`)})
		if err == nil {
			t.Fatal("native fallback accepted")
		}
	}
	entries, err := os.ReadDir(dir)
	if err != nil || len(entries) != 0 {
		t.Fatal("native filesystem changed", entries, err)
	}
}

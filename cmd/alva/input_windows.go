// INPUT: Explicit sole-reader Windows stdin file/pipe and caller cancellation
// OUTPUT: Native direct REPL/approval file lines; explicit unsupported console and pinned-shell trampoline
// POS: [L6 application] Windows pipeline input is real; no uncancellable cooked-console read or pathname substitute for pinned shell authority.
//go:build windows

package main

import (
	"context"
	"errors"
	"os"

	sandboxhost "github.com/QuincySx/Alva/platform/sandbox/host"
	"github.com/QuincySx/Alva/platform/system/lineinput"
	sandboxabi "github.com/QuincySx/Alva/sdk/sandbox/abi"
)

func readREPLFile(ctx context.Context, file *os.File) (string, error) {
	return lineinput.ReadLine(ctx, file, 32<<10)
}
func readApprovalFile(ctx context.Context, file *os.File) (string, error) {
	return lineinput.ReadLine(ctx, file, 256)
}
func openApprovalTTY() (*os.File, error) {
	return nil, errors.New("Windows interactive console approval is unsupported; explicit file/pipe input is available")
}
func executeWASMShell(context.Context, sandboxhost.EscalationTarget) (sandboxabi.EscalationResponse, error) {
	return sandboxabi.EscalationResponse{}, errors.New("Windows pinned-directory host shell escalation is unsupported")
}
func runWASMShellTrampoline(args []string) (bool, error) {
	if len(args) > 0 && args[0] == "__alva_private_wasm_shell_v1" {
		return true, errors.New("Unix shell trampoline unsupported on Windows")
	}
	return false, nil
}

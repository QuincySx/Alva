// INPUT: Explicit unsupported native target
// OUTPUT: Fail-closed shell and interactive approval operations
// POS: [L6 application] No unsafe host process or uncancellable terminal fallback on unverified platforms.
//go:build !darwin && !linux && !windows

package main

import (
	"context"
	"errors"
	sandboxhost "github.com/QuincySx/Alva/go/platform/sandbox/host"
	sandboxabi "github.com/QuincySx/Alva/go/sdk/sandbox/abi"
	"os"
)

func executeWASMShell(context.Context, sandboxhost.EscalationTarget) (sandboxabi.EscalationResponse, error) {
	return sandboxabi.EscalationResponse{}, errors.New("host shell escalation is unsupported on this platform")
}
func openApprovalTTY() (*os.File, error) {
	return nil, errors.New("TTY approval is unsupported on this platform")
}
func readApprovalFile(context.Context, *os.File) (string, error) {
	return "", errors.New("file approval is unsupported on this platform")
}
func runWASMShellTrampoline(args []string) (bool, error) {
	if len(args) > 0 && args[0] == "__alva_private_wasm_shell_v1" {
		return true, errors.New("host shell is unsupported on this platform")
	}
	return false, nil
}

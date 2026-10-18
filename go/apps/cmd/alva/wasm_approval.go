// INPUT: Explicit approval channels, crypto/rand, context, sandbox/host
// OUTPUT: Per-invocation challenge/response approval with bounded cancellable input
// POS: [L6 application] CLI HITL policy; opting into shell never approves a particular command.
package main

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"

	sandboxhost "github.com/QuincySx/Alva/go/platform/sandbox/host"
)

type wasmApproval struct {
	readLine func(context.Context) (string, error)
	output   io.Writer
}

func (a wasmApproval) Approve(ctx context.Context, target sandboxhost.EscalationTarget) (bool, error) {
	if err := ctx.Err(); err != nil {
		return false, err
	}
	var data [8]byte
	if _, err := rand.Read(data[:]); err != nil {
		return false, err
	}
	challenge := "approve " + hex.EncodeToString(data[:])
	text := fmt.Sprintf("UNSANDBOXED host shell request job=%q guest_cwd=%q host_cwd_display=%q command=%q\nType exactly %q to approve THIS invocation; any other line denies: ", target.JobID, target.Request.Cwd, target.HostPath, target.Request.Command, challenge)
	if err := writeString(a.output, text); err != nil {
		return false, err
	}
	line, err := a.readLine(ctx)
	if errors.Is(err, io.EOF) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	if err := ctx.Err(); err != nil {
		return false, err
	}
	return strings.TrimSuffix(strings.TrimSuffix(line, "\n"), "\r") == challenge, nil
}
func openWASMApproval(o sandboxOptions, input io.Reader, output io.Writer) (sandboxhost.Approval, func() error, error) {
	closeFn := func() error { return nil }
	if o.approvalTTY {
		file, err := openApprovalTTY()
		if err != nil {
			return nil, nil, err
		}
		input = file
		output = file
		closeFn = file.Close
	}
	var read func(context.Context) (string, error)
	switch value := input.(type) {
	case *os.File:
		read = func(ctx context.Context) (string, error) { return readApprovalFile(ctx, value) }
	case *strings.Reader, *bytes.Reader, *bytes.Buffer:
		read = func(ctx context.Context) (string, error) { return finiteApprovalLine(ctx, value) }
	default:
		closeFn()
		return nil, nil, errors.New("approval channel must be an explicit file/terminal or finite in-memory reader")
	}
	return wasmApproval{readLine: read, output: output}, closeFn, nil
}
func finiteApprovalLine(ctx context.Context, reader io.Reader) (string, error) {
	return finiteInputLine(ctx, reader, 256)
}
func finiteInputLine(ctx context.Context, reader io.Reader, maximum int) (string, error) {
	var data []byte
	var one [1]byte
	for len(data) <= maximum {
		if err := ctx.Err(); err != nil {
			return "", err
		}
		n, err := reader.Read(one[:])
		if n > 0 {
			data = append(data, one[0])
			if one[0] == '\n' {
				return string(data), nil
			}
		}
		if err != nil {
			if err == io.EOF && len(data) > 0 {
				return string(data), nil
			}
			return "", err
		}
		if n == 0 {
			return "", io.ErrNoProgress
		}
	}
	return "", fmt.Errorf("input line exceeds %d bytes", maximum)
}

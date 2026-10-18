// INPUT: context, errors, io, os, strings, testing, time, native/process.
// OUTPUT: Real full-duplex tail capture, exit, EOF-not-exit, bounds and explicit environment regressions.
// POS: [platform / native execution host] Temporary Go test subprocess acceptance without shell interpretation, accounts or detached work.
package command

import (
	"context"
	"errors"
	"io"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/platform/system/process"
)

func TestCommandHelper(t *testing.T) {
	mode := ""
	for _, arg := range os.Args {
		if strings.HasPrefix(arg, "--capture-helper=") {
			mode = strings.TrimPrefix(arg, "--capture-helper=")
		}
	}
	if mode == "" {
		return
	}
	if os.Getenv("ALVA_HOST_SECRET") != "" {
		os.Exit(90)
	}
	switch mode {
	case "duplex":
		io.WriteString(os.Stdout, strings.Repeat("O", 200000))
		io.WriteString(os.Stderr, strings.Repeat("E", 100000))
		data, err := io.ReadAll(os.Stdin)
		if err != nil {
			os.Exit(91)
		}
		os.Stdout.Write(data)
		io.WriteString(os.Stdout, "END🙂")
		os.Exit(7)
	case "flood":
		io.WriteString(os.Stdout, strings.Repeat("X", 1<<20))
		os.Exit(0)
	case "closed-streams":
		os.Stdout.Close()
		os.Stderr.Close()
		for {
			time.Sleep(time.Hour)
		}
	default:
		os.Exit(92)
	}
}
func helperConfig(t *testing.T, mode string) process.Config {
	t.Helper()
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	return process.Config{Command: exe, Args: []string{"-test.run=^TestCommandHelper$", "--", "--capture-helper=" + mode}, Cwd: t.TempDir()}
}
func TestCaptureDrainsFastExitTailAndFullDuplexWithoutAmbientEnvironment(t *testing.T) {
	t.Setenv("ALVA_HOST_SECRET", "must-not-inherit")
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	input := strings.Repeat("I", 128<<10)
	result, err := Run(ctx, helperConfig(t, "duplex"), []byte(input), 1<<20)
	if err != nil || !result.Complete || !result.Reaped || result.ExitCode != 7 || result.InputBytes != int64(len(input)) {
		t.Fatalf("natural command result: %+v %v", struct {
			Code             int
			Complete, Reaped bool
			Input            int64
		}{result.ExitCode, result.Complete, result.Reaped, result.InputBytes}, err)
	}
	if string(result.Stdout) != strings.Repeat("O", 200000)+input+"END🙂" || string(result.Stderr) != strings.Repeat("E", 100000) {
		t.Fatal("fast exit truncated captured output")
	}
}
func TestOutputOverflowIsExplicitIncompleteAndActuallyReaped(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	result, err := Run(ctx, helperConfig(t, "flood"), nil, 1024)
	if !errors.Is(err, ErrOutputLimit) || !result.OutputLimit || result.Complete || !result.Reaped || len(result.Stdout) != 1024 {
		t.Fatal("overflow looked successful", result, err)
	}
}
func TestEOFDoesNotFakeProcessExitAndCancellationReaps(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 150*time.Millisecond)
	defer cancel()
	result, err := Run(ctx, helperConfig(t, "closed-streams"), nil, 1024)
	if !errors.Is(err, context.DeadlineExceeded) || !result.Started || !result.Reaped || result.Complete {
		t.Fatal("EOF falsely completed live process", result, err)
	}
}

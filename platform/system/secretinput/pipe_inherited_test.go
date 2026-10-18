// INPUT: Real test-binary fd0 inheritance, a still-open parent producer and bounded child contexts
// OUTPUT: Inherited stdin EOF, cancellation and overflow regressions beyond in-process os.Pipe behavior
// POS: [platform / OS primitive] Exercise the actual native command descriptor boundary without credentials or a GUI.
//go:build darwin || linux || windows

package secretinput

import (
	"bytes"
	"context"
	"errors"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"
)

func TestInheritedStdinPipeHelper(t *testing.T) {
	if len(os.Args) < 3 || os.Args[len(os.Args)-2] != "alva-secret-pipe-helper" {
		return
	}
	ctx, cancel := context.WithTimeout(t.Context(), 200*time.Millisecond)
	defer cancel()
	data, err := Read(ctx, os.Stdin, 8)
	switch os.Args[len(os.Args)-1] {
	case "cancel":
		if !errors.Is(err, context.DeadlineExceeded) || data != nil {
			t.Fatalf("inherited open fd0 did not cancel: %v", err)
		}
	case "eof":
		if err != nil || string(data) != "secret\n" {
			t.Fatalf("inherited fd0 could not read exact bounded input: %v", err)
		}
	case "overflow":
		if err == nil || !strings.Contains(err.Error(), "exceeds byte ceiling") || data != nil {
			t.Fatalf("inherited oversized fd0 waited for EOF or exposed bytes: %v", err)
		}
	default:
		t.Fatal("invalid fixture mode")
	}
}

func TestActualInheritedStdinPipe(t *testing.T) {
	for _, mode := range []string{"cancel", "eof", "overflow"} {
		t.Run(mode, func(t *testing.T) {
			reader, writer, err := os.Pipe()
			if err != nil {
				t.Fatal(err)
			}
			defer reader.Close()
			defer writer.Close()
			ctx, cancel := context.WithTimeout(t.Context(), 3*time.Second)
			defer cancel()
			executable, err := os.Executable()
			if err != nil {
				t.Fatal(err)
			}
			cmd := exec.CommandContext(ctx, executable, "-test.run=^TestInheritedStdinPipeHelper$", "--", "alva-secret-pipe-helper", mode)
			cmd.Stdin = reader
			var output bytes.Buffer
			cmd.Stdout, cmd.Stderr = &output, &output
			if err := cmd.Start(); err != nil {
				t.Fatal(err)
			}
			reader.Close()
			switch mode {
			case "eof":
				_, err = writer.Write([]byte("secret\n"))
				writer.Close()
			case "overflow":
				_, err = writer.Write([]byte("123456789"))
			case "cancel":
				_, err = writer.Write([]byte("partial"))
			}
			if err != nil {
				_ = cmd.Process.Kill()
				_ = cmd.Wait()
				t.Fatal(err)
			}
			if err := cmd.Wait(); err != nil {
				t.Fatalf("actual inherited fd0 %s: %v\n%s", mode, err, output.String())
			}
		})
	}
}

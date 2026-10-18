// INPUT: Real inherited Windows fd0 pipe, bounded helper modes and parent-owned stdio
// OUTPUT: No-readahead lines, no-EOF cancellation/overflow, resumed borrowed pipe and EOF partial acceptance
// POS: [platform / OS primitive] Tests actual inherited synchronous handles, not only in-process pollable os.Pipe.
//go:build windows

package lineinput

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"testing"
	"time"

	"golang.org/x/sys/windows"
)

func TestWindowsLineHelper(t *testing.T) {
	if len(os.Args) < 3 || os.Args[len(os.Args)-2] != "alva-line-helper" {
		return
	}
	mode := os.Args[len(os.Args)-1]
	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancel()
	switch mode {
	case "cancel":
		if _, err := ReadLine(ctx, os.Stdin, 16); !errors.Is(err, context.DeadlineExceeded) {
			os.Exit(91)
		}
		if _, err := os.Stdin.Stat(); err != nil {
			os.Exit(92)
		}
		fmt.Println("canceled")
		line, err := ReadLine(context.Background(), os.Stdin, 16)
		if err != nil || line != "after\n" {
			os.Exit(93)
		}
	case "lines":
		first, err := ReadLine(context.Background(), os.Stdin, 16)
		if err != nil || first != "first\n" {
			os.Exit(94)
		}
		second, err := ReadLine(context.Background(), os.Stdin, 16)
		if err != nil || second != "中文\r\n" {
			os.Exit(95)
		}
		if _, err := ReadLine(ctx, os.Stdin, 16); !errors.Is(err, context.DeadlineExceeded) {
			os.Exit(96)
		}
	case "overflow":
		if _, err := ReadLine(context.Background(), os.Stdin, 4); err == nil {
			os.Exit(97)
		}
	case "boundary":
		line, err := ReadLine(context.Background(), os.Stdin, 4)
		if err != nil || line != "1234\n" {
			os.Exit(101)
		}
	case "eof":
		line, err := ReadLine(context.Background(), os.Stdin, 16)
		if err != nil || line != "partial" {
			os.Exit(98)
		}
		if _, err := ReadLine(context.Background(), os.Stdin, 16); !errors.Is(err, io.EOF) {
			os.Exit(99)
		}
	default:
		os.Exit(100)
	}
	fmt.Println("ok")
	os.Exit(0)
}

func TestActualWindowsInheritedLineInput(t *testing.T) {
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	for _, mode := range []string{"cancel", "lines", "boundary", "overflow", "eof"} {
		t.Run(mode, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
			defer cancel()
			cmd := exec.CommandContext(ctx, binary, "-test.run=^TestWindowsLineHelper$", "--", "alva-line-helper", mode)
			root, err := windows.GetWindowsDirectory()
			if err != nil {
				t.Fatal(err)
			}
			cmd.Env = []string{"SYSTEMROOT=" + root}
			stdin, err := cmd.StdinPipe()
			if err != nil {
				t.Fatal(err)
			}
			defer stdin.Close()
			stdout, err := cmd.StdoutPipe()
			if err != nil {
				t.Fatal(err)
			}
			if err := cmd.Start(); err != nil {
				t.Fatal(err)
			}
			reader := bufio.NewReader(stdout)
			payload := map[string]string{"lines": "first\n中文\r\n", "boundary": "1234\n", "overflow": "12345", "eof": "partial"}[mode]
			if payload != "" {
				if _, err := io.WriteString(stdin, payload); err != nil {
					t.Fatal(err)
				}
			}
			if mode == "eof" {
				stdin.Close()
			}
			if mode == "cancel" {
				line, err := reader.ReadString('\n')
				if err != nil || line != "canceled\n" {
					t.Fatal("actual inherited read did not cancel before pipe EOF", line, err)
				}
				if _, err := io.WriteString(stdin, "after\n"); err != nil {
					t.Fatal(err)
				}
			}
			line, readErr := reader.ReadString('\n')
			waitErr := cmd.Wait()
			if readErr != nil || waitErr != nil || line != "ok\n" {
				t.Fatal("actual Windows input failed", mode, line, readErr, waitErr)
			}
		})
	}
}

// INPUT: Real owned Darwin helper processes, WNOWAIT/group metadata and deterministic EPERM observation seams
// OUTPUT: Exit-transition recovery, live-group/inspection denial and partial-prefix natural-exit regressions
// POS: [platform / OS primitive] Permission failures clear only after actual no-live-group proof, before the unique owner reap.
//go:build darwin

package process

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"testing"
	"time"

	"golang.org/x/sys/unix"
)

func TestDarwinOwnedExitFixture(t *testing.T) {
	switch os.Getenv("ALVA_DARWIN_EXIT_FIXTURE") {
	case "partial":
		_, _ = fmt.Fprint(os.Stdout, `{"result":`)
		os.Exit(0)
	case "gated":
		_, _ = fmt.Fprintln(os.Stdout, "ready")
		var signal [1]byte
		if _, err := io.ReadFull(os.Stdin, signal[:]); err != nil {
			os.Exit(2)
		}
		os.Exit(0)
	case "live":
		_, _ = fmt.Fprintln(os.Stdout, "ready")
		_, _ = io.Copy(io.Discard, os.Stdin)
		os.Exit(0)
	case "group":
		// Inherit this fixture's owned group, but not its stdio pipes. The
		// direct leader exits naturally while an actual group member lives.
		child := exec.Command(os.Args[0], "-test.run=^TestDarwinOwnedExitFixture$")
		child.Env = []string{"PATH=" + SafePath, "LANG=en_US.UTF-8", "GORACE=atexit_sleep_ms=0", "ALVA_DARWIN_EXIT_FIXTURE=sleep"}
		if err := child.Start(); err != nil {
			os.Exit(3)
		}
		_, _ = fmt.Fprintln(os.Stdout, child.Process.Pid)
		os.Exit(0)
	case "sleep":
		time.Sleep(30 * time.Second)
		os.Exit(0)
	}
}

func darwinFixture(t *testing.T, mode string) *Process {
	t.Helper()
	child, err := Start(Config{Command: os.Args[0], Args: []string{"-test.run=^TestDarwinOwnedExitFixture$"}, Cwd: t.TempDir(), Env: map[string]string{"ALVA_DARWIN_EXIT_FIXTURE": mode, "GORACE": "atexit_sleep_ms=0"}})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		defer cancel()
		if err := child.Close(ctx); err != nil {
			t.Error("actual fixture cleanup:", err)
		}
	})
	return child
}

func TestDarwinEPERMReobservesRealExitWithoutSignalOrReap(t *testing.T) {
	child := darwinFixture(t, "gated")
	var ready [6]byte
	if _, err := io.ReadFull(child.Stdout, ready[:]); err != nil || string(ready[:]) != "ready\n" {
		t.Fatal("gated fixture did not actually enter", err)
	}
	pid := child.cmd.Process.Pid
	probes := 0
	err := observeDarwinEPERM(syscall.EPERM, 100*time.Millisecond, func() (bool, error) {
		probes++
		if probes == 1 {
			ended, err := exited(pid)
			if err != nil || ended {
				t.Fatal("gated fixture was not actually live", ended, err)
			}
			if _, err = child.Stdin.Write([]byte{1}); err != nil {
				t.Fatal(err)
			}
			return false, nil // The original one-shot branch permanently failed here.
		}
		return darwinOwnedGroupExited(pid)
	})
	if err != nil || probes < 2 {
		t.Fatal("transient non-waitable exit was treated as permanent denial", probes, err)
	}
	if ended, err := exited(pid); err != nil || !ended || child.cmd.ProcessState != nil || child.CleanupComplete() {
		t.Fatal("observation reaped or invented owner completion", ended, err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 3*time.Second)
	defer cancel()
	if err = child.Close(ctx); err != nil || !child.CleanupComplete() {
		t.Fatal("unique real reap did not finish", err)
	}
	if code, ok := child.ExitCode(); !ok || code != 0 {
		t.Fatal("reobservation signaled a naturally exiting child", code, ok)
	}
}

func TestDarwinEPERMLiveLeaderAndInspectionFailureRemainErrors(t *testing.T) {
	child := darwinFixture(t, "live")
	var ready [6]byte
	if _, err := io.ReadFull(child.Stdout, ready[:]); err != nil || string(ready[:]) != "ready\n" {
		t.Fatal("actual live child not ready", err)
	}
	probes := 0
	err := observeDarwinEPERM(syscall.EPERM, 5*time.Millisecond, func() (bool, error) {
		probes++
		return darwinOwnedGroupExited(child.cmd.Process.Pid)
	})
	if !errors.Is(err, syscall.EPERM) || probes < 2 || child.CleanupComplete() {
		t.Fatal("persistent live-group denial was cleared", probes, err)
	}
	if ended, err := exited(child.cmd.Process.Pid); err != nil || ended {
		t.Fatal("observation killed or lost live leader", ended, err)
	}
	inspection := errors.New("fixture inspection unavailable")
	err = observeDarwinEPERM(syscall.EPERM, time.Second, func() (bool, error) { return false, inspection })
	if !errors.Is(err, syscall.EPERM) || !errors.Is(err, inspection) {
		t.Fatal("inspection failure or original permission evidence lost", err)
	}
}

func TestDarwinEPERMExitedLeaderDoesNotHideActualLiveGroupMember(t *testing.T) {
	child := darwinFixture(t, "group")
	data, err := io.ReadAll(child.Stdout)
	if err != nil {
		t.Fatal(err)
	}
	descendant, err := strconv.Atoi(strings.TrimSpace(string(data)))
	if err != nil || descendant <= 0 {
		t.Fatal("actual descendant identity missing", string(data), err)
	}
	deadline := time.Now().Add(3 * time.Second)
	for {
		ended, err := exited(child.cmd.Process.Pid)
		if err != nil {
			t.Fatal(err)
		}
		if ended {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("actual group leader did not exit")
		}
		time.Sleep(time.Millisecond)
	}
	err = observeDarwinEPERM(syscall.EPERM, 5*time.Millisecond, func() (bool, error) { return darwinOwnedGroupExited(child.cmd.Process.Pid) })
	if !errors.Is(err, syscall.EPERM) {
		t.Fatal("leader-only exit hid live descendant", err)
	}
	members, err := unix.SysctlKinfoProcSlice("kern.proc.pgrp", child.cmd.Process.Pid)
	live := false
	for _, member := range members {
		if int(member.Proc.P_pid) == descendant && member.Proc.P_stat != 5 {
			live = true
		}
	}
	if err != nil || !live {
		t.Fatal("fixture did not exercise a real live group member", err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 3*time.Second)
	defer cancel()
	if err = child.Close(ctx); err != nil || !child.CleanupComplete() {
		t.Fatal("actual group kill and leader reap failed", err)
	}
}

func TestDarwinPartialPrefixNaturalExitCloseDoesNotCacheTransientEPERM(t *testing.T) {
	var workers sync.WaitGroup
	for range 4 {
		workers.Go(func() {
			for range 32 {
				child := darwinFixture(t, "partial")
				ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
				stop := context.AfterFunc(ctx, child.BeginClose)
				data, readErr := io.ReadAll(child.Stdout)
				closeErr := child.Close(ctx)
				stop()
				cancel()
				code, exited := child.ExitCode()
				if readErr != nil || string(data) != `{"result":` || closeErr != nil || !child.CleanupComplete() || !exited || code != 0 {
					t.Errorf("natural partial-prefix exit: read=%v body=%q close=%v code=%d/%v clean=%v", readErr, data, closeErr, code, exited, child.CleanupComplete())
					return
				}
			}
		})
	}
	workers.Wait()
}

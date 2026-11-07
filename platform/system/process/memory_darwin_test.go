// INPUT: A disposable owned cat process and the real Darwin libproc usage ABI.
// OUTPUT: Actual owned-PID memory observation and rejection after physical cleanup.
// POS: [platform / OS primitive acceptance] Never probes an unrelated process or assumes a recycled PID remains owned.
//go:build darwin

package process

import (
	"context"
	"errors"
	"os"
	"testing"
	"time"
)

func TestMemoryBytesOnlyWhileActualChildIsOwned(t *testing.T) {
	child, err := Start(Config{Command: "/bin/cat", Cwd: t.TempDir()})
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	defer child.Close(ctx)
	deadline := time.Now().Add(time.Second)
	var used uint64
	for used == 0 && time.Now().Before(deadline) {
		used, err = child.MemoryBytes()
		if err != nil {
			t.Fatal(err)
		}
		time.Sleep(time.Millisecond)
	}
	if used == 0 {
		t.Fatal("actual owned process memory was not observed")
	}
	if err = child.Close(ctx); err != nil || !child.CleanupComplete() {
		t.Fatal("owned process was not reaped", err)
	}
	if _, err = child.MemoryBytes(); !errors.Is(err, os.ErrClosed) {
		t.Fatal("memory probe accepted a no-longer-owned PID", err)
	}
}

// INPUT: Private evidence metadata byte accounting and explicitly held operation admission
// OUTPUT: Retention cap without deletion and owner-cancelled serial admission regressions
// POS: [L7 Hub product] Metadata-only accounting tests do not pretend to generate hundreds of MiB of actual file evidence
package worktree

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/apps/hub"
)

func TestEvidenceBudgetRefusesRetainedBytesWithoutDeletingOriginals(t *testing.T) {
	m, _ := managerFixture(t)
	for i := 0; i < 4; i++ {
		id, err := hub.NewID("diff-")
		if err != nil {
			t.Fatal(err)
		}
		directory := filepath.Join(m.config.Root, "evidence", id)
		if err = os.Mkdir(directory, 0700); err != nil {
			t.Fatal(err)
		}
		data, _ := json.Marshal(map[string]int{"snapshot_bytes": 64 << 20, "patch_bytes": 1 << 20})
		if err = os.WriteFile(filepath.Join(directory, "index.json"), data, 0600); err != nil {
			t.Fatal(err)
		}
	}
	if err := m.evidenceBudget(1); err == nil {
		t.Fatal("over-budget retention accepted")
	}
	files, err := os.ReadDir(filepath.Join(m.config.Root, "evidence"))
	if err != nil || len(files) != 4 {
		t.Fatal("budget check deleted recovery evidence", err)
	}
}
func TestSerialOperationWaiterCancelsWithManagerOwnerAndCloseRetainsActiveOwner(t *testing.T) {
	m, _ := managerFixture(t)
	owned, finish, err := m.operation(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	queued := make(chan error, 1)
	go func() {
		_, end, err := m.operation(context.Background())
		if end != nil {
			end()
		}
		queued <- err
	}()
	short, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if err = m.Close(short); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("Close abandoned active owner", err)
	}
	if !errors.Is(owned.Err(), context.Canceled) {
		t.Fatal("owner did not cancel active operation")
	}
	select {
	case err = <-queued:
		if !errors.Is(err, context.Canceled) && err == nil {
			t.Fatal("queued operation started after close", err)
		}
	case <-time.After(time.Second):
		t.Fatal("queued operation ignored owner cancellation")
	}
	finish()
	if err = m.Close(t.Context()); err != nil || !m.CleanupComplete() {
		t.Fatal("retry did not join real active admission", err)
	}
}

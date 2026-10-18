// INPUT: testing, kernel/abi
// OUTPUT: Scheduling conflict/wave boundary regressions
// POS: [L2 kernel runtime] Pins explicit concurrency opt-in, global barriers and workspace-normalized reader/writer conflicts.
package core

import (
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"testing"
)

func TestConflictAwareWaveComposition(t *testing.T) {
	read := abi.Scheduling{ConcurrentSafe: true, Resources: []abi.ResourceClaim{{Resource: "./a/../file", Access: abi.AccessRead}}}
	write := abi.Scheduling{ConcurrentSafe: true, Resources: []abi.ResourceClaim{{Resource: "/work/file", Access: abi.AccessWrite}}}
	barrier := abi.Scheduling{ConcurrentSafe: true, ExecutionMode: abi.SerialGlobal}
	for _, fixture := range []struct {
		name        string
		scheduling  []abi.Scheduling
		limit, want int
	}{{"readers", []abi.Scheduling{read, read}, 8, 2}, {"writer conflict", []abi.Scheduling{read, write}, 8, 1}, {"global", []abi.Scheduling{read, barrier}, 8, 1}, {"undeclared", []abi.Scheduling{{}, read}, 8, 1}, {"cap", []abi.Scheduling{read, read}, 1, 1}, {"coordinator", []abi.Scheduling{{ConcurrentSafe: true, ExecutionMode: abi.Coordinator}, read}, 8, 2}} {
		t.Run(fixture.name, func(t *testing.T) {
			records := make([]abi.SessionRecord, len(fixture.scheduling))
			for i, schedule := range fixture.scheduling {
				records[i].ToolIntent = &abi.ToolIntent{Scheduling: schedule}
			}
			if got := waveLength(records, fixture.limit, "/work"); got != fixture.want {
				t.Fatalf("wave=%d want=%d", got, fixture.want)
			}
		})
	}
}

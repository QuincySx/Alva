// INPUT: context, errors, testing, time, kernel/abi
// OUTPUT: Cross-Agent tool lock concurrency and cancellation regressions
// POS: [L2.5 SDK assembly] Verifies actual atomic claims and truthful coordinator/global barrier behavior.
package core_test

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"testing"
	"time"
)

func TestToolLocksReadSharingWriterCancellationAndCoordinator(t *testing.T) {
	registry, _ := core.NewToolLockRegistry(10)
	claim := func(access abi.Access) abi.Scheduling {
		return abi.Scheduling{ConcurrentSafe: true, Resources: []abi.ResourceClaim{{Resource: "./file", Access: access}}}
	}
	first, err := registry.Acquire(context.Background(), claim(abi.AccessRead), "/workspace")
	if err != nil {
		t.Fatal(err)
	}
	defer first()
	second, err := registry.Acquire(context.Background(), claim(abi.AccessRead), "/workspace")
	if err != nil {
		t.Fatal(err)
	}
	defer second()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Millisecond)
	defer cancel()
	if _, err := registry.Acquire(ctx, claim(abi.AccessWrite), "/workspace"); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal(err)
	}
	coordinator, err := registry.Acquire(context.Background(), abi.Scheduling{ExecutionMode: abi.Coordinator}, "/workspace")
	if err != nil {
		t.Fatal(err)
	}
	coordinator()
	first()
	second()
	writer, err := registry.Acquire(context.Background(), claim(abi.AccessWrite), "/workspace")
	if err != nil {
		t.Fatal(err)
	}
	writer()
	writer()
}
func TestGlobalToolLockIsBarrierAndWaiterBudgetReleases(t *testing.T) {
	registry, _ := core.NewToolLockRegistry(1)
	release, err := registry.Acquire(context.Background(), abi.Scheduling{ExecutionMode: abi.SerialGlobal}, "")
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Millisecond)
	defer cancel()
	if _, err := registry.Acquire(ctx, abi.Scheduling{}, ""); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal(err)
	}
	release()
	free, err := registry.Acquire(context.Background(), abi.Scheduling{}, "")
	if err != nil {
		t.Fatal("canceled waiter leaked", err)
	}
	free()
}

func TestUndeclaredToolCannotOverlapOtherAgentResourceWrite(t *testing.T) {
	registry, _ := core.NewToolLockRegistry(4)
	release, err := registry.Acquire(context.Background(), abi.Scheduling{ConcurrentSafe: true, Resources: []abi.ResourceClaim{{Resource: "/workspace", Access: abi.AccessWrite}}}, "")
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Millisecond)
	defer cancel()
	if _, err := registry.Acquire(ctx, abi.Scheduling{}, ""); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("undeclared tool escaped cross-Agent barrier", err)
	}
	coordinator, err := registry.Acquire(context.Background(), abi.Scheduling{ExecutionMode: abi.Coordinator}, "")
	if err != nil {
		t.Fatal(err)
	}
	coordinator()
	release()
	unknown, err := registry.Acquire(context.Background(), abi.Scheduling{}, "")
	if err != nil {
		t.Fatal(err)
	}
	ctx2, cancel2 := context.WithTimeout(context.Background(), 10*time.Millisecond)
	defer cancel2()
	if _, err := registry.Acquire(ctx2, abi.Scheduling{ConcurrentSafe: true}, ""); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("active undeclared tool failed to act as barrier", err)
	}
	unknown()
}

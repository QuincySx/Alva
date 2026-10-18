// INPUT: context, encoding/json, errors, testing, time
// OUTPUT: Spawn communication typed-nil rejection, callback safety and reentrant identity regressions
// POS: [L2.3 agent contracts] Rejects invalid executable capabilities before use and keeps communication metadata outside registry locks.
package abi

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"
)

type communicationFixture struct{ kind func() string }

func (c *communicationFixture) Kind() string                { return c.kind() }
func (*communicationFixture) Description() string           { return "communication fixture" }
func (*communicationFixture) ConfigSchema() json.RawMessage { return json.RawMessage(`{}`) }
func (*communicationFixture) Attach(context.Context, SpawnCommContext, json.RawMessage) (SpawnCommHandle, error) {
	return SpawnCommHandle{}, nil
}

type nilSafeCommunication struct{}

func (*nilSafeCommunication) Kind() string                  { return "nil-safe" }
func (*nilSafeCommunication) Description() string           { return "nil-safe metadata" }
func (*nilSafeCommunication) ConfigSchema() json.RawMessage { return json.RawMessage(`{}`) }
func (c *nilSafeCommunication) Attach(context.Context, SpawnCommContext, json.RawMessage) (SpawnCommHandle, error) {
	if c == nil {
		panic("nil communication attached")
	}
	return SpawnCommHandle{}, nil
}

func TestCommunicationRegistryRejectsNilImplementationsBeforeMetadata(t *testing.T) {
	var missing *communicationFixture
	var nilSafe *nilSafeCommunication
	for _, tc := range []struct {
		name  string
		value SpawnCommunication
	}{
		{"nil", nil}, {"typed nil", missing}, {"nil-safe typed nil", nilSafe},
	} {
		t.Run(tc.name, func(t *testing.T) {
			registry, err := NewCommunicationRegistry(1)
			if err != nil {
				t.Fatal(err)
			}
			defer func() {
				if value := recover(); value != nil {
					t.Errorf("invalid communication panicked before rejection: %v", value)
				}
			}()
			if err := registry.Register(tc.value); err == nil {
				t.Fatal("invalid communication accepted")
			}
			if len(registry.List()) != 0 {
				t.Fatal("invalid communication consumed registry capacity")
			}
		})
	}
}

func TestChildCompleteFuncRejectsNilAndPreservesCallbackResult(t *testing.T) {
	t.Run("nil", func(t *testing.T) {
		defer func() {
			if value := recover(); value != nil {
				t.Errorf("nil completion callback panicked: %v", value)
			}
		}()
		var callback ChildCompleteFunc
		if err := callback.Call(context.Background(), SpawnResult{}); err == nil {
			t.Fatal("nil completion callback accepted")
		}
	})
	t.Run("forward", func(t *testing.T) {
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		result := SpawnResult{Text: "selected output", IsError: true, Error: "child failure"}
		failure := errors.New("completion fixture failed")
		called := 0
		callback := ChildCompleteFunc(func(actual context.Context, value SpawnResult) error {
			called++
			if actual != ctx || value != result {
				t.Error("completion callback input changed")
			}
			return failure
		})
		if err := callback.Call(ctx, result); !errors.Is(err, failure) || called != 1 {
			t.Fatal(err, called)
		}
	})
}

func TestCommunicationRegistryReadsIdentityOutsideLock(t *testing.T) {
	registry, err := NewCommunicationRegistry(1)
	if err != nil {
		t.Fatal(err)
	}
	calls := 0
	communication := &communicationFixture{kind: func() string {
		calls++
		_, _ = registry.Get("existing")
		return "reentrant"
	}}
	done := make(chan error, 1)
	go func() { done <- registry.Register(communication) }()
	select {
	case err := <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(time.Second):
		t.Fatal("communication metadata called while registry write lock held")
	}
	if calls != 1 {
		t.Fatal("communication identity was not captured once", calls)
	}
	actual, ok := registry.Get("reentrant")
	if !ok || actual != communication {
		t.Fatal("captured identity did not bind communication")
	}
}

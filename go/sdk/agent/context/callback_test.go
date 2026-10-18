// INPUT: context, errors, reflect, strings, testing, time, kernel/abi, llm/wire
// OUTPUT: Reentrant counter and late compression callback settlement regressions
// POS: [L3 capability library] Keeps pluggable callbacks outside store locks and rejects invalid or cancelled working-view replacements.
package agentcontext

import (
	"context"
	"errors"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type observingCounter struct{ observe func() }

func (counter *observingCounter) CountTokens(string) int {
	if counter.observe != nil {
		counter.observe()
	}
	return 1
}
func (*observingCounter) ContextWindow() int { return 1000 }

func TestStoreCounterCanObserveStoreDuringWrites(t *testing.T) {
	for _, operation := range []string{"inject", "rewrite", "batch", "externalize"} {
		t.Run(operation, func(t *testing.T) {
			counter := &observingCounter{}
			refs, err := NewMemoryReferences("session", 1<<20)
			if err != nil {
				t.Fatal(err)
			}
			store := NewStore(1000, 1000, counter, nil, StoreServices{References: refs})
			message := wire.TextMessage(wire.RoleUser, "original")
			message.ID = "message"
			if err := store.InjectMessage("session", abi.RuntimeInject, message); err != nil {
				t.Fatal(err)
			}
			counter.observe = func() { _ = store.Snapshot("session") }
			done := make(chan error, 1)
			go func() {
				switch operation {
				case "inject":
					done <- store.InjectMessage("session", abi.RuntimeInject, message)
				case "rewrite":
					done <- store.RewriteMessage("session", message.ID, message)
				case "batch":
					done <- store.RewriteBatch("session", []MessageRewrite{{ID: message.ID, Message: message}})
				case "externalize":
					_, err := store.Externalize(context.Background(), "session", abi.MessageRange{From: abi.MessageSelector{Kind: abi.FromStart}, To: abi.MessageSelector{Kind: abi.ToEnd}}, "")
					done <- err
				}
			}()
			select {
			case err := <-done:
				if err != nil {
					t.Fatal(err)
				}
			case <-time.After(time.Second):
				t.Fatal("counter called with store write lock held")
			}
		})
	}
}

type externalizerFixture func(context.Context, string, string) (wire.Reference, error)

func (fn externalizerFixture) Externalize(ctx context.Context, path, text string) (wire.Reference, error) {
	return fn(ctx, path, text)
}

func TestCompressionSettlesCallbacksBeforeReplacingView(t *testing.T) {
	for _, scenario := range []string{"cancelled_summary", "cancelled_externalization", "invalid_reference", "reference_processing"} {
		t.Run(scenario, func(t *testing.T) {
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			apply := DefaultApply{}
			action := abi.CompressAction{Kind: abi.CompressSummary, Range: abi.MessageRange{From: abi.MessageSelector{Kind: abi.FromStart}, To: abi.MessageSelector{Kind: abi.ToEnd}}}
			apply.Summarizer = SummarizerFunc(func(context.Context, []abi.ContextEntry, []string) (string, error) {
				cancel()
				return "late summary", nil
			})
			if scenario != "cancelled_summary" {
				action.Kind, action.Path = abi.CompressExternalize, "approved-output"
				apply.Externalizer = externalizerFixture(func(context.Context, string, string) (wire.Reference, error) {
					if scenario == "cancelled_externalization" {
						cancel()
					}
					if scenario == "invalid_reference" {
						return wire.Reference{}, nil
					}
					return wire.Reference{URI: "artifact://retained", Processing: "Original source evidence"}, nil
				})
			}
			messages := []wire.Message{wire.TextMessage(wire.RoleUser, "original")}
			before := wire.CloneMessages(messages)
			err := apply.ApplyCompressions(ctx, []abi.CompressAction{action}, &messages, NewStore(1000, 1000, nil, nil), "session")
			if scenario == "reference_processing" {
				if err != nil || len(messages) != 1 || messages[0].Content[0].Reference == nil || !strings.HasPrefix(messages[0].Content[0].Reference.Processing, "Original source evidence") {
					t.Fatal("externalizer provenance discarded", messages, err)
				}
				return
			}
			if err == nil || !reflect.DeepEqual(messages, before) {
				t.Fatal("late/invalid callback changed working view", messages, err)
			}
			if scenario != "invalid_reference" && !errors.Is(err, context.Canceled) {
				t.Fatal("cancellation identity lost", err)
			}
		})
	}
}

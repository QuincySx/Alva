// INPUT: context, errors, reflect, testing, kernel/abi, llm/wire
// OUTPUT: Conversation-log identity and cancellation cleanup regressions
// POS: [L2 kernel runtime] Accepts value logs with dynamically noncomparable fields without panicking during final settlement.
package core

import (
	"context"
	"errors"
	"reflect"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type configuredValueLog struct {
	config  any
	flushes *int
}

func (configuredValueLog) ID() string { return "value-log" }
func (configuredValueLog) Messages(context.Context) ([]wire.Message, error) {
	return nil, nil
}
func (configuredValueLog) AppendRecords(_ context.Context, records []abi.SessionRecord) ([]abi.SessionRecord, error) {
	return records, nil
}
func (log configuredValueLog) Flush(context.Context) error {
	*log.flushes++
	return nil
}

func TestLoopSettlesValueLogWithDynamicConfiguration(t *testing.T) {
	for _, config := range []any{"comparable", map[string]string{"backend": "custom"}, []string{"custom"}} {
		name := reflect.TypeOf(config).String()
		t.Run(name, func(t *testing.T) {
			defer func() {
				if value := recover(); value != nil {
					t.Errorf("valid value log panicked during cleanup: %v", value)
				}
			}()
			flushes := 0
			log := configuredValueLog{config: config, flushes: &flushes}
			state, err := abi.NewAgentState(fixtureModel{}, log, nil)
			if err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithCancel(t.Context())
			cancel()
			err = (DefaultAgentLoop{}).Run(ctx, RunConfig{State: state}, nil, func(abi.Event) error { return nil })
			if !errors.Is(err, context.Canceled) || flushes == 0 {
				t.Fatal("cancelled run failed to settle actual log", flushes, err)
			}
			if reflect.ValueOf(log).Comparable() && flushes != 1 {
				t.Fatal("comparable actual log was not deduplicated", flushes)
			}
		})
	}
}

// INPUT: context, encoding/json, errors, fmt, reflect, unicode/utf8, kernel/abi
// OUTPUT: BudgetAdmission
// POS: [L3 context policy] Optional final request estimation guard including tools and late wrapper additions.
package agentcontext

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

// BudgetAdmission enforces the configured input estimation budget at the final
// model interface boundary. Without a Counter, JSON rune count / 4 is a
// rough application heuristic, not a provider tokenizer or a multimodal
// token guarantee. Applications can replace this middleware with a model-aware
// admission policy. A missing finite budget makes no hard-budget claim.
type BudgetAdmission struct{ System *abi.ContextSystem }

func (BudgetAdmission) Name() string { return "context-budget" }
func (policy BudgetAdmission) AdmitModel(ctx context.Context, runtime abi.RuntimeContext, request abi.ModelRequest) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if policy.System == nil {
		return errors.New("context budget system is required")
	}
	system := policy.System
	if system.Counter != nil && missingBudgetCapability(system.Counter) {
		return errors.New("context budget counter implementation is required")
	}
	budget := system.BudgetTokens
	if budget <= 0 {
		if missingBudgetCapability(system.Handle) || runtime.InvocationSessionID == "" {
			return errors.New("context budget handle and runtime are required")
		}
		budget = system.Handle.Budget(runtime.InvocationSessionID).BudgetTokens
	}
	if budget <= 0 && system.Counter != nil {
		budget = system.Counter.ContextWindow()
	}
	if budget <= 0 {
		return nil
	}
	data, err := json.Marshal(request)
	if err != nil {
		return errors.New("context budget cannot encode final request")
	}
	used := (utf8.RuneCount(data) + 3) / 4
	if system.Counter != nil {
		used = system.Counter.CountTokens(string(data))
		if used <= 0 {
			return errors.New("context budget counter did not estimate the final request")
		}
	}
	if used > budget {
		return fmt.Errorf("final model request exceeds context estimation budget (%d > %d)", used, budget)
	}
	return ctx.Err()
}

func missingBudgetCapability(value any) bool {
	if value == nil {
		return true
	}
	v := reflect.ValueOf(value)
	switch v.Kind() {
	case reflect.Pointer, reflect.Map, reflect.Func, reflect.Chan, reflect.Interface, reflect.Slice:
		return v.IsNil()
	}
	return false
}

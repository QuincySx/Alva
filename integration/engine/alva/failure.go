// INPUT: Typed ProviderError evidence, complete execution error trees and actual response visibility
// OUTPUT: Conservative engine failure/outcome classification without retries or string matching
// POS: [L7 Alva product adapter] Alva adapter policy; mixed native/session/cleanup errors never authorize provider fallback
package alva

import (
	"context"

	"github.com/QuincySx/Alva/integration/engine"
	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

func executionFailure(ctx context.Context, err error, responseStarted bool) (engine.Outcome, engine.FailureKind) {
	if ctx.Err() != nil && abi.OnlyCancellation(err) {
		return engine.Cancelled, engine.FailureCancelled
	}
	failure, classified := providerFailure(err, 0)
	if !classified || responseStarted {
		failure = engine.FailureExecution
	}
	return engine.Failed, failure
}

// A ProviderError is a transport-evidence boundary, not a reason to use
// errors.As on an arbitrary joined tree. All outer siblings must independently
// classify to the same failure. The bounded traversal also fails closed on
// malformed/cyclic custom errors instead of recursing indefinitely.
func providerFailure(err error, depth int) (engine.FailureKind, bool) {
	if err == nil || depth >= 64 {
		return engine.FailureExecution, false
	}
	if _, storage := err.(*abi.SessionError); storage {
		return engine.FailureExecution, false
	}
	if value, ok := err.(*agentabi.ProviderError); ok {
		if value == nil || !plainProviderCause(value.Err, depth+1) {
			return engine.FailureExecution, false
		}
		status := 0
		if value.StatusCode != nil {
			status = *value.StatusCode
		}
		switch value.Kind {
		case agentabi.ProviderCredential:
			if !value.Retryable && (status == 0 || status == 401 || status == 403) {
				return engine.FailureAuth, true
			}
		case agentabi.ProviderRateLimited:
			if value.Retryable && status == 429 {
				return engine.FailureRateLimit, true
			}
		case agentabi.ProviderNetwork:
			// The canonical provider only emits Network when Do failed before
			// receiving a response. SSE/read/parse failures are not Network.
			if value.Retryable && status == 0 {
				return engine.FailureStartup, true
			}
		case agentabi.ProviderAPICall:
			if value.Retryable {
				switch status {
				case 408, 500, 502, 503, 504, 529:
					return engine.FailureStartup, true
				}
			}
		}
		return engine.FailureExecution, false
	}
	if joined, ok := err.(interface{ Unwrap() []error }); ok {
		children := joined.Unwrap()
		if len(children) == 0 {
			return engine.FailureExecution, false
		}
		var common engine.FailureKind
		for _, child := range children {
			failure, classified := providerFailure(child, depth+1)
			if !classified || (common != "" && common != failure) {
				return engine.FailureExecution, false
			}
			common = failure
		}
		return common, true
	}
	if wrapped, ok := err.(interface{ Unwrap() error }); ok {
		return providerFailure(wrapped.Unwrap(), depth+1)
	}
	return engine.FailureExecution, false
}

// Normal Network.Err is an untyped url/net/syscall wrapper chain. It is
// evidence for this single failed transport, not an aggregate of execution
// failures. Never allow typed native failures or joined sibling causes to hide
// beneath a ProviderError and regain retry eligibility.
func plainProviderCause(err error, depth int) bool {
	if err == nil {
		return true
	}
	if depth >= 64 {
		return false
	}
	switch err.(type) {
	case *abi.SessionError, *agentabi.ProviderError, interface{ Unwrap() []error }:
		return false
	}
	if err == context.Canceled || err == context.DeadlineExceeded {
		return false
	}
	if wrapped, ok := err.(interface{ Unwrap() error }); ok {
		return plainProviderCause(wrapped.Unwrap(), depth+1)
	}
	return true
}

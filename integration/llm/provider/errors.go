// INPUT: Actual HTTP status, bounded Retry-After header, a bounded HTTP 400 error code and transport failures
// OUTPUT: Typed ProviderError classifications (including structured context-length rejection) with private response bodies/URLs excluded
// POS: [integration / model transport] Provider transport evidence only; no retry/fallback/account policy or inferred model side effects
package provider

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"strings"
	"time"

	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
)

func statusError(kind Kind, response *http.Response) *agentabi.ProviderError {
	status := response.StatusCode
	value := &agentabi.ProviderError{Kind: agentabi.ProviderAPICall, Message: fmt.Sprintf("model endpoint returned HTTP %d", status), StatusCode: &status}
	switch status {
	case http.StatusUnauthorized, http.StatusForbidden:
		value.Kind = agentabi.ProviderCredential
	case http.StatusBadRequest:
		value.Kind = agentabi.ProviderInvalidArgument
		if contextLengthCode(kind, response.Body) {
			value.Kind = agentabi.ProviderContextLength
			value.Message = "model endpoint rejected the request as longer than the model context window (HTTP 400)"
		}
	case http.StatusRequestEntityTooLarge:
		value.Kind = agentabi.ProviderInvalidPrompt
	case http.StatusTooManyRequests:
		value.Kind = agentabi.ProviderRateLimited
		value.Retryable = true
	case http.StatusRequestTimeout, http.StatusInternalServerError, http.StatusBadGateway, http.StatusServiceUnavailable, http.StatusGatewayTimeout, 529:
		value.Retryable = true
	}
	// This is evidence for an explicitly chosen policy, not an automatic delay
	// or replay. Never retain arbitrary headers or provider response bodies.
	if value.Retryable {
		value.RetryAfter = retryAfter(response.Header.Get("Retry-After"), time.Now())
	}
	return value
}

// contextLengthCode reads at most 64 KiB of an HTTP 400 body and reports
// whether its structured error code is OpenAI's documented
// "context_length_exceeded". Only the two OpenAI protocols carry such a code;
// Anthropic and Gemini identify this rejection only in message text, which is
// never interpreted, so they stay invalid_argument. The body is discarded.
func contextLengthCode(kind Kind, body io.Reader) bool {
	if kind != OpenAIChatKind && kind != OpenAIResponsesKind || body == nil {
		return false
	}
	data, err := io.ReadAll(io.LimitReader(body, 64<<10+1))
	if err != nil || len(data) > 64<<10 {
		return false
	}
	var envelope struct {
		Error *struct {
			Code json.RawMessage `json:"code"`
		} `json:"error"`
	}
	if json.Unmarshal(data, &envelope) != nil || envelope.Error == nil {
		return false
	}
	var code string
	return json.Unmarshal(envelope.Error.Code, &code) == nil && code == "context_length_exceeded"
}

func retryAfter(raw string, now time.Time) *time.Duration {
	if len(raw) == 0 || len(raw) > 128 {
		return nil
	}
	raw = strings.TrimSpace(raw)
	if seconds, err := strconv.ParseUint(raw, 10, 64); err == nil {
		if seconds > 24*60*60 {
			return nil
		}
		value := time.Duration(seconds) * time.Second
		return &value
	}
	when, err := http.ParseTime(raw)
	if err != nil {
		return nil
	}
	duration := when.Sub(now)
	if duration < 0 {
		duration = 0
	}
	if duration > 24*time.Hour {
		return nil
	}
	return &duration
}
func networkError(err error) *agentabi.ProviderError {
	return &agentabi.ProviderError{Kind: agentabi.ProviderNetwork, Message: "model HTTP transport failed before a response was received", Retryable: true, Err: err}
}

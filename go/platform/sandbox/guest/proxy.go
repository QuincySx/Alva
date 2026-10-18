// INPUT: context, kernel/abi, llm/wire, sandbox/abi
// OUTPUT: ProxyModel, LoadContext, Fetch, Escalate, AppendAudit
// POS: [platform / WASM host or guest] Provider-free typed guest proxies preserving all model content and explicit capability failures.
package guest

import (
	"context"

	kernelabi "github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"github.com/QuincySx/Alva/go/sdk/sandbox/abi"
)

type ProxyModel struct{}

func (ProxyModel) Stream(ctx context.Context, request kernelabi.ModelRequest, emit func(wire.StreamChunk) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	proxy := abi.ModelRequest{Version: abi.Version, Messages: wire.CloneMessages(request.Messages), Config: wire.CloneModelConfig(request.Config)}
	for _, tool := range request.Tools {
		proxy.Tools = append(proxy.Tools, wire.ToolDefinition{Name: tool.Name, Description: tool.Description, Parameters: append([]byte(nil), tool.Parameters...)})
	}
	if err := proxy.Validate(); err != nil {
		return err
	}
	encoded, err := abi.Encode(proxy, abi.MaxModelRequestBytes)
	if err != nil {
		return err
	}
	data, err := exchange(abi.OpModel, encoded, abi.MaxResponseBytes)
	if err != nil {
		return err
	}
	var response abi.ModelResponse
	if err := abi.Decode(data, abi.MaxResponseBytes, &response); err != nil {
		return err
	}
	if err := response.Validate(); err != nil {
		return err
	}
	for _, chunk := range response.Chunks {
		if err := ctx.Err(); err != nil {
			return err
		}
		if err := emit(wire.CloneStreamChunk(chunk)); err != nil {
			return err
		}
	}
	return nil
}
func LoadContext() (abi.Context, error) {
	var value abi.Context
	data, err := exchange(abi.OpContext, []byte("{}"), abi.MaxContextBytes)
	if err != nil {
		return value, err
	}
	if err := abi.Decode(data, abi.MaxContextBytes, &value); err != nil {
		return value, err
	}
	return value, value.Validate()
}
func Fetch(ctx context.Context, request abi.FetchRequest) (abi.FetchResult, error) {
	var result abi.FetchResult
	if err := ctx.Err(); err != nil {
		return result, err
	}
	if err := request.Validate(); err != nil {
		return result, err
	}
	encoded, err := abi.Encode(request, abi.MaxFetchRequestBytes)
	if err != nil {
		return result, err
	}
	data, err := exchange(abi.OpFetch, encoded, abi.MaxResponseBytes)
	if err != nil {
		return result, err
	}
	if err := abi.Decode(data, abi.MaxResponseBytes, &result); err != nil {
		return result, err
	}
	return result, result.Validate()
}
func Escalate(ctx context.Context, request abi.EscalationRequest) (abi.EscalationResult, error) {
	var result abi.EscalationResult
	if err := ctx.Err(); err != nil {
		return result, err
	}
	if err := request.Validate(); err != nil {
		return result, err
	}
	encoded, err := abi.Encode(request, abi.MaxEscalationBytes)
	if err != nil {
		return result, err
	}
	data, err := exchange(abi.OpEscalation, encoded, abi.MaxResponseBytes)
	if err != nil {
		return result, err
	}
	if err := abi.Decode(data, abi.MaxResponseBytes, &result); err != nil {
		return result, err
	}
	return result, result.Validate()
}
func AppendAudit(event abi.AuditEvent) error {
	if err := event.Validate(); err != nil {
		return err
	}
	encoded, err := abi.Encode(event, abi.MaxAuditBytes)
	if err != nil {
		return err
	}
	_, err = exchange(abi.OpAudit, encoded, 0)
	return err
}

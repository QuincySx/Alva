// INPUT: context, encoding/base64/json, errors, fmt, strings, time, unicode/utf8, llm/wire, native/process
// OUTPUT: Config, Observer, ACP v1 DTOs, PermissionHandler and typed RPC errors
// POS: [integration / external protocol] Explicit official protocol values; no old custom envelopes, storage policy or automatic model delivery.
package acp

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/system/process"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const Version = 1
const maxQueuedBytes = 8 << 20

var ErrClosed = errors.New("ACP connection closed")

type Config struct {
	ID                                            string
	Process                                       process.Config
	ClientInfo                                    Implementation
	ConnectTimeout, RequestTimeout, CancelTimeout time.Duration
	ObserverTimeout                               time.Duration
	MaxFrameBytes                                 int
}
type Implementation struct {
	Name    string `json:"name"`
	Version string `json:"version"`
	Title   string `json:"title,omitempty"`
}
type IOEvent struct {
	ConnectionID string
	Direction    string
	// Stage is received, received_partial, received_truncated, write_attempt or
	// write_result. Stderr received frames include their original LF; partial
	// EOF and bounded truncated prefixes are explicit. A write attempt is not
	// proof of delivery or tool execution. This protocol never redacts data.
	Stage        string
	Data         []byte
	BytesWritten int
	Time         time.Time
}
type Observer interface {
	Observe(context.Context, IOEvent) error
}

// ObserverError keeps private journal paths/secrets out of public Error text;
// trusted callers retain the real cause through errors.Is/As and Unwrap.
type ObserverError struct{ Err error }

func (*ObserverError) Error() string     { return "ACP observer failed; interaction stopped" }
func (err *ObserverError) Unwrap() error { return err.Err }

type ObserverFunc func(context.Context, IOEvent) error

func (fn ObserverFunc) Observe(ctx context.Context, event IOEvent) error { return fn(ctx, event) }

type RPCError struct {
	Code    int             `json:"code"`
	Message string          `json:"message"`
	Data    json.RawMessage `json:"data,omitempty"`
}

func (err *RPCError) Error() string {
	return fmt.Sprintf("ACP RPC error %d: %s", err.Code, err.Message)
}

// Client capabilities are intentionally empty: this implementation offers
// permission callbacks but does not claim filesystem/terminal/elicitation APIs.
type InitializeRequest struct {
	ProtocolVersion    int             `json:"protocolVersion"`
	ClientCapabilities struct{}        `json:"clientCapabilities"`
	ClientInfo         *Implementation `json:"clientInfo,omitempty"`
}
type PromptCapabilities struct {
	Image           bool `json:"image,omitempty"`
	Audio           bool `json:"audio,omitempty"`
	EmbeddedContext bool `json:"embeddedContext,omitempty"`
}
type AgentCapabilities struct {
	LoadSession        bool               `json:"loadSession,omitempty"`
	PromptCapabilities PromptCapabilities `json:"promptCapabilities,omitempty"`
	MCP                json.RawMessage    `json:"mcpCapabilities,omitempty"`
	Session            json.RawMessage    `json:"sessionCapabilities,omitempty"`
	Auth               json.RawMessage    `json:"auth,omitempty"`
	Meta               json.RawMessage    `json:"_meta,omitempty"`
}
type InitializeResponse struct {
	ProtocolVersion   int               `json:"protocolVersion"`
	AgentCapabilities AgentCapabilities `json:"agentCapabilities,omitempty"`
	AgentInfo         *Implementation   `json:"agentInfo,omitempty"`
	AuthMethods       []json.RawMessage `json:"authMethods,omitempty"`
	Meta              json.RawMessage   `json:"_meta,omitempty"`
}

// Explicit MCP handoff is not accepted by this first client surface. The empty
// required mcpServers list is still sent on the wire; no ambient servers/grants.
type NewSessionRequest struct {
	Cwd        string     `json:"cwd"`
	MCPServers []struct{} `json:"mcpServers"`
}
type NewSessionResponse struct {
	SessionID     string          `json:"sessionId"`
	Modes         json.RawMessage `json:"modes,omitempty"`
	ConfigOptions json.RawMessage `json:"configOptions,omitempty"`
	Meta          json.RawMessage `json:"_meta,omitempty"`
}
type LoadSessionRequest struct {
	SessionID  string     `json:"sessionId"`
	Cwd        string     `json:"cwd"`
	MCPServers []struct{} `json:"mcpServers"`
}
type PromptRequest struct {
	SessionID string         `json:"sessionId"`
	Prompt    []ContentBlock `json:"prompt"`
}
type PromptResponse struct {
	StopReason string          `json:"stopReason"`
	Meta       json.RawMessage `json:"_meta,omitempty"`
}
type CancelNotification struct {
	SessionID string `json:"sessionId"`
}
type AuthenticateRequest struct {
	MethodID string `json:"methodId"`
}

type ContentBlock struct {
	Type        string          `json:"type"`
	Text        *string         `json:"text,omitempty"`
	URI         string          `json:"uri,omitempty"`
	Name        string          `json:"name,omitempty"`
	Title       string          `json:"title,omitempty"`
	Description string          `json:"description,omitempty"`
	Size        *int64          `json:"size,omitempty"`
	MimeType    string          `json:"mimeType,omitempty"`
	Data        *string         `json:"data,omitempty"`
	Resource    json.RawMessage `json:"resource,omitempty"`
	Annotations json.RawMessage `json:"annotations,omitempty"`
	Meta        json.RawMessage `json:"_meta,omitempty"`
}

func Text(text string) ContentBlock { return ContentBlock{Type: "text", Text: &text} }
func (block ContentBlock) Validate(capabilities PromptCapabilities) error {
	encoded, err := json.Marshal(block)
	if err != nil || !wire.ValidJSON(encoded) {
		return errors.New("invalid ACP content JSON")
	}
	for _, text := range []string{block.Type, block.URI, block.Name, block.Title, block.Description, block.MimeType} {
		if !utf8.ValidString(text) {
			return errors.New("ACP content metadata must be UTF-8")
		}
	}
	if block.Text != nil && !utf8.ValidString(*block.Text) {
		return errors.New("ACP text must be UTF-8")
	}
	switch block.Type {
	case "text":
		if block.Text == nil || block.Data != nil || len(block.Resource) != 0 || block.URI != "" {
			return errors.New("ACP text payload required")
		}
	case "resource_link":
		if block.URI == "" || block.Name == "" || block.Text != nil || block.Data != nil || len(block.Resource) != 0 || (block.Size != nil && *block.Size < 0) {
			return errors.New("ACP resource link identity required")
		}
	case "image", "audio":
		if block.Text != nil || len(block.Resource) != 0 {
			return errors.New("ACP mixed media/content payload")
		}
		if (block.Type == "image" && !capabilities.Image) || (block.Type == "audio" && !capabilities.Audio) {
			return errors.New("agent did not advertise selected media capability")
		}
		if block.Data == nil || block.MimeType == "" {
			return errors.New("ACP media data/mimeType required")
		}
		if _, err := base64.StdEncoding.DecodeString(*block.Data); err != nil {
			return errors.New("ACP media data must be base64")
		}
	case "resource":
		if block.Text != nil || block.Data != nil || block.URI != "" {
			return errors.New("ACP mixed resource/content payload")
		}
		if !capabilities.EmbeddedContext {
			return errors.New("agent did not advertise embedded context")
		}
		var resource struct {
			URI  string  `json:"uri"`
			Text *string `json:"text"`
			Blob *string `json:"blob"`
		}
		if json.Unmarshal(block.Resource, &resource) != nil || resource.URI == "" || (resource.Text == nil) == (resource.Blob == nil) {
			return errors.New("ACP embedded resource requires uri and text/blob union")
		}
	default:
		return errors.New("unsupported ACP prompt content")
	}
	return nil
}

type ToolCallUpdate struct {
	ToolCallID string          `json:"toolCallId"`
	Title      *string         `json:"title,omitempty"`
	Name       *string         `json:"name,omitempty"`
	Kind       *string         `json:"kind,omitempty"`
	Status     *string         `json:"status,omitempty"`
	RawInput   json.RawMessage `json:"rawInput,omitempty"`
	RawOutput  json.RawMessage `json:"rawOutput,omitempty"`
	Content    json.RawMessage `json:"content,omitempty"`
	Locations  json.RawMessage `json:"locations,omitempty"`
	Meta       json.RawMessage `json:"_meta,omitempty"`
}
type PermissionOption struct {
	OptionID string          `json:"optionId"`
	Name     string          `json:"name"`
	Kind     string          `json:"kind"`
	Meta     json.RawMessage `json:"_meta,omitempty"`
}
type PermissionRequest struct {
	SessionID string             `json:"sessionId"`
	ToolCall  ToolCallUpdate     `json:"toolCall"`
	Options   []PermissionOption `json:"options"`
	Meta      json.RawMessage    `json:"_meta,omitempty"`
}
type PermissionOutcome struct {
	Outcome  string `json:"outcome"`
	OptionID string `json:"optionId,omitempty"`
}
type PermissionResponse struct {
	Outcome PermissionOutcome `json:"outcome"`
}
type PermissionHandler interface {
	Approve(context.Context, PermissionRequest) (PermissionOutcome, error)
}
type PermissionFunc func(context.Context, PermissionRequest) (PermissionOutcome, error)

func (fn PermissionFunc) Approve(ctx context.Context, request PermissionRequest) (PermissionOutcome, error) {
	return fn(ctx, request)
}

// Update.Raw is the unchanged update object, including unknown future variants
// and omission/null patch distinctions. Typed projections never replace it.
type Update struct {
	SessionID string
	Kind      string
	MessageID *string
	Content   *ContentBlock
	Tool      *ToolCallUpdate
	Raw       json.RawMessage
	bytes     int
}

func identity(value string) bool {
	return value != "" && len(value) <= 512 && utf8.ValidString(value) && !strings.ContainsAny(value, "\x00\r\n")
}

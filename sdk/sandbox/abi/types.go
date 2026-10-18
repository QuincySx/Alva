// INPUT: encoding/json, errors, net/url, strings, unicode/utf8, llm/wire
// OUTPUT: Version, operation/limit constants, Context, ModelRequest/Response, FetchRequest/Response, EscalationRequest/Response, AuditEvent
// POS: [L0 foundation] Pure bounded capability messages shared by native hosts and WASIp1 guests.
package abi

import (
	"encoding/json"
	"errors"
	"net/url"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const Version uint32 = 1
const (
	OpContext uint32 = iota + 1
	OpModel
	OpFetch
	OpEscalation
	OpAudit
	OpReferenceRead
	OpReferenceSearch
	OpHistory
	OpScript
)
const (
	MaxContextBytes           = 256 << 10
	MaxModelRequestBytes      = 4 << 20
	MaxResponseBytes          = 16 << 20
	MaxFetchRequestBytes      = 4 << 20
	MaxFetchBodyBytes         = 1 << 20
	MaxFetchResponseBodyBytes = 4 << 20
	MaxAuditBytes             = 64 << 10
	MaxEscalationBytes        = 64 << 10
	MaxReferenceBytes         = 16 << 10
)

// Policy is a guest-side additional restriction, never a host approval grant.
type Policy struct {
	AllowFileWrite         bool `json:"allow_file_write"`
	AllowEscalationRequest bool `json:"allow_escalation_request"`
}
type Context struct {
	Version       uint32 `json:"version"`
	SessionID     string `json:"session_id"`
	Workspace     string `json:"workspace"`
	Task          string `json:"task"`
	SystemPrompt  string `json:"system_prompt"`
	MaxIterations int    `json:"max_iterations"`
	Policy        Policy `json:"policy"`
	// Attachments are indexes only, never implicitly expanded by the guest.
	Attachments      []wire.Reference `json:"attachments,omitempty"`
	HistoryAvailable bool             `json:"history_available,omitempty"`
	// ScriptAvailable advertises an independently isolated host-owned script
	// instance using only the job's existing guest-path and fetch grants.
	ScriptAvailable bool `json:"script_available,omitempty"`
}

func (value Context) Validate() error {
	if value.Version != Version || !Identity(value.SessionID) || !GuestPath(value.Workspace) || !Text(value.Task) || value.Task == "" || !Text(value.SystemPrompt) || value.MaxIterations < 1 || value.MaxIterations > 1000 {
		return errors.New("invalid worker context")
	}
	if len(value.Attachments) > 8 {
		return errors.New("too many worker attachment references")
	}
	for _, ref := range value.Attachments {
		if err := (wire.ContentBlock{Kind: wire.ContentReference, Reference: &ref}).Validate(); err != nil {
			return err
		}
	}
	return nil
}

type ModelRequest struct {
	Version  uint32                `json:"version"`
	Messages []wire.Message        `json:"messages"`
	Tools    []wire.ToolDefinition `json:"tools"`
	Config   wire.ModelConfig      `json:"config"`
}

func (value ModelRequest) Validate() error {
	if value.Version != Version || len(value.Messages) > 16384 || len(value.Tools) > 256 {
		return errors.New("invalid model proxy request")
	}
	if err := value.Config.Validate(); err != nil {
		return err
	}
	for _, message := range value.Messages {
		if err := message.Validate(); err != nil {
			return err
		}
	}
	names := make(map[string]bool)
	for _, tool := range value.Tools {
		if !Identity(tool.Name) || !Text(tool.Description) || !wire.ValidJSON(tool.Parameters) || names[tool.Name] {
			return errors.New("invalid proxy tool definition")
		}
		names[tool.Name] = true
	}
	return nil
}

type ModelResponse struct {
	Version uint32             `json:"version"`
	Chunks  []wire.StreamChunk `json:"chunks"`
}

func (value ModelResponse) Validate() error {
	if value.Version != Version || len(value.Chunks) < 1 || len(value.Chunks) > 65536 {
		return errors.New("invalid model proxy response")
	}
	accumulator := wire.NewStreamAccumulator(MaxResponseBytes)
	for _, chunk := range value.Chunks {
		if err := accumulator.Push(chunk); err != nil {
			return err
		}
	}
	_, err := accumulator.Finish()
	return err
}

type Header struct {
	Name  string `json:"name"`
	Value string `json:"value"`
}
type FetchRequest struct {
	Version uint32   `json:"version"`
	Method  string   `json:"method"`
	URL     string   `json:"url"`
	Headers []Header `json:"headers"`
	Body    []byte   `json:"body,omitempty"`
}

func (value FetchRequest) Validate() error {
	if value.Version != Version || !Identity(value.Method) || len(value.Method) > 32 || !Text(value.URL) || len(value.URL) > 8192 || len(value.Headers) > 128 || len(value.Body) > MaxFetchBodyBytes {
		return errors.New("invalid fetch request")
	}
	u, err := url.Parse(value.URL)
	if err != nil || u.Host == "" || u.User != nil || (u.Scheme != "http" && u.Scheme != "https") {
		return errors.New("invalid fetch URL")
	}
	for _, h := range value.Headers {
		if !Identity(h.Name) || !Text(h.Value) || len(h.Value) > 8192 || strings.ContainsAny(h.Value, "\r\n\x00") {
			return errors.New("invalid fetch header")
		}
	}
	return nil
}

type FetchResponse struct {
	Status  int      `json:"status"`
	Headers []Header `json:"headers"`
	Body    []byte   `json:"body,omitempty"`
}
type FetchResult struct {
	Version  uint32         `json:"version"`
	Response *FetchResponse `json:"response,omitempty"`
	Error    string         `json:"error,omitempty"`
}

func (value FetchResult) Validate() error {
	if value.Version != Version || (value.Response == nil) == (value.Error == "") || !Text(value.Error) {
		return errors.New("invalid fetch result")
	}
	if value.Response != nil && (value.Response.Status < 100 || value.Response.Status > 599 || len(value.Response.Body) > MaxFetchResponseBodyBytes || len(value.Response.Headers) > 256) {
		return errors.New("invalid fetch response")
	}
	if value.Response != nil {
		for _, header := range value.Response.Headers {
			if !Identity(header.Name) || !Text(header.Value) || len(header.Value) > 8192 || strings.ContainsAny(header.Value, "\x00\r\n") {
				return errors.New("invalid fetch response header")
			}
		}
	}
	return nil
}

type EscalationRequest struct {
	Version   uint32 `json:"version"`
	Command   string `json:"command"`
	Cwd       string `json:"cwd"`
	TimeoutMS uint64 `json:"timeout_ms"`
}

func (value EscalationRequest) Validate() error {
	if value.Version != Version || value.Command == "" || !Text(value.Command) || strings.IndexByte(value.Command, 0) >= 0 || !GuestPath(value.Cwd) || value.TimeoutMS < 1 || value.TimeoutMS > 3600000 {
		return errors.New("invalid escalation request")
	}
	return nil
}

type EscalationResponse struct {
	Stdout   string `json:"stdout"`
	Stderr   string `json:"stderr"`
	ExitCode int    `json:"exit_code"`
}
type EscalationResult struct {
	Version  uint32              `json:"version"`
	Response *EscalationResponse `json:"response,omitempty"`
	Error    string              `json:"error,omitempty"`
}

func (value EscalationResult) Validate() error {
	if value.Version != Version || (value.Response == nil) == (value.Error == "") || !Text(value.Error) {
		return errors.New("invalid escalation result")
	}
	if value.Response != nil && (!Text(value.Response.Stdout) || !Text(value.Response.Stderr)) {
		return errors.New("invalid escalation output")
	}
	return nil
}

// AuditEvent is untrusted guest evidence, not proof that host policy approved IO.
type AuditEvent struct {
	Version   uint32          `json:"version"`
	Kind      string          `json:"kind"`
	CallID    string          `json:"call_id,omitempty"`
	Tool      string          `json:"tool,omitempty"`
	IsError   bool            `json:"is_error,omitempty"`
	Summary   string          `json:"summary,omitempty"`
	Truncated bool            `json:"truncated,omitempty"`
	Payload   json.RawMessage `json:"payload,omitempty"`
}

func (value AuditEvent) Validate() error {
	if value.Version != Version || !Identity(value.Kind) || !Text(value.CallID) || !Text(value.Tool) || !Text(value.Summary) || len(value.Summary) > 4096 || (value.Payload != nil && !wire.ValidJSON(value.Payload)) {
		return errors.New("invalid guest audit event")
	}
	return nil
}
func Text(value string) bool { return utf8.ValidString(value) }
func Identity(value string) bool {
	return value != "" && len(value) <= 512 && Text(value) && !strings.ContainsAny(value, "\x00\r\n")
}
func GuestPath(value string) bool {
	return strings.HasPrefix(value, "/") && len(value) <= 4096 && Text(value) && !strings.ContainsAny(value, "\x00\\")
}

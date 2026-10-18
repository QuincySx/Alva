// INPUT: JSON values, explicit host capability grants and native owned process configuration
// OUTPUT: AEP version, handshake/tool/action contracts, Handler, Observer and Config
// POS: [integration / external protocol] Third-party Plugin protocol, distinct from SDK policy, bundle loading and persistence
package aep

import (
	"context"
	"encoding/json"
	"time"

	"github.com/QuincySx/Alva/platform/system/process"
)

const Version = "0.1.0"
const FrameLimit = 16 << 20

type HostCapabilities struct {
	StateAccess []string `json:"stateAccess"`
	Events      []string `json:"events"`
	HostAPI     []string `json:"hostApi"`
}
type PluginInfo struct {
	Name        string `json:"name"`
	Version     string `json:"version"`
	Description string `json:"description,omitempty"`
	Author      string `json:"author,omitempty"`
}
type Tool struct {
	Name        string          `json:"name"`
	Description string          `json:"description"`
	InputSchema json.RawMessage `json:"inputSchema"`
}
type Handshake struct {
	ProtocolVersion string     `json:"protocolVersion"`
	Plugin          PluginInfo `json:"plugin"`
	Tools           []Tool     `json:"tools"`
	Subscriptions   []string   `json:"eventSubscriptions"`
	Capabilities    []string   `json:"requestedCapabilities"`
}
type Config struct {
	Name, PluginVersion string
	Process             process.Config
	// These are independently approved manifest grants, not permissions supplied
	// by the child handshake. No undeclared callback is ever invoked.
	RequestedCapabilities          []string
	HostCapabilities               HostCapabilities
	ConnectTimeout, RequestTimeout time.Duration
}
type Handler interface {
	Handle(context.Context, string, json.RawMessage) (json.RawMessage, error)
}

// CallbackError carries only a public protocol classification. Arbitrary host
// error text (paths, credentials, database details) never crosses this boundary.
type CallbackError struct{ Code int }

func (e *CallbackError) Error() string { return "AEP host callback rejected" }

const (
	HandleExpired    = -32000
	CapabilityDenied = -32001
	InvalidParams    = -32602
	MethodNotFound   = -32601
)

type HandlerFunc func(context.Context, string, json.RawMessage) (json.RawMessage, error)

func (f HandlerFunc) Handle(ctx context.Context, method string, params json.RawMessage) (json.RawMessage, error) {
	return f(ctx, method, params)
}

type IOEvent struct {
	Direction string
	Frame     json.RawMessage
	At        time.Time
}
type Observer interface {
	Record(context.Context, IOEvent) error
}
type ObserverFunc func(context.Context, IOEvent) error

func (f ObserverFunc) Record(ctx context.Context, event IOEvent) error { return f(ctx, event) }

type Action struct {
	Action            string          `json:"action"`
	Reason            string          `json:"reason,omitempty"`
	ModifiedArguments json.RawMessage `json:"modified_arguments,omitempty"`
	Result            json.RawMessage `json:"result,omitempty"`
	Messages          json.RawMessage `json:"messages,omitempty"`
	Response          json.RawMessage `json:"response,omitempty"`
}

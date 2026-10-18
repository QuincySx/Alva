// INPUT: context, encoding/json, crypto/sha256, explicit native process configuration
// OUTPUT: Config, IOEvent, Observer, Tool, Transport, ToolAlias
// POS: [integration / external protocol] MCP protocol contracts without persistence policy or automatic model exposure
package mcp

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"time"

	"github.com/QuincySx/Alva/go/platform/system/process"
)

const FrameLimit = 1 << 20
const CatalogLimit = 32 << 20
const ToolLimit = 8192
const PageLimit = 128
const SchemaLimit = 64 << 10

var versions = []string{"2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"}

type Config struct {
	ID             string
	Process        process.Config
	ConnectTimeout time.Duration
	RequestTimeout time.Duration
}
type IOEvent struct {
	Server    string          `json:"server"`
	Direction string          `json:"direction"`
	Frame     json.RawMessage `json:"frame"`
	At        time.Time       `json:"at"`
}
type Observer interface {
	Record(context.Context, IOEvent) (string, error)
}
type ObserverFunc func(context.Context, IOEvent) (string, error)

func (f ObserverFunc) Record(ctx context.Context, event IOEvent) (string, error) {
	return f(ctx, event)
}

type Tool struct {
	Name        string          `json:"name"`
	Description string          `json:"description,omitempty"`
	InputSchema json.RawMessage `json:"inputSchema"`
}
type Transport interface {
	Connected() bool
	ListTools(context.Context) ([]Tool, error)
	CallTool(context.Context, string, json.RawMessage) (json.RawMessage, error)
	Close(context.Context) error
}

// Length prefixes distinguish identities without relying on separators in logical names.
func ToolAlias(server, tool string) string {
	hash := sha256.Sum256([]byte(fmt.Sprintf("%d:%s%d:%s", len(server), server, len(tool), tool)))
	return "mcp_" + hex.EncodeToString(hash[:30])
}

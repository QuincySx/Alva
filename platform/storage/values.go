// INPUT: time, kernel/abi, llm/wire and pure immutable blob retention vocabulary.
// OUTPUT: Backend-independent session, record, artifact, causal-link and inspection values.
// POS: [storage contract] Keeps all consumers on shared values without importing a concrete database or file implementation.
package storage

import (
	"time"

	"github.com/QuincySx/Alva/platform/storage/blob"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type SessionInfo struct {
	ID        string    `json:"id"`
	ParentID  string    `json:"parent_id,omitempty"`
	CreatedAt time.Time `json:"created_at"`
	HighWater uint64    `json:"high_water"`
}

type SessionPage struct {
	Sessions []SessionInfo `json:"sessions"`
	Next     string        `json:"next"`
	HasMore  bool          `json:"has_more"`
}

type ArtifactInput struct {
	Kind, Name, MediaType, Processing string
	Data                              []byte
	Text                              bool
}

type ArtifactInfo struct {
	ID         string `json:"id"`
	SessionID  string `json:"session_id"`
	Kind       string `json:"kind"`
	Name       string `json:"name"`
	MediaType  string `json:"media_type"`
	Processing string `json:"processing"`
	SHA256     string `json:"sha256"`
	Bytes      int64  `json:"bytes"`
	Text       bool   `json:"is_text"`
}

type ArtifactPage struct {
	Info       ArtifactInfo `json:"info"`
	Offset     int64        `json:"offset"`
	NextOffset int64        `json:"next_offset"`
	Data       []byte       `json:"data"`
	Truncated  bool         `json:"truncated"`
}

type RecordEntry struct {
	ID            string               `json:"id"`
	Sequence      uint64               `json:"sequence"`
	Time          time.Time            `json:"time"`
	Kind          abi.RecordKind       `json:"kind"`
	Namespace     string               `json:"namespace"`
	Type          string               `json:"type"`
	SchemaVersion uint32               `json:"schema_version"`
	Parent        *abi.RecordReference `json:"parent,omitempty"`
	Summary       string               `json:"summary"`
	Value         wire.Reference       `json:"value"`
}

type RecordPage struct {
	Entries   []RecordEntry `json:"entries"`
	Next      uint64        `json:"next"`
	HighWater uint64        `json:"high_water"`
	HasMore   bool          `json:"has_more"`
}

type RecordMatch struct {
	Entry   RecordEntry `json:"entry"`
	Offset  int64       `json:"offset"`
	Snippet string      `json:"snippet"`
}

type SearchPage struct {
	Matches   []RecordMatch `json:"matches"`
	Next      uint64        `json:"next"`
	HighWater uint64        `json:"high_water"`
	HasMore   bool          `json:"has_more"`
}

type ChildCreation struct {
	Session Session
	Link    abi.RecordReference
}

type ChildLink struct {
	ParentID string              `json:"parent_id"`
	ChildID  string              `json:"child_id"`
	Link     abi.RecordReference `json:"link"`
}

type RecoveryReport struct{ ModelIntents, ToolIntents, UnknownToolExecutions int }

type ArtifactEntry struct {
	Sequence uint64       `json:"sequence"`
	Info     ArtifactInfo `json:"info"`
}

type ArtifactIndexPage struct {
	Entries   []ArtifactEntry `json:"entries"`
	Next      uint64          `json:"next"`
	HighWater uint64          `json:"high_water"`
	HasMore   bool            `json:"has_more"`
}

type BundleOptions struct {
	HighWater       uint64
	IncludeChildren bool
}

type BundleSummary struct {
	RootID     string            `json:"root_id"`
	Sessions   int               `json:"sessions"`
	Records    int               `json:"records"`
	Bytes      int               `json:"bytes"`
	SHA256     string            `json:"sha256"`
	HighWaters map[string]uint64 `json:"high_waters"`
}

// ConversationMetadata is detached metadata, not a grant to read its artifact.
type ConversationMetadata struct {
	Record       abi.RecordReference
	Role         wire.Role
	Reference    wire.Reference
	RestoreBytes int64
}

type BlobStorageInfo struct {
	ID        string         `json:"id"`
	Retention blob.Retention `json:"retention"`
}

func (info ArtifactInfo) Reference() wire.Reference {
	return wire.Reference{URI: "alva-artifact://" + info.ID, MediaType: info.MediaType, SHA256: info.SHA256, Bytes: info.Bytes, Processing: info.Processing}
}

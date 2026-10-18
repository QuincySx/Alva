// INPUT: context, time and trusted host-bound native inspection identities
// OUTPUT: Inspector read contract, bounded metadata/reference DTOs and independent inspection capabilities
// POS: [L4.5 engine contract] Observation is independent of execution and never grants scope, restores an Agent or owns cancellation.
package engine

import (
	"context"
	"time"
)

type InspectionLevel string

const (
	InspectionUnsupported InspectionLevel = "unsupported"
	InspectionSummary     InspectionLevel = "summary"
	InspectionFull        InspectionLevel = "full"
)

type InspectionCapabilities struct {
	Level             InspectionLevel `json:"level"`
	Historical        bool            `json:"historical"`
	Live              bool            `json:"live"`
	RawRecords        bool            `json:"raw_records"`
	RecursiveChildren bool            `json:"recursive_children"`
	ExportJSONL       bool            `json:"export_jsonl_bundle"`
}

// InspectionScope is supplied by trusted product routing. An HTTP/native UI
// caller must not select an independent owner or native identity as a grant.
type InspectionScope struct{ OwnerID, NativeID string }
type InspectionQuery struct {
	After, HighWater uint64
	Limit            int
	Literal          string
}
type InspectionRecordReference struct {
	SessionID string `json:"session_id"`
	RecordID  string `json:"record_id"`
	Sequence  uint64 `json:"sequence"`
}
type InspectionReference struct {
	URI        string `json:"uri"`
	MediaType  string `json:"media_type,omitempty"`
	SHA256     string `json:"sha256,omitempty"`
	Bytes      int64  `json:"bytes,omitempty"`
	Processing string `json:"processing,omitempty"`
}
type InspectionEntry struct {
	ID            string                     `json:"id"`
	Sequence      uint64                     `json:"sequence"`
	Time          time.Time                  `json:"time"`
	Kind          string                     `json:"kind"`
	Namespace     string                     `json:"namespace"`
	Type          string                     `json:"type"`
	SchemaVersion uint32                     `json:"schema_version"`
	Parent        *InspectionRecordReference `json:"parent,omitempty"`
	Summary       string                     `json:"summary"`
	Value         InspectionReference        `json:"value"`
}
type InspectionPage struct {
	Entries   []InspectionEntry `json:"entries"`
	Next      uint64            `json:"next"`
	HighWater uint64            `json:"high_water"`
	HasMore   bool              `json:"has_more"`
}
type InspectionText struct {
	URI        string `json:"uri"`
	Text       string `json:"text"`
	Offset     int64  `json:"offset"`
	NextOffset int64  `json:"next_offset"`
	TotalBytes int64  `json:"total_bytes"`
	Truncated  bool   `json:"truncated"`
}
type InspectionChild struct {
	ParentID string                    `json:"parent_id"`
	ChildID  string                    `json:"child_id"`
	Link     InspectionRecordReference `json:"link"`
}
type InspectionBundle struct {
	Data       []byte            `json:"-"`
	RootID     string            `json:"root_id"`
	Sessions   int               `json:"sessions"`
	Records    int               `json:"records"`
	SHA256     string            `json:"sha256"`
	HighWaters map[string]uint64 `json:"high_waters"`
}

// Inspector observes committed facts only. Observe is lossless/backpressured:
// a slow sink stops reads, and its error or context cancellation detaches only
// this observer. Export must fail without partial bytes when bounds are hit.
// It deliberately exposes no execution, permission, or deletion operation.
type Inspector interface {
	InspectionCapabilities() InspectionCapabilities
	Page(context.Context, InspectionScope, InspectionQuery) (InspectionPage, error)
	Read(context.Context, InspectionScope, string, int64) (InspectionText, error)
	Child(context.Context, InspectionScope, uint64) (InspectionChild, error)
	Observe(context.Context, InspectionScope, uint64, func(InspectionPage) error) error
	Export(context.Context, InspectionScope, uint64, bool) (InspectionBundle, error)
}

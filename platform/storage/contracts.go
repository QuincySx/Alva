// INPUT: context, JSON, SDK log/reference contracts and shared storage values.
// OUTPUT: Session and composable storage ports shared by every consumer and backend.
// POS: [storage contract] Defines the single storage boundary; only outer assembly selects implementations.
package storage

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"

	"github.com/QuincySx/Alva/platform/storage/blob"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/memory"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

var ErrNotFound = errors.New("session or artifact is unavailable to this owner")
var ErrActive = errors.New("session already has an active run")
var ErrClosed = errors.New("storage is closed")
var ErrRevisionConflict = errors.New("extension snapshot revision changed; no write committed")
var ErrBindingConflict = errors.New("session identity is already bound to another owner or workspace")

const MaxArtifactBytes = 20 << 20
const MaxBundleBytes = 8 << 20
const MaxBundleRecords = 2048
const MaxBundleSessions = 32

// Session preserves the SDK's atomic batch contract. SameAuthority compares
// actual provider ownership, never just caller-controlled textual IDs.
// AppendExtensionSnapshot is an atomic compare-and-append against the latest
// exact namespace/type stream, including all additional records or none.
// An error does not authorize automatic retry or replay of an external effect.
type Session interface {
	abi.PersistentSession
	SameAuthority(abi.ConversationLog) bool
	AppendExtensionSnapshot(ctx context.Context, namespace, kind string, expectedRevision uint64, payload json.RawMessage, additional ...abi.SessionRecord) (abi.SessionRecord, error)
}

type SessionReader interface {
	OpenSession(ctx context.Context, owner, id string) (Session, error)
}
type Sessions interface {
	SessionReader
	Create(ctx context.Context, owner, id, parent string) (Session, error)
	List(ctx context.Context, owner, after string, limit int) (SessionPage, error)
}
type RecordRetention interface {
	RecordRetention() blob.Retention
}
type Runs interface {
	// Claim and interruption settlement precede execution. The returned
	// release is idempotent; it never cancels or replays an actual tool.
	BeginRun(ctx context.Context, owner, id string) (RecoveryReport, func(), error)
	Recover(ctx context.Context, owner, id string) (RecoveryReport, error)
}
type RecordLookup interface {
	LatestRecord(ctx context.Context, owner, id, namespace, kind string) (RecordEntry, bool, error)
}
type Records interface {
	RecordLookup
	Page(ctx context.Context, owner, id string, after uint64, limit int, highWater uint64) (RecordPage, error)
	Search(ctx context.Context, owner, id, query string, after uint64, limit int, highWater uint64) (SearchPage, error)
}
type ConversationTail interface {
	SessionID() string
	HighWater() uint64
	Metadata() ([]ConversationMetadata, error)
	Read(ctx context.Context, count, maxBytes int) ([]wire.Message, error)
}
type Conversations interface {
	BoundConversationTail(ctx context.Context, owner, id string, limit int) (ConversationTail, error)
}
type ArtifactWriter interface {
	PutArtifact(ctx context.Context, owner, id string, input ArtifactInput) (ArtifactInfo, error)
}
type ArtifactReader interface {
	ArtifactMetadata(ctx context.Context, owner, id, uri string) (ArtifactInfo, error)
	ReadArtifact(ctx context.Context, owner, id, uri string, offset int64, limit int) (ArtifactPage, error)
	ReadArtifactFull(ctx context.Context, owner, id, uri string, limit int) (ArtifactInfo, []byte, error)
	PageArtifacts(ctx context.Context, owner, id, kind, name string, after, highWater uint64, limit int) (ArtifactIndexPage, error)
}
type References interface {
	BoundReferences(ctx context.Context, owner, id string) (agentcontext.ReferenceBackend, error)
	ArtifactStorage() BlobStorageInfo
}
type Artifacts interface {
	ArtifactWriter
	ArtifactReader
	References
}

// Journal is the small common requirement of execution-evidence Plugins.
type Journal interface {
	SessionReader
	ArtifactWriter
}
type ChildReader interface {
	Parent(ctx context.Context, owner, id string) (string, error)
	ParentLink(ctx context.Context, owner, id string) (ChildLink, error)
	ResolveChildLink(ctx context.Context, owner, parent string, sequence uint64) (ChildLink, error)
}
type Children interface {
	ChildReader
	CreateChild(ctx context.Context, owner, parent, id, runID, callID, scope string) (ChildCreation, error)
}
type Bundles interface {
	ExportBundle(ctx context.Context, owner, id string, options BundleOptions) ([]byte, BundleSummary, error)
}

// Repository composes an application's full provider. Individual components
// consume only the narrower ports they use. No implementation type is part of
// this interface, and no consumer may downcast it to select another backend.
// The supplying assembly owns Close after actual execution/resources join.
// Close is idempotent after success; an error retains ownership for retry.
type Repository interface {
	Sessions
	RecordRetention
	Runs
	Records
	Conversations
	Artifacts
	Children
	Bundles
	Close() error
}

// MemoryStore supplies an explicitly bound memory namespace through the same
// SDK Backend contract used by the service and Plugins. Close is idempotent
// after success; an error retains ownership for retry.
type MemoryStore interface {
	Bind(namespace string) (memory.Backend, error)
	Close() error
}

type SessionBinding struct{ ID, Owner, Workspace string }

// BindingStore is an already-scoped namespace for external runtime identity.
// PutBinding is create-or-verify: identical values are idempotent, an existing
// different owner/workspace fails without changing it. Close transfers only
// after the supplying assembly successfully installs this owned component.
// Close is idempotent after success; an error retains ownership for retry.
type BindingStore interface {
	ReadBinding(ctx context.Context, id string) (SessionBinding, error)
	PutBinding(ctx context.Context, binding SessionBinding) error
	Close() error
}

// Missing admits no typed-nil provider through an interface slot.
func Missing(value any) bool {
	if value == nil {
		return true
	}
	v := reflect.ValueOf(value)
	switch v.Kind() {
	case reflect.Pointer, reflect.Map, reflect.Slice, reflect.Interface, reflect.Func, reflect.Chan:
		return v.IsNil()
	}
	return false
}

// INPUT: Product branch routing and registered owner-scoped EngineInspector
// OUTPUT: Typed-child Inspector pages/live observation, explicit bounded record text and disabled MCP config import
// POS: [L7 Hub product] Read-only product Inspector never resumes an Agent, reconstructs a runtime or starts MCP
package product

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func (s *HubService) boundBranch(ctx context.Context, id, branchID string) (hub.Branch, error) {
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return hub.Branch{}, err
	}
	for _, branch := range detail.Branches {
		if branch.ID == branchID && branch.NativeSessionID != "" {
			return branch, nil
		}
	}
	return hub.Branch{}, errors.New("branch has no owned native execution records")
}

type InspectorPage struct {
	NativeID        string                        `json:"native_id"`
	Capabilities    engine.InspectionCapabilities `json:"capabilities"`
	Entries         []storage.RecordEntry
	Next, HighWater uint64
	HasMore         bool
	Processing      string
}

func (s *HubService) Inspector(ctx context.Context, id, branchID, query string, after, highWater uint64) (InspectorPage, error) {
	return s.InspectorAt(ctx, id, branchID, nil, query, after, highWater)
}

type ChildHop struct {
	LinkSequence uint64 `json:"link_sequence"`
}

func (s *HubService) inspectionTarget(ctx context.Context, id, branchID string, path []ChildHop) (engine.Inspector, engine.InspectionScope, error) {
	branch, err := s.boundBranch(ctx, id, branchID)
	if err != nil {
		return nil, engine.InspectionScope{}, err
	}
	descriptor, ok := s.Runtime.Descriptor(branch.Target.EngineID)
	if !ok || descriptor.Inspector == nil || !descriptor.Inspector.InspectionCapabilities().Historical {
		return nil, engine.InspectionScope{}, engine.ErrUnsupported
	}
	inspector := descriptor.Inspector
	if len(path) > 8 || (len(path) > 0 && !inspector.InspectionCapabilities().RecursiveChildren) {
		return nil, engine.InspectionScope{}, errors.New("Inspector child depth/capability unavailable")
	}
	scope := engine.InspectionScope{OwnerID: id, NativeID: branch.NativeSessionID}
	seen := map[string]bool{scope.NativeID: true}
	for _, hop := range path {
		link, err := inspector.Child(ctx, scope, hop.LinkSequence)
		if err != nil {
			return nil, engine.InspectionScope{}, err
		}
		if link.ParentID != scope.NativeID || link.Link.SessionID != scope.NativeID || link.Link.Sequence != hop.LinkSequence || link.ChildID == "" || seen[link.ChildID] {
			return nil, engine.InspectionScope{}, errors.New("Inspector child link cycle or target mismatch")
		}
		scope.NativeID = link.ChildID
		seen[scope.NativeID] = true
	}
	return inspector, scope, nil
}
func productEntry(entry engine.InspectionEntry) storage.RecordEntry {
	result := storage.RecordEntry{ID: entry.ID, Sequence: entry.Sequence, Time: entry.Time, Kind: abi.RecordKind(entry.Kind), Namespace: entry.Namespace, Type: entry.Type, SchemaVersion: entry.SchemaVersion, Summary: entry.Summary, Value: wire.Reference{URI: entry.Value.URI, MediaType: entry.Value.MediaType, SHA256: entry.Value.SHA256, Bytes: entry.Value.Bytes, Processing: entry.Value.Processing}}
	if entry.Parent != nil {
		result.Parent = &abi.RecordReference{SessionID: entry.Parent.SessionID, RecordID: entry.Parent.RecordID, Sequence: entry.Parent.Sequence}
	}
	return result
}
func inspectorPage(nativeID, query string, page engine.InspectionPage) InspectorPage {
	result := InspectorPage{NativeID: nativeID, Next: page.Next, HighWater: page.HighWater, HasMore: page.HasMore, Processing: "Committed metadata only; bodies remain native references. Typed child chain is explicitly bound to this claimed product branch; no runtime or MCP worker was started."}
	if query != "" {
		result.Processing = "Literal text search in a bounded native-record page. Continue at the returned cursor; this is not a global history scan."
	}
	for _, entry := range page.Entries {
		result.Entries = append(result.Entries, productEntry(entry))
	}
	return result
}
func (s *HubService) InspectorAt(ctx context.Context, id, branchID string, path []ChildHop, query string, after, highWater uint64) (InspectorPage, error) {
	inspector, scope, err := s.inspectionTarget(ctx, id, branchID, path)
	if err != nil {
		return InspectorPage{}, err
	}
	page, err := inspector.Page(ctx, scope, engine.InspectionQuery{After: after, HighWater: highWater, Limit: 10, Literal: query})
	result := inspectorPage(scope.NativeID, query, page)
	result.Capabilities = inspector.InspectionCapabilities()
	return result, err
}
func (s *HubService) InspectorRead(ctx context.Context, id, branchID, uri string, offset int64) (agentcontext.TextPage, error) {
	return s.InspectorReadAt(ctx, id, branchID, nil, uri, offset)
}
func (s *HubService) InspectorReadAt(ctx context.Context, id, branchID string, path []ChildHop, uri string, offset int64) (agentcontext.TextPage, error) {
	inspector, scope, err := s.inspectionTarget(ctx, id, branchID, path)
	if err != nil {
		return agentcontext.TextPage{}, err
	}
	page, err := inspector.Read(ctx, scope, uri, offset)
	return agentcontext.TextPage{URI: page.URI, Text: page.Text, Offset: page.Offset, NextOffset: page.NextOffset, TotalBytes: page.TotalBytes, Truncated: page.Truncated}, err
}

// ObserveInspectorAt binds its scope once to the selected durable branch and
// exact typed ancestry. Detaching this observer never cancels its engine.
func (s *HubService) ObserveInspectorAt(ctx context.Context, id, branchID string, path []ChildHop, after uint64, sink func(InspectorPage) error) error {
	inspector, scope, err := s.inspectionTarget(ctx, id, branchID, path)
	if err != nil {
		return err
	}
	if sink == nil || !inspector.InspectionCapabilities().Live {
		return engine.ErrUnsupported
	}
	return inspector.Observe(ctx, scope, after, func(page engine.InspectionPage) error {
		result := inspectorPage(scope.NativeID, "", page)
		result.Capabilities = inspector.InspectionCapabilities()
		return sink(result)
	})
}
func (s *HubService) SaveMCPDraft(ctx context.Context, id, raw string) error {
	if len(raw) > 64<<10 {
		return errors.New("MCP configuration exceeds 64 KiB editor budget")
	}
	if trimmed := bytes.TrimSpace([]byte(raw)); len(trimmed) == 0 || trimmed[0] != '[' {
		return errors.New("MCP configuration must be an explicit JSON array, not null")
	}
	decoder := json.NewDecoder(bytes.NewBufferString(raw))
	decoder.DisallowUnknownFields()
	var servers []hub.MCPServer
	if err := decoder.Decode(&servers); err != nil {
		return err
	}
	if decoder.Decode(new(any)) != io.EOF {
		return errors.New("trailing MCP configuration")
	}
	return s.Store.ReplaceMCP(ctx, id, servers)
}

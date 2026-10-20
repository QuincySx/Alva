// INPUT: Explicit native Repository and engine-only observation DTOs
// OUTPUT: RepositoryInspector cold metadata/search/text, causal child, backpressured observation and bounded JSONL export
// POS: [L7 Alva product adapter] Committed native facts remain authoritative; observation neither creates nor restores execution.
package alva

import (
	"context"
	"errors"
	"time"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage"
)

// Storage is the capability contract supplied by the outer assembly.
type Storage interface {
	storage.Records
	storage.References
	storage.ChildReader
	storage.Bundles
}

type RepositoryInspector struct{ repository Storage }

func NewRepositoryInspector(repository Storage) (*RepositoryInspector, error) {
	if storage.Missing(repository) {
		return nil, errors.New("native inspection repository required")
	}
	return &RepositoryInspector{repository: repository}, nil
}
func (*RepositoryInspector) InspectionCapabilities() engine.InspectionCapabilities {
	return engine.InspectionCapabilities{Level: engine.InspectionFull, Historical: true, Live: true, RawRecords: true, RecursiveChildren: true, ExportJSONL: true}
}
func inspectionEntry(entry storage.RecordEntry) engine.InspectionEntry {
	result := engine.InspectionEntry{ID: entry.ID, Sequence: entry.Sequence, Time: entry.Time, Kind: string(entry.Kind), Namespace: entry.Namespace, Type: entry.Type, SchemaVersion: entry.SchemaVersion, Summary: entry.Summary, Value: engine.InspectionReference{URI: entry.Value.URI, MediaType: entry.Value.MediaType, SHA256: entry.Value.SHA256, Bytes: entry.Value.Bytes, Processing: entry.Value.Processing}}
	if entry.Parent != nil {
		result.Parent = &engine.InspectionRecordReference{SessionID: entry.Parent.SessionID, RecordID: entry.Parent.RecordID, Sequence: entry.Parent.Sequence}
	}
	return result
}
func (i *RepositoryInspector) Page(ctx context.Context, scope engine.InspectionScope, query engine.InspectionQuery) (engine.InspectionPage, error) {
	if query.Limit < 1 || query.Limit > 20 {
		return engine.InspectionPage{}, errors.New("inspection metadata requires1..20 records")
	}
	var result engine.InspectionPage
	if query.Literal == "" {
		page, err := i.repository.Page(ctx, scope.OwnerID, scope.NativeID, query.After, query.Limit, query.HighWater)
		if err != nil {
			return result, err
		}
		result.Next, result.HighWater, result.HasMore = page.Next, page.HighWater, page.HasMore
		for _, entry := range page.Entries {
			result.Entries = append(result.Entries, inspectionEntry(entry))
		}
		return result, nil
	}
	page, err := i.repository.Search(ctx, scope.OwnerID, scope.NativeID, query.Literal, query.After, query.Limit, query.HighWater)
	if err != nil {
		return result, err
	}
	result.Next, result.HighWater, result.HasMore = page.Next, page.HighWater, page.HasMore
	for _, match := range page.Matches {
		result.Entries = append(result.Entries, inspectionEntry(match.Entry))
	}
	return result, nil
}
func (i *RepositoryInspector) Read(ctx context.Context, scope engine.InspectionScope, uri string, offset int64) (engine.InspectionText, error) {
	backend, err := i.repository.BoundReferences(ctx, scope.OwnerID, scope.NativeID)
	if err != nil {
		return engine.InspectionText{}, err
	}
	page, err := backend.ReadText(ctx, uri, offset, 2048)
	return engine.InspectionText{URI: page.URI, Text: page.Text, Offset: page.Offset, NextOffset: page.NextOffset, TotalBytes: page.TotalBytes, Truncated: page.Truncated}, err
}
func (i *RepositoryInspector) Child(ctx context.Context, scope engine.InspectionScope, sequence uint64) (engine.InspectionChild, error) {
	link, err := i.repository.ResolveChildLink(ctx, scope.OwnerID, scope.NativeID, sequence)
	if err != nil {
		return engine.InspectionChild{}, err
	}
	indexed, err := i.repository.ParentLink(ctx, scope.OwnerID, link.ChildID)
	if err != nil || indexed != link {
		return engine.InspectionChild{}, errors.New("native child causal index does not match its original link")
	}
	return engine.InspectionChild{ParentID: link.ParentID, ChildID: link.ChildID, Link: engine.InspectionRecordReference{SessionID: link.Link.SessionID, RecordID: link.Link.RecordID, Sequence: link.Link.Sequence}}, nil
}
func (i *RepositoryInspector) Observe(ctx context.Context, scope engine.InspectionScope, after uint64, sink func(engine.InspectionPage) error) error {
	if sink == nil {
		return errors.New("inspection observer requires a backpressured sink")
	}
	timer := time.NewTicker(150 * time.Millisecond)
	defer timer.Stop()
	for {
		if err := ctx.Err(); err != nil {
			return err
		}
		page, err := i.Page(ctx, scope, engine.InspectionQuery{After: after, Limit: 20})
		if err != nil {
			return err
		}
		if len(page.Entries) != 0 {
			if err := ctx.Err(); err != nil {
				return err
			}
			if page.Next <= after {
				return errors.New("native inspection cursor did not advance")
			}
			if err := sink(page); err != nil {
				return err
			}
			after = page.Next
		}
		if page.HasMore {
			continue
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-timer.C:
		}
	}
}
func (i *RepositoryInspector) Export(ctx context.Context, scope engine.InspectionScope, highWater uint64, children bool) (engine.InspectionBundle, error) {
	data, summary, err := i.repository.ExportBundle(ctx, scope.OwnerID, scope.NativeID, storage.BundleOptions{HighWater: highWater, IncludeChildren: children})
	if err != nil {
		return engine.InspectionBundle{}, err
	}
	return engine.InspectionBundle{Data: data, RootID: summary.RootID, Sessions: summary.Sessions, Records: summary.Records, SHA256: summary.SHA256, HighWaters: summary.HighWaters}, nil
}

var _ engine.Inspector = (*RepositoryInspector)(nil)

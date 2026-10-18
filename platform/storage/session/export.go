// INPUT: Explicit trusted owner/root, fixed per-session high-water and verified typed child relations
// OUTPUT: ExportBundle produces bounded versioned redacted JSONL and exact scope/count/hash metadata
// POS: [platform / durable storage] Explicit export reads native facts without restoring a runtime, fetching referenced artifacts or changing originals.
package session

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

const MaxBundleBytes = 8 << 20
const MaxBundleRecords = 2048
const MaxBundleSessions = 32

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
type bundleTarget struct {
	id     string
	depth  int
	high   uint64
	parent *abi.RecordReference
}

// ExportBundle is a trusted application read, not a model/tool capability.
// The caller must bind its real product/direct owner and selected native root.
// Child opt-in follows only indexed, reverified causal links. Each participant
// has a fixed high-water; this is deliberately not a global atomic snapshot.
// Any invalid identity/body or budget failure returns no partial bundle.
func (r *Repository) ExportBundle(ctx context.Context, owner, rootID string, options BundleOptions) ([]byte, BundleSummary, error) {
	if !validIdentity(owner) || !validIdentity(rootID) {
		return nil, BundleSummary{}, ErrNotFound
	}
	var output bytes.Buffer
	line := func(value any) error {
		raw, err := json.Marshal(value)
		if err != nil {
			return err
		}
		if output.Len()+len(raw)+1 > MaxBundleBytes {
			return errors.New("Inspector bundle exceeds8MiB; no export was produced")
		}
		output.Write(raw)
		output.WriteByte('\n')
		return nil
	}
	summary := BundleSummary{RootID: rootID, HighWaters: make(map[string]uint64)}
	if err := line(map[string]any{"type": "alva.inspection.bundle", "schema_version": 1, "root_id": rootID, "include_typed_children": options.IncludeChildren, "consistency": "fixed high-water per native session, not an atomic cross-session snapshot", "processing": "Redacted export copies of committed typed records. Referenced artifacts remain references; no Agent, MCP worker, media decoder, remote source or tool was started."}); err != nil {
		return nil, BundleSummary{}, err
	}
	queue := []bundleTarget{{id: rootID, high: options.HighWater}}
	seen := make(map[string]bool)
	for len(queue) > 0 {
		if err := ctx.Err(); err != nil {
			return nil, BundleSummary{}, err
		}
		target := queue[0]
		queue = queue[1:]
		if seen[target.id] || len(seen) >= MaxBundleSessions || target.depth > 8 {
			return nil, BundleSummary{}, errors.New("Inspector bundle child cycle or32-session/8-hop budget exceeded; no export produced")
		}
		seen[target.id] = true
		probe, err := r.Page(ctx, owner, target.id, 0, 1, target.high)
		if err != nil {
			return nil, BundleSummary{}, err
		}
		high := probe.HighWater
		summary.HighWaters[target.id] = high
		summary.Sessions++
		if err := line(map[string]any{"type": "session", "native_session_id": target.id, "high_water": high, "parent_link": target.parent}); err != nil {
			return nil, BundleSummary{}, err
		}
		after := uint64(0)
		for after < high {
			page, err := r.Page(ctx, owner, target.id, after, 100, high)
			if err != nil {
				return nil, BundleSummary{}, err
			}
			if len(page.Entries) == 0 || page.Next <= after {
				return nil, BundleSummary{}, errors.New("Inspector export encountered missing committed metadata")
			}
			for _, entry := range page.Entries {
				if err := ctx.Err(); err != nil {
					return nil, BundleSummary{}, err
				}
				if summary.Records >= MaxBundleRecords {
					return nil, BundleSummary{}, errors.New("Inspector bundle exceeds2048 records; no export produced")
				}
				maximum := min(1<<20, MaxBundleBytes-output.Len())
				if maximum < 1 || entry.Value.Bytes > int64(maximum) {
					return nil, BundleSummary{}, errors.New("Inspector record exceeds export budget; original remains available through bounded reads")
				}
				_, raw, err := r.ReadArtifactFull(ctx, owner, target.id, entry.Value.URI, maximum)
				if err != nil {
					return nil, BundleSummary{}, err
				}
				var record abi.SessionRecord
				if err := json.Unmarshal(raw, &record); err != nil {
					return nil, BundleSummary{}, err
				}
				if err := record.Validate(); err != nil {
					return nil, BundleSummary{}, err
				}
				if record.ID != entry.ID || record.Sequence != entry.Sequence || record.Kind != entry.Kind || record.Namespace != entry.Namespace || record.Type != entry.Type || record.SchemaVersion != entry.SchemaVersion {
					return nil, BundleSummary{}, errors.New("Inspector export record/index identity mismatch")
				}
				redacted, err := RedactJSON(raw)
				if err != nil {
					return nil, BundleSummary{}, err
				}
				hash := sha256.Sum256(redacted)
				if err := line(map[string]any{"type": "record", "native_session_id": target.id, "sequence": entry.Sequence, "source_reference": entry.Value, "export_record_sha256": hex.EncodeToString(hash[:]), "record": json.RawMessage(redacted)}); err != nil {
					return nil, BundleSummary{}, err
				}
				summary.Records++
				if options.IncludeChildren && entry.Kind == abi.RecordExtension && entry.Namespace == "alva.agent" && entry.Type == "child_session_linked" {
					link, err := r.ResolveChildLink(ctx, owner, target.id, entry.Sequence)
					if err != nil {
						return nil, BundleSummary{}, err
					}
					indexed, err := r.ParentLink(ctx, owner, link.ChildID)
					if err != nil || indexed != link {
						if err == nil {
							err = errors.New("child link is not the exact native causal index")
						}
						return nil, BundleSummary{}, err
					}
					if len(queue)+len(seen) >= MaxBundleSessions {
						return nil, BundleSummary{}, errors.New("Inspector recursive export exceeds32 participants")
					}
					parent := link.Link
					queue = append(queue, bundleTarget{id: link.ChildID, depth: target.depth + 1, parent: &parent})
				}
			}
			after = page.Next
		}
	}
	if err := ctx.Err(); err != nil {
		return nil, BundleSummary{}, err
	}
	if err := line(map[string]any{"type": "complete", "sessions": summary.Sessions, "records": summary.Records, "high_waters": summary.HighWaters}); err != nil {
		return nil, BundleSummary{}, err
	}
	summary.Bytes = output.Len()
	hash := sha256.Sum256(output.Bytes())
	summary.SHA256 = hex.EncodeToString(hash[:])
	return bytes.Clone(output.Bytes()), summary, nil
}

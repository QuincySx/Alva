// INPUT: A real product SQLite store with a running Attempt and committed tool_start entries
// OUTPUT: RecordApprovalSettled ToolDisplay lookup, redaction-safe bound and the per-attempt cap
// POS: [L7 storage test] A settled Alva approval line is bounded stored history, never an unbounded increment.
package sqlite

import (
	"context"
	"encoding/json"
	"fmt"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestRecordApprovalSettledTakesDisplayFromTheCallAndIsCapped(t *testing.T) {
	ctx := context.Background()
	s := newStore(t)
	p, err := s.CreateSession(ctx, "approval lines", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	claim, err := s.BeginTurn(ctx, p.ID, "edit a file")
	if err != nil {
		t.Fatal(err)
	}
	attempt := claim.Attempt.ID
	display := &engine.ToolDisplay{Kind: engine.ToolEdit, Path: "a.go"}
	if err = s.Record(ctx, attempt, engine.Event{Kind: engine.ToolStart, ID: "call-1", Name: "write_file", Display: display}, true); err != nil {
		t.Fatal(err)
	}
	if err = s.RecordApprovalSettled(ctx, p.ID, attempt, hub.ApprovalSettled{CallID: "call-1", Allowed: true, Effect: engine.EffectWrite}); err != nil {
		t.Fatal(err)
	}
	if err = s.RecordApprovalSettled(ctx, p.ID, attempt, hub.ApprovalSettled{CallID: "missing-call", Allowed: false}); err != nil {
		t.Fatal(err)
	}
	var lines []hub.ApprovalSettled
	entries, err := s.History(ctx, p.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if entry.Timeline != nil && entry.Timeline.Kind == hub.TimelineApprovalSettled {
			lines = append(lines, *entry.Timeline.Approval)
		}
	}
	if len(lines) != 2 || lines[0].Tool == nil || lines[0].Tool.Path != "a.go" || !lines[0].Allowed || lines[1].Tool != nil || lines[1].Allowed {
		encoded, _ := json.Marshal(lines)
		t.Fatalf("settled lines %s", encoded)
	}
	for i := 0; i < hub.MaxApprovalSettledPerAttempt+8; i++ {
		if err = s.RecordApprovalSettled(ctx, p.ID, attempt, hub.ApprovalSettled{CallID: fmt.Sprintf("c%d", i), Allowed: true}); err != nil {
			t.Fatal(err)
		}
	}
	var stored int
	if err = s.db.QueryRowContext(ctx, `SELECT count(*) FROM hub_events WHERE attempt=? AND json_extract(data,'$.timeline.kind')=?`, attempt, hub.TimelineApprovalSettled).Scan(&stored); err != nil || stored != hub.MaxApprovalSettledPerAttempt {
		t.Fatal("settled lines per attempt are unbounded", stored, err)
	}
	if err = s.RecordApprovalSettled(ctx, p.ID, "no-such-attempt", hub.ApprovalSettled{Allowed: true}); err == nil {
		t.Fatal("a line was stored for an attempt that does not exist")
	}
}

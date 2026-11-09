// INPUT: context, hub product store/routing and the owner-scoped native repository behind HubService
// OUTPUT: ConversationService: session pages, detail, handoffs, raw product history and owned native reads
// POS: [L7 Hub product] Narrow conversation entry for desktop, HTTP and CLI; entries never read the product store directly
package product

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

// ConversationService is the conversation half of the product interface:
// listing, reading and managing Hub sessions. It owns no state of its own;
// every rule stays in HubService and the product store transaction.
type ConversationService struct {
	service *HubService
}

func (c *ConversationService) Page(ctx context.Context, query string, cursor hub.SessionCursor, limit int) (hub.SessionPage, error) {
	return c.service.Store.PageSessions(ctx, query, cursor, limit)
}

// Detail reads one session. A soft-deleted session is unavailable here: its
// data stays on disk but no entry can address it any more.
func (c *ConversationService) Detail(ctx context.Context, id string) (hub.Detail, error) {
	detail, err := c.service.Store.Detail(ctx, id)
	if err == nil && detail.Session.DeletedAt != 0 {
		return hub.Detail{}, hub.ErrNotFound
	}
	return detail, err
}

// Pinned lists the pinned sessions (at most hub.MaxPinnedSessions).
func (c *ConversationService) Pinned(ctx context.Context) ([]hub.Session, error) {
	return c.service.Store.PinnedSessions(ctx)
}

// Rename validates and stores a new title. It never changes ordering.
func (c *ConversationService) Rename(ctx context.Context, id, title string) error {
	title, err := RenameTitle(title)
	if err != nil {
		return err
	}
	return c.service.Store.RenameSession(ctx, id, title)
}

// SetPinned pins or unpins a session in the sidebar's pinned group.
func (c *ConversationService) SetPinned(ctx context.Context, id string, pinned bool) error {
	return c.service.Store.PinSession(ctx, id, pinned)
}

// Delete soft-deletes an idle session. A running or waiting session, or one
// whose execution owner has not finished cleanup, is refused with
// hub.ErrSessionRunning. Data stays on disk.
func (c *ConversationService) Delete(ctx context.Context, id string) error {
	if _, active := c.service.Runtime.ActiveExecution(id); active {
		return hub.ErrSessionRunning
	}
	return c.service.Store.DeleteSession(ctx, id)
}

func (c *ConversationService) Handoff(ctx context.Context, id, handoffID string) (*hub.Handoff, error) {
	return c.service.Store.ReadHandoff(ctx, id, handoffID)
}

// History and HistoryBefore are the bounded raw product-event pages HTTP and
// CLI expose; the Message Stream itself reads through Transcripts.
func (c *ConversationService) History(ctx context.Context, id string, after uint64, limit int) ([]hub.HistoryEntry, error) {
	return c.service.Store.History(ctx, id, after, limit)
}

func (c *ConversationService) HistoryBefore(ctx context.Context, id string, before uint64, limit int) ([]hub.HistoryEntry, error) {
	return c.service.Store.HistoryBefore(ctx, id, before, limit)
}

// ReadArtifact reads one bounded artifact page from a saved branch's native
// session. The branch must belong to the session; no host path is accepted.
func (c *ConversationService) ReadArtifact(ctx context.Context, id, branchID, uri string, offset int64) (storage.ArtifactPage, error) {
	detail, err := c.service.Store.Detail(ctx, id)
	if err != nil {
		return storage.ArtifactPage{}, err
	}
	for _, branch := range detail.Branches {
		if branch.ID == branchID && branch.NativeSessionID != "" {
			return c.service.Repository.ReadArtifact(ctx, id, branch.NativeSessionID, uri, offset, 4096)
		}
	}
	return storage.ArtifactPage{}, hub.ErrNotFound
}

// ReadNativeRecord reads exactly the committed native record a product event
// references, after checking that a saved branch of this session owns that
// native session. The repository is owner-scoped, so a native identity of an
// external engine is never found under this session.
func (c *ConversationService) ReadNativeRecord(ctx context.Context, id string, reference engine.Reference, maxBytes int64) (abi.SessionRecord, error) {
	var record abi.SessionRecord
	if reference.SessionID == "" || reference.Sequence == 0 || maxBytes <= 0 {
		return record, errors.New("exact native record reference required")
	}
	detail, err := c.service.Store.Detail(ctx, id)
	if err != nil {
		return record, err
	}
	owned := false
	for _, branch := range detail.Branches {
		owned = owned || branch.NativeSessionID == reference.SessionID
	}
	if !owned {
		return record, errors.New("native record is not owned by a saved product branch")
	}
	page, err := c.service.Repository.Page(ctx, id, reference.SessionID, reference.Sequence-1, 1, reference.Sequence)
	if err != nil {
		return record, err
	}
	if len(page.Entries) != 1 || page.Entries[0].ID != reference.RecordID || page.Entries[0].Value.Bytes > maxBytes {
		return record, errors.New("exact native record metadata missing or oversized")
	}
	_, data, err := c.service.Repository.ReadArtifactFull(ctx, id, reference.SessionID, page.Entries[0].Value.URI, int(maxBytes))
	if err != nil {
		return record, err
	}
	if json.Unmarshal(data, &record) != nil || record.ID != reference.RecordID {
		return abi.SessionRecord{}, errors.New("qualified native record does not match its reference")
	}
	return record, nil
}

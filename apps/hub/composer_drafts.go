// INPUT: context, bounded UTF-8 composer values and exact selected reference receipts.
// OUTPUT: ComposerDraftStore, logical draft scopes and revisioned copy/validation policy.
// POS: [L7 Hub product] Keeps manual input durable independently from execution admission or window identity.
package hub

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"strings"
	"unicode/utf8"
)

var ErrComposerDraftConflict = errors.New("composer draft changed; reload the shared draft before saving")

// ComposerDraftScope identifies a logical composer, never a window. Home IDs
// are explicit stable product identities; session IDs are Hub IDs.
type ComposerDraftScope struct {
	Kind string `json:"kind"`
	ID   string `json:"id"`
}

func (s ComposerDraftScope) Validate() error {
	if s.Kind != "home" && s.Kind != "session" || !composerDraftText(s.ID, 256, false) {
		return errors.New("composer draft needs an explicit home or session scope")
	}
	return nil
}

// ComposerDraftReference selects an already stored original by its immutable
// receipt. The metadata and original remain owned by the file/reference module.
type ComposerDraftReference struct {
	Kind   string `json:"kind"`
	ID     string `json:"id"`
	SHA256 string `json:"sha256"`
}

type ComposerDraft struct {
	Scope            ComposerDraftScope       `json:"scope"`
	Revision         uint64                   `json:"revision"`
	Text             string                   `json:"text"`
	PendingSessionID string                   `json:"pending_session_id,omitempty"`
	References       []ComposerDraftReference `json:"references,omitempty"`
	PreparedCommands json.RawMessage          `json:"prepared_commands,omitempty"`
}

// Empty drafts retain their revision. Deleting a row or restarting revision
// numbering would let an old Send acknowledgement erase a newer edit.
type ComposerDraftStore interface {
	ComposerDraft(context.Context, ComposerDraftScope) (ComposerDraft, error)
	SaveComposerDraft(context.Context, ComposerDraft, uint64) (ComposerDraft, error)
}

// ComposerDraftAdmissionStore binds the first explicit Send's product session
// in the same transaction as the home draft. It does not admit or replay a Turn.
type ComposerDraftAdmissionStore interface {
	CreateComposerSession(context.Context, ComposerDraft, string, string, Target) (Session, ComposerDraft, error)
}

func (d ComposerDraft) Validate() error {
	if err := d.Scope.Validate(); err != nil {
		return err
	}
	if d.Revision >= 1<<63 || !composerDraftText(d.Text, 64<<10, true) || len(d.References) > 8 || len(d.PreparedCommands) > 32<<10 || d.PendingSessionID != "" && (!composerDraftText(d.PendingSessionID, 256, false) || d.Scope.Kind != "home") {
		return errors.New("composer draft exceeds its text, preparation, reference or revision budget")
	}
	seen := make(map[string]bool, len(d.References))
	for _, ref := range d.References {
		key := ref.Kind + "\x00" + ref.ID
		if ref.Kind != "file" && ref.Kind != "attachment" && ref.Kind != "workspace" || !composerDraftText(ref.ID, 256, false) || !composerDraftSHA(ref.SHA256) || seen[key] {
			return errors.New("invalid or duplicate composer reference receipt")
		}
		seen[key] = true
	}
	if len(d.PreparedCommands) != 0 {
		var commands []json.RawMessage
		if !utf8.Valid(d.PreparedCommands) || json.Unmarshal(d.PreparedCommands, &commands) != nil || len(commands) > 8 || strings.TrimSpace(string(d.PreparedCommands)) == "null" {
			return errors.New("composer preparations need a bounded JSON array")
		}
	}
	return nil
}

func CloneComposerDraft(d ComposerDraft) ComposerDraft {
	d.References = append([]ComposerDraftReference(nil), d.References...)
	d.PreparedCommands = append(json.RawMessage(nil), d.PreparedCommands...)
	return d
}

func composerDraftText(value string, max int, empty bool) bool {
	return (empty || strings.TrimSpace(value) != "") && len(value) <= max && utf8.ValidString(value) && !strings.ContainsRune(value, 0)
}

func composerDraftSHA(value string) bool {
	if len(value) != 64 || value != strings.ToLower(value) {
		return false
	}
	_, err := hex.DecodeString(value)
	return err == nil
}

// INPUT: Optional CompanionHistoryStore pages and the existing companion identity/binding authority.
// OUTPUT: Validated deleted-history discovery, complete keyset browsing and current tombstone read-only checks.
// POS: [L7 Hub product] Browsing never opens a conversation, grants an account route or claims an execution.
package product

import (
	"context"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func (s *CompanionService) Archive(ctx context.Context, owner string) (hub.CompanionArchive, error) {
	ctx, done, err := s.operation(ctx)
	if err != nil {
		return hub.CompanionArchive{}, err
	}
	defer done()
	store, ok := s.store.(hub.CompanionHistoryStore)
	if !ok {
		return hub.CompanionArchive{}, engine.ErrUnsupported
	}
	out, err := store.CompanionArchive(ctx, owner)
	if err != nil {
		return hub.CompanionArchive{}, companionError(err)
	}
	if len(out.Profiles) > 256 || len(out.Groups) > 256 {
		return hub.CompanionArchive{}, companionReceiptError()
	}
	out.Profiles = append([]hub.CompanionProfile(nil), out.Profiles...)
	out.Groups = append([]hub.CompanionGroup(nil), out.Groups...)
	seen := map[string]bool{}
	for i, p := range out.Profiles {
		if !validCompanion(p, owner, "") || !p.Deleted || seen[p.ID] {
			return hub.CompanionArchive{}, companionReceiptError()
		}
		seen[p.ID] = true
		out.Profiles[i] = cloneCompanionProfile(p)
	}
	for i, g := range out.Groups {
		if !validCompanionGroup(g, owner, "") || !g.Deleted || seen[g.ID] {
			return hub.CompanionArchive{}, companionReceiptError()
		}
		seen[g.ID] = true
		out.Groups[i] = cloneCompanionGroup(g)
	}
	return out, nil
}
func (s *CompanionService) historyScope(ctx context.Context, owner, id string, scope uint64, query hub.CompanionHistoryQuery, maximum int) (uint64, error) {
	if query.Limit < 1 || query.Limit > maximum || len(query.Before) > 2048 {
		return 0, errors.New("invalid companion history page")
	}
	g, err := s.readGroup(ctx, owner, id)
	if err != nil {
		return 0, err
	}
	if scope == 0 {
		scope = g.ScopeRevision
	}
	if scope > g.ScopeRevision {
		return 0, hub.ErrNotFound
	}
	return scope, nil
}
func validCompanionPage(count, limit int, more bool, next, before string) bool {
	return count <= limit && more == (next != "") && (!more || count == limit && next != before && len(next) <= 2048)
}
func (s *CompanionService) MessagePage(ctx context.Context, owner, id string, scope uint64, query hub.CompanionHistoryQuery) (hub.CompanionMessagePage, error) {
	ctx, done, err := s.operation(ctx)
	if err != nil {
		return hub.CompanionMessagePage{}, err
	}
	defer done()
	store, ok := s.store.(hub.CompanionHistoryStore)
	if !ok {
		return hub.CompanionMessagePage{}, engine.ErrUnsupported
	}
	scope, err = s.historyScope(ctx, owner, id, scope, query, 256)
	if err != nil {
		return hub.CompanionMessagePage{}, err
	}
	out, err := store.CompanionMessagePage(ctx, owner, id, scope, query)
	if err != nil {
		return hub.CompanionMessagePage{}, companionError(err)
	}
	if !validCompanionPage(len(out.Messages), query.Limit, out.HasMore, out.NextBefore, query.Before) {
		return hub.CompanionMessagePage{}, companionReceiptError()
	}
	seen := map[string]bool{}
	for _, m := range out.Messages {
		if m.ID == "" || seen[m.ID] || m.GroupID != id || m.GroupRevision != scope || m.Author == "" || m.RoundID == "" || len(m.Text) > 32<<10 {
			return hub.CompanionMessagePage{}, companionReceiptError()
		}
		seen[m.ID] = true
	}
	out.Messages = append([]hub.CompanionMessage(nil), out.Messages...)
	return out, nil
}
func (s *CompanionService) RoundPage(ctx context.Context, owner, id string, scope uint64, query hub.CompanionHistoryQuery) (hub.CompanionRoundPage, error) {
	ctx, done, err := s.operation(ctx)
	if err != nil {
		return hub.CompanionRoundPage{}, err
	}
	defer done()
	store, ok := s.store.(hub.CompanionHistoryStore)
	if !ok {
		return hub.CompanionRoundPage{}, engine.ErrUnsupported
	}
	scope, err = s.historyScope(ctx, owner, id, scope, query, 128)
	if err != nil {
		return hub.CompanionRoundPage{}, err
	}
	out, err := store.CompanionRoundPage(ctx, owner, id, scope, query)
	if err != nil {
		return hub.CompanionRoundPage{}, companionError(err)
	}
	if !validCompanionPage(len(out.Rounds), query.Limit, out.HasMore, out.NextBefore, query.Before) {
		return hub.CompanionRoundPage{}, companionReceiptError()
	}
	out.Rounds = append([]hub.CompanionRound(nil), out.Rounds...)
	seen := map[string]bool{}
	for i, r := range out.Rounds {
		if r.ID == "" || seen[r.ID] || r.OwnerID != owner || r.GroupID != id || r.GroupRevision != scope || r.CreatedAt <= 0 || len(r.Members) > 16 {
			return hub.CompanionRoundPage{}, companionReceiptError()
		}
		seen[r.ID] = true
		out.Rounds[i].Members = append([]hub.CompanionConversation(nil), r.Members...)
		for j, c := range out.Rounds[i].Members {
			if c.OwnerID != owner || c.GroupID != id || c.GroupRevision != scope || c.SessionID == "" || !validCompanion(c.Profile, owner, c.CompanionID) {
				return hub.CompanionRoundPage{}, companionReceiptError()
			}
			out.Rounds[i].Members[j].Profile = cloneCompanionProfile(c.Profile)
		}
	}
	return out, nil
}

// SessionReadOnly checks current identities rather than historical execution
// snapshots. A deleted private companion or group remains observable forever.
func (s *CompanionService) SessionReadOnly(ctx context.Context, id string) (bool, error) {
	ctx, done, err := s.operation(ctx)
	if err != nil {
		return false, err
	}
	defer done()
	c, err := s.store.CompanionSession(ctx, id)
	if err != nil {
		return false, companionError(err)
	}
	if c.SessionID != id || c.OwnerID == "" || c.CompanionID == "" {
		return false, companionReceiptError()
	}
	p, err := s.readProfile(ctx, c.OwnerID, c.CompanionID)
	if err != nil {
		return false, err
	}
	if p.Deleted {
		return true, nil
	}
	if c.GroupID != "" {
		g, err := s.readGroup(ctx, c.OwnerID, c.GroupID)
		if err != nil {
			return false, err
		}
		return g.Deleted, nil
	}
	return false, nil
}

// INPUT: Explicit companion storage responses and the exact owner/configuration/revision requested by the caller.
// OUTPUT: Validated CRUD receipts and opaque storage failures without retrying uncertain mutations.
// POS: [L7 Hub product] A no-op, foreign or malformed backend response never becomes a successful companion UI fact.
package product

import (
	"context"
	"errors"
	"reflect"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/platform/storage"
)

type companionStorageError struct{ cause error }

func (e companionStorageError) Error() string {
	return "companion storage failed; reload to inspect the committed state"
}
func (e companionStorageError) Unwrap() error { return e.cause }
func companionError(err error) error {
	if err == nil {
		return nil
	}
	for _, known := range []error{hub.ErrNotFound, hub.ErrCompanionConflict, hub.ErrCompanionDisabled, hub.ErrSessionBusy, storage.ErrClosed, context.Canceled, context.DeadlineExceeded} {
		if errors.Is(err, known) {
			return known
		}
	}
	return companionStorageError{err}
}
func companionReceiptError() error {
	return errors.New("companion storage returned an invalid receipt; reload before another change")
}

func cloneCompanionProfile(p hub.CompanionProfile) hub.CompanionProfile {
	p.Skills = append([]hub.CompanionSkill(nil), p.Skills...)
	p.Target.RequiredCapabilities = append([]string(nil), p.Target.RequiredCapabilities...)
	return p
}
func cloneCompanionGroup(g hub.CompanionGroup) hub.CompanionGroup {
	g.Members = append([]string(nil), g.Members...)
	return g
}
func validCompanion(p hub.CompanionProfile, owner, id string) bool {
	return p.Validate() == nil && p.OwnerID == owner && p.ID != "" && (id == "" || p.ID == id) && p.Revision > 0 && p.CreatedAt > 0 && p.UpdatedAt >= p.CreatedAt
}
func validCompanionGroup(g hub.CompanionGroup, owner, id string) bool {
	return g.Validate() == nil && g.OwnerID == owner && g.ID != "" && (id == "" || g.ID == id) && g.Revision > 0 && g.ScopeRevision > 0 && g.CreatedAt > 0 && g.UpdatedAt >= g.CreatedAt
}
func (s *CompanionService) readProfile(ctx context.Context, owner, id string) (hub.CompanionProfile, error) {
	p, err := s.store.Companion(ctx, owner, id)
	if err != nil {
		return hub.CompanionProfile{}, companionError(err)
	}
	if !validCompanion(p, owner, id) {
		return hub.CompanionProfile{}, companionReceiptError()
	}
	return cloneCompanionProfile(p), nil
}
func (s *CompanionService) readGroup(ctx context.Context, owner, id string) (hub.CompanionGroup, error) {
	g, err := s.store.CompanionGroup(ctx, owner, id)
	if err != nil {
		return hub.CompanionGroup{}, companionError(err)
	}
	if !validCompanionGroup(g, owner, id) {
		return hub.CompanionGroup{}, companionReceiptError()
	}
	return cloneCompanionGroup(g), nil
}
func (s *CompanionService) saveProfile(ctx context.Context, p hub.CompanionProfile, expected uint64) (hub.CompanionProfile, error) {
	p.Skills = append([]hub.CompanionSkill(nil), p.Skills...)
	p.Target.RequiredCapabilities = append([]string(nil), p.Target.RequiredCapabilities...)
	if err := p.Validate(); err != nil {
		return hub.CompanionProfile{}, err
	}
	if expected > 0 && p.ID == "" || expected == 0 && p.ID != "" {
		return hub.CompanionProfile{}, hub.ErrCompanionConflict
	}
	out, err := s.store.SaveCompanion(ctx, p, expected)
	if err != nil {
		return hub.CompanionProfile{}, companionError(err)
	}
	out.Skills = append([]hub.CompanionSkill(nil), out.Skills...)
	out.Target.RequiredCapabilities = append([]string(nil), out.Target.RequiredCapabilities...)
	if !validCompanion(out, p.OwnerID, p.ID) || out.Revision != expected+1 {
		return hub.CompanionProfile{}, companionReceiptError()
	}
	p.ID, p.Revision, p.CreatedAt, p.UpdatedAt = out.ID, out.Revision, out.CreatedAt, out.UpdatedAt
	if !reflect.DeepEqual(p, out) {
		return hub.CompanionProfile{}, companionReceiptError()
	}
	committed, err := s.readProfile(context.WithoutCancel(ctx), p.OwnerID, out.ID)
	if err != nil {
		return hub.CompanionProfile{}, err
	}
	committed.Skills = append([]hub.CompanionSkill(nil), committed.Skills...)
	committed.Target.RequiredCapabilities = append([]string(nil), committed.Target.RequiredCapabilities...)
	if !reflect.DeepEqual(committed, out) {
		return hub.CompanionProfile{}, companionReceiptError()
	}
	return out, nil
}
func (s *CompanionService) saveGroup(ctx context.Context, g hub.CompanionGroup, expected uint64) (hub.CompanionGroup, error) {
	g = cloneCompanionGroup(g)
	if err := g.Validate(); err != nil {
		return hub.CompanionGroup{}, err
	}
	scope := uint64(1)
	if expected > 0 {
		old, err := s.readGroup(ctx, g.OwnerID, g.ID)
		if err != nil {
			return hub.CompanionGroup{}, err
		}
		if old.Revision != expected {
			return hub.CompanionGroup{}, hub.ErrCompanionConflict
		}
		scope = old.ScopeRevision
		if !reflect.DeepEqual(old.Members, g.Members) || old.Workspace != g.Workspace {
			scope++
		}
	} else if g.ID != "" {
		return hub.CompanionGroup{}, hub.ErrCompanionConflict
	}
	out, err := s.store.SaveCompanionGroup(ctx, g, expected)
	if err != nil {
		return hub.CompanionGroup{}, companionError(err)
	}
	out = cloneCompanionGroup(out)
	if !validCompanionGroup(out, g.OwnerID, g.ID) || out.Revision != expected+1 || out.ScopeRevision != scope {
		return hub.CompanionGroup{}, companionReceiptError()
	}
	g.ID, g.Revision, g.ScopeRevision, g.CreatedAt, g.UpdatedAt = out.ID, out.Revision, out.ScopeRevision, out.CreatedAt, out.UpdatedAt
	if !reflect.DeepEqual(g, out) {
		return hub.CompanionGroup{}, companionReceiptError()
	}
	committed, err := s.readGroup(context.WithoutCancel(ctx), g.OwnerID, out.ID)
	if err != nil {
		return hub.CompanionGroup{}, err
	}
	if !reflect.DeepEqual(committed, out) {
		return hub.CompanionGroup{}, companionReceiptError()
	}
	return out, nil
}

func (s *CompanionService) messages(ctx context.Context, owner, id string, scope uint64, limit int) ([]hub.CompanionMessage, error) {
	if scope == 0 {
		g, err := s.readGroup(ctx, owner, id)
		if err != nil {
			return nil, err
		}
		scope = g.ScopeRevision
	}
	out, err := s.store.CompanionMessages(ctx, owner, id, scope, limit)
	if err != nil {
		return nil, companionError(err)
	}
	seen := map[string]bool{}
	if len(out) > limit {
		return nil, companionReceiptError()
	}
	for _, m := range out {
		if m.ID == "" || seen[m.ID] || m.GroupID != id || m.GroupRevision != scope || m.Author == "" || m.RoundID == "" || len(m.Text) > 32<<10 {
			return nil, companionReceiptError()
		}
		seen[m.ID] = true
	}
	return append([]hub.CompanionMessage(nil), out...), nil
}
func (s *CompanionService) rounds(ctx context.Context, owner, id string, scope uint64, limit int) ([]hub.CompanionRound, error) {
	if scope == 0 {
		g, err := s.readGroup(ctx, owner, id)
		if err != nil {
			return nil, err
		}
		scope = g.ScopeRevision
	}
	out, err := s.store.CompanionRounds(ctx, owner, id, scope, limit)
	if err != nil {
		return nil, companionError(err)
	}
	seen := map[string]bool{}
	if len(out) > limit {
		return nil, companionReceiptError()
	}
	for _, r := range out {
		if r.ID == "" || seen[r.ID] || r.OwnerID != owner || r.GroupID != id || r.GroupRevision != scope || r.CreatedAt <= 0 || len(r.Members) > 16 {
			return nil, companionReceiptError()
		}
		seen[r.ID] = true
		for _, c := range r.Members {
			if c.OwnerID != owner || c.GroupID != id || c.GroupRevision != scope || c.SessionID == "" || !validCompanion(c.Profile, owner, c.CompanionID) {
				return nil, companionReceiptError()
			}
		}
	}
	for i := range out {
		out[i].Members = append([]hub.CompanionConversation(nil), out[i].Members...)
		for j := range out[i].Members {
			out[i].Members[j].Profile = cloneCompanionProfile(out[i].Members[j].Profile)
		}
	}
	return append([]hub.CompanionRound(nil), out...), nil
}

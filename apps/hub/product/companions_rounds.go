// INPUT: Fixed round/member snapshots, revision-bound original waits and existing Runtime ownership receipts.
// OUTPUT: Explicit same-round continuation, exact-once authored publication and reviewed termination without tool replay.
// POS: [L7 Hub product] Cold/restarted rounds remain unresolved until an operator chooses continuation or original-checkpoint abandonment.
package product

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"reflect"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/harness/durable"
	"github.com/QuincySx/Alva/integration/engine"
)

type CompanionRoundReview struct {
	Round   hub.CompanionRound
	Wait    *WaitView
	Problem string
}

func (s *CompanionService) Round(ctx context.Context, owner, id string) (hub.CompanionRound, error) {
	ctx, done, err := s.operation(ctx)
	if err != nil {
		return hub.CompanionRound{}, err
	}
	defer done()
	return s.round(ctx, owner, id)
}
func (s *CompanionService) round(ctx context.Context, owner, id string) (hub.CompanionRound, error) {
	r, err := s.store.CompanionRound(ctx, owner, id)
	if err != nil {
		return hub.CompanionRound{}, companionError(err)
	}
	if r.ID != id || r.OwnerID != owner || r.Revision == 0 || r.NextMember < 0 || r.NextMember > len(r.Members) {
		return hub.CompanionRound{}, companionReceiptError()
	}
	return r, nil
}
func (s *HubService) ReviewCompanionRound(ctx context.Context, owner, id string) (CompanionRoundReview, error) {
	ctx, done, err := s.companionOperation(ctx)
	if err != nil {
		return CompanionRoundReview{}, err
	}
	defer done()
	r, err := s.Companions.round(ctx, owner, id)
	out := CompanionRoundReview{Round: r}
	if err != nil {
		return out, err
	}
	if r.NextMember >= len(r.Members) {
		return out, nil
	}
	c := r.Members[r.NextMember]
	if r.Stage == "ready" {
		return out, nil
	}
	view, err := s.Waits(ctx, c.SessionID)
	if err != nil {
		return out, err
	}
	out.Wait = &view
	if r.ActiveBranchID != "" && view.BranchID != r.ActiveBranchID {
		out.Problem = "member branch changed; review the original session before termination"
	}
	return out, nil
}
func (s *HubService) saveCompanionProgress(ctx context.Context, r hub.CompanionRound) (hub.CompanionRound, error) {
	out, err := s.Companions.store.SaveCompanionRoundProgress(ctx, r.OwnerID, r, r.Revision)
	if err != nil {
		return r, companionError(err)
	}
	want := r
	want.Revision++
	want.EndedAt = 0
	if !reflect.DeepEqual(out, want) {
		return r, companionReceiptError()
	}
	actual, err := s.Companions.round(context.WithoutCancel(ctx), r.OwnerID, r.ID)
	if err != nil {
		return r, err
	}
	if !reflect.DeepEqual(actual, want) {
		return r, companionReceiptError()
	}
	return actual, nil
}

func companionPromptHash(prompt string) string {
	h := sha256.Sum256([]byte(prompt))
	return hex.EncodeToString(h[:])
}
func (s *HubService) commitCompanionTerminal(ctx context.Context, r hub.CompanionRound, status, problem string) (hub.CompanionRound, error) {
	err := s.Companions.store.FinishCompanionRound(context.WithoutCancel(ctx), r.OwnerID, r.ID, status, problem)
	if err != nil {
		return r, companionError(err)
	}
	out, err := s.Companions.round(context.WithoutCancel(ctx), r.OwnerID, r.ID)
	if err != nil {
		return r, err
	}
	want := r
	want.Status = status
	want.Error = problem
	want.Revision++
	want.EndedAt = out.EndedAt
	if out.EndedAt <= 0 || !reflect.DeepEqual(out, want) {
		return r, companionReceiptError()
	}
	return out, nil
}
func (s *HubService) finishCompanionRound(ctx context.Context, r hub.CompanionRound, status string, cause error) (hub.CompanionRound, error) {
	problem := ""
	if cause != nil {
		problem = boundedProblem(cause)
	}
	out, err := s.commitCompanionTerminal(ctx, r, status, problem)
	return out, errors.Join(cause, err)
}

func (s *HubService) waitCompanionExecution(ctx context.Context, sid, execution string, done <-chan error) error {
	select {
	case err := <-done:
		if ctx.Err() != nil {
			return errors.Join(ctx.Err(), err)
		}
		return err
	case <-ctx.Done():
		var cancelErr error
		if execution != "" {
			cancelErr = s.Runtime.CancelExecution(context.WithoutCancel(ctx), sid, execution)
			if errors.Is(cancelErr, hub.ErrSessionNotRunning) {
				cancelErr = nil
			}
		}
		return errors.Join(ctx.Err(), cancelErr, <-done)
	}
}
func (s *HubService) groupContext(ctx context.Context, r hub.CompanionRound) (string, error) {
	messages, err := s.Companions.messages(ctx, r.OwnerID, r.GroupID, r.GroupRevision, 32)
	if err != nil {
		return "", err
	}
	shared := "[Bounded group messages only. No private conversation or private memory is shared.]\n"
	remaining := 24 << 10
	parts := []string{}
	omitted := 0
	for i := len(messages) - 1; i >= 0; i-- {
		part := messages[i].Author + ": " + messages[i].Text + "\n"
		if len(part) > remaining {
			omitted++
			continue
		}
		parts = append(parts, part)
		remaining -= len(part)
	}
	if omitted > 0 {
		shared += fmt.Sprintf("[%d saved messages omitted from this bounded selection; their originals remain in group history.]\n", omitted)
	}
	for i := len(parts) - 1; i >= 0; i-- {
		shared += parts[i]
	}
	return shared + "\nUser request: " + r.Prompt, nil
}
func (s *HubService) executeCompanionRound(ctx context.Context, r hub.CompanionRound, resume *WaitTarget) (hub.CompanionRound, error) {
	for r.NextMember < len(r.Members) {
		c := r.Members[r.NextMember]
		if err := ctx.Err(); err != nil {
			return s.finishCompanionRound(ctx, r, "cancelled", err)
		}
		if r.Stage == "admitting" || r.Stage == "executing" || r.Stage == "waiting" || r.Stage == "publishing" || r.Stage == "unknown" {
			detail, err := s.Store.Detail(ctx, c.SessionID)
			if err != nil {
				return r, err
			}
			if s.Runtime.SessionActive(c.SessionID) {
				return r, errors.New("the original member execution is still active")
			}
			if detail.Session.ActiveBranchID != r.ActiveBranchID {
				return r, errors.New("original member branch changed; review and terminate this round")
			}
			turn, err := s.Store.BranchTurn(ctx, c.SessionID, r.ActiveBranchID)
			if r.Stage == "admitting" && r.ActiveTurnID == "" {
				if errors.Is(err, hub.ErrNotFound) && r.BeforeTurnID == "" || err == nil && turn.ID == r.BeforeTurnID {
					r.Stage = "ready"
					r.ActiveBranchID = ""
					r.BeforeTurnID = ""
					r.PendingPromptSHA256 = ""
					r.Status = "running"
					r, err = s.saveCompanionProgress(ctx, r)
					if err != nil {
						return r, err
					}
					continue
				}
				if err != nil {
					return r, err
				}
				if companionPromptHash(turn.Prompt) != r.PendingPromptSHA256 {
					return r, errors.New("admission outcome is unknown; inspect the original session and explicitly terminate without replay")
				}
				r.ActiveTurnID = turn.ID
				r.Stage = "executing"
				r, err = s.saveCompanionProgress(ctx, r)
				if err != nil {
					return r, err
				}
			}
			if err != nil {
				return r, err
			}
			if turn.ID != r.ActiveTurnID {
				return r, errors.New("member Turn changed; no automatic replay or continuation is permitted")
			}
			switch turn.Status {
			case engine.Waiting:
				if resume == nil {
					return r, errors.New("resolve and review the original member checkpoint, then explicitly continue this group round")
				}
				if resume.BranchID != r.ActiveBranchID || resume.TurnID != r.ActiveTurnID {
					return r, hub.ErrCompanionConflict
				}
				r.Status = "running"
				r.Stage = "executing"
				r, err = s.saveCompanionProgress(ctx, r)
				if err != nil {
					return r, err
				}
				_, execution, done, err := s.resumeWaitOwned(ctx, c.SessionID, *resume, ctx)
				resume = nil
				if err != nil {
					r.Status = "waiting"
					r.Stage = "waiting"
					saved, saveErr := s.saveCompanionProgress(context.WithoutCancel(ctx), r)
					return saved, errors.Join(err, saveErr)
				}
				err = s.waitCompanionExecution(ctx, c.SessionID, execution, done)
				if err != nil {
					if ctx.Err() != nil {
						return s.finishCompanionRound(ctx, r, "cancelled", err)
					}
					r.Status = "unknown"
					r.Stage = "unknown"
					saved, saveErr := s.saveCompanionProgress(context.WithoutCancel(ctx), r)
					return saved, errors.Join(err, saveErr)
				}
				continue
			case engine.Succeeded:
				r.Status = "running"
				r.Stage = "publishing"
				r, err = s.saveCompanionProgress(ctx, r)
				if err != nil {
					return r, err
				}
				text, err := s.companionTurnText(ctx, c.SessionID, r.ActiveTurnID)
				if err != nil {
					return r, err
				}
				m := hub.CompanionMessage{ID: r.ID + "-" + c.CompanionID, RoundID: r.ID, GroupID: r.GroupID, GroupRevision: r.GroupRevision, CompanionID: c.CompanionID, Author: c.Profile.Name, Text: text, SessionID: c.SessionID, TurnID: r.ActiveTurnID, Status: "completed", CreatedAt: r.CreatedAt}
				out, err := s.Companions.store.PublishCompanionReply(ctx, r.OwnerID, r.ID, r.Revision, m)
				if err != nil {
					return r, companionError(err)
				}
				want := r
				want.NextMember++
				want.Revision++
				want.Stage = "ready"
				want.ActiveTurnID = ""
				want.ActiveBranchID = ""
				want.BeforeTurnID = ""
				want.PendingPromptSHA256 = ""
				if !reflect.DeepEqual(out, want) {
					return r, companionReceiptError()
				}
				actual, e := s.Companions.round(context.WithoutCancel(ctx), r.OwnerID, r.ID)
				if e != nil {
					return r, e
				}
				if !reflect.DeepEqual(actual, want) {
					return r, companionReceiptError()
				}
				r = actual
				continue
			default:
				return r, fmt.Errorf("original member ended %s; review effects and explicitly terminate this round; it will not be replayed", turn.Status)
			}
		}
		if r.Stage != "ready" {
			return r, errors.New("unknown group progress requires review")
		}
		shared, err := s.groupContext(ctx, r)
		if err != nil {
			return r, err
		}
		s.mu.Lock()
		prepared, err := s.companionPromptLocked(ctx, c, shared)
		if err != nil {
			s.mu.Unlock()
			return r, err
		}
		detail, err := s.Store.Detail(ctx, c.SessionID)
		if err != nil {
			s.mu.Unlock()
			return r, err
		}
		before, err := s.Store.BranchTurn(ctx, c.SessionID, detail.Session.ActiveBranchID)
		if err != nil && !errors.Is(err, hub.ErrNotFound) {
			s.mu.Unlock()
			return r, err
		}
		r.Status = "running"
		r.Stage = "admitting"
		r.ActiveBranchID = detail.Session.ActiveBranchID
		r.BeforeTurnID = before.ID
		r.PendingPromptSHA256 = companionPromptHash(prepared)
		r, err = s.saveCompanionProgress(ctx, r)
		if err != nil {
			s.mu.Unlock()
			return r, err
		}
		turn, execution, done, err := s.Runtime.StartOnBranchOwned(ctx, c.SessionID, r.ActiveBranchID, prepared)
		s.mu.Unlock()
		if err != nil {
			return r, err
		}
		r.ActiveTurnID = turn
		r.Stage = "executing"
		saved, saveErr := s.saveCompanionProgress(context.WithoutCancel(ctx), r)
		if saveErr == nil {
			r = saved
		}
		runErr := s.waitCompanionExecution(ctx, c.SessionID, execution, done)
		if saveErr != nil {
			return r, errors.Join(saveErr, runErr)
		}
		if runErr != nil {
			if ctx.Err() != nil {
				return s.finishCompanionRound(ctx, r, "cancelled", runErr)
			}
			r.Status = "unknown"
			r.Error = boundedProblem(runErr)
			r.Stage = "unknown"
			saved, saveErr = s.saveCompanionProgress(context.WithoutCancel(ctx), r)
			return saved, errors.Join(runErr, saveErr)
		}
		latest, err := s.Store.BranchTurn(context.WithoutCancel(ctx), c.SessionID, r.ActiveBranchID)
		if err != nil {
			return r, err
		}
		if latest.Status == engine.Waiting {
			r.Status = "waiting"
			r.Stage = "waiting"
			return s.saveCompanionProgress(context.WithoutCancel(ctx), r)
		}
	}
	return s.finishCompanionRound(ctx, r, "completed", nil)
}
func (s *HubService) ContinueCompanionGroup(ctx context.Context, owner, id string, expected uint64, target *WaitTarget, approved bool) (hub.CompanionRound, error) {
	ctx, done, err := s.companionOperation(ctx)
	if err != nil {
		return hub.CompanionRound{}, err
	}
	defer done()
	r, err := s.Companions.round(ctx, owner, id)
	if err != nil {
		return r, err
	}
	ctx, groupDone, err := s.companionOperation(ctx, owner+"\x00"+r.GroupID)
	if err != nil {
		return r, err
	}
	defer groupDone()
	if r.Revision != expected {
		return r, hub.ErrCompanionConflict
	}
	if r.Status == "completed" || r.Status == "cancelled" || r.Status == "failed" {
		return r, errors.New("group round is already terminal")
	}
	g, err := s.Companions.readGroup(ctx, owner, r.GroupID)
	if err != nil {
		return r, err
	}
	if g.ScopeRevision != r.GroupRevision {
		return r, hub.ErrCompanionConflict
	}
	for _, c := range r.Members {
		if err = s.validateCompanionRoute(c.Profile.Target, approved); err != nil {
			return r, err
		}
	}
	r.OperationID = s.Companions.operationIdentity(ctx)
	r, err = s.saveCompanionProgress(ctx, r)
	if err != nil {
		return r, err
	}
	return s.executeCompanionRound(ctx, r, target)
}
func (s *HubService) TerminateCompanionGroup(ctx context.Context, owner, id string, expected uint64, confirmed bool) (hub.CompanionRound, error) {
	ctx, done, err := s.companionOperation(ctx)
	if err != nil {
		return hub.CompanionRound{}, err
	}
	defer done()
	r, err := s.Companions.round(ctx, owner, id)
	if err != nil {
		return r, err
	}
	ctx, groupDone, err := s.companionOperation(ctx, owner+"\x00"+r.GroupID)
	if err != nil {
		return r, err
	}
	defer groupDone()
	if !confirmed || r.Revision != expected {
		return r, hub.ErrCompanionConflict
	}
	for _, c := range r.Members {
		if s.Runtime.SessionActive(c.SessionID) {
			return r, errors.New("stop and join the actual member execution before terminating the round")
		}
	}
	if r.NextMember < len(r.Members) && r.Stage != "ready" {
		c := r.Members[r.NextMember]
		view, err := s.Waits(ctx, c.SessionID)
		if err != nil {
			return r, err
		}
		if view.Snapshot.State.Status != durable.Idle {
			return r, errors.New("review and abandon the original member checkpoint in Waiting tasks before terminating this group round")
		}
	}
	status := "cancelled"
	if r.NextMember == len(r.Members) {
		status = "completed"
	}
	return s.commitCompanionTerminal(ctx, r, status, "explicit termination; prior effects were not undone and unknown outcomes were not replayed")
}

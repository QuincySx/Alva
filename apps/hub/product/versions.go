// INPUT: context, errors, sort, hub VersionStore and Runtime, engine capabilities
// OUTPUT: TurnService.Edit/Regenerate/SelectVersion, ConversationService.Versions/Fork/VersionPolicy and the VersionSwitch/ForkResult values the interfaces render
// POS: [L7 Hub product] Version lines (edit resend, regenerate, switch while idle) and forks into a new conversation; the rules live in the admission transaction, never in an interface
package product

import (
	"context"
	"errors"
	"sort"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

// VersionChoice is one version at a fork position: the Turn it shows there
// and the line to switch to.
type VersionChoice struct {
	TurnID string
	LineID string
	Active bool
}

// VersionSwitch is the "‹2/3›" control of one user block: Index is the shown
// version (1-based) of Count, in the order the versions were made.
type VersionSwitch struct {
	TurnID  string
	Index   int
	Count   int
	Choices []VersionChoice
}

// Edit resends the user's Turn with new text as a new version: history before
// the Turn is kept, the old version stays reachable through its switcher.
// Only an idle conversation can do it, and only a Turn without files or Skills.
func (t *TurnService) Edit(ctx context.Context, id, turnID, text string) (TurnStart, error) {
	if err := hub.ValidTurnText(text); err != nil {
		return TurnStart{}, err
	}
	if _, active := t.service.Runtime.ActiveExecution(id); active {
		return TurnStart{}, hub.ErrVersionBusy
	}
	versionable, err := t.service.Store.TurnVersionable(ctx, id, turnID)
	if err != nil {
		return TurnStart{}, err
	}
	if versionable.Reason != "" {
		return TurnStart{}, errors.Join(hub.ErrVersionUnsupported, errors.New(versionable.Reason))
	}
	return t.service.admitWith(ctx, context.Background(), id, text, nil, nil, nil, text, "", &hub.LineFork{TurnID: turnID}, "")
}

// Regenerate asks the model again for the conversation's last reply, with the
// same user message, as a new version.
func (t *TurnService) Regenerate(ctx context.Context, id, turnID string) (TurnStart, error) {
	if _, active := t.service.Runtime.ActiveExecution(id); active {
		return TurnStart{}, hub.ErrVersionBusy
	}
	versionable, err := t.service.Store.TurnVersionable(ctx, id, turnID)
	if err != nil {
		return TurnStart{}, err
	}
	if versionable.Reason != "" {
		return TurnStart{}, errors.Join(hub.ErrVersionUnsupported, errors.New(versionable.Reason))
	}
	userText := versionable.UserText
	if userText == "" {
		userText = versionable.Turn.Prompt
	}
	return t.service.admitWith(ctx, context.Background(), id, versionable.Turn.Prompt, nil, nil, nil, userText, "", &hub.LineFork{TurnID: turnID, Regenerate: true}, "")
}

// SelectVersion shows another version of an idle conversation. Nothing is
// deleted: switching back shows the other version again.
func (t *TurnService) SelectVersion(ctx context.Context, id, lineID string) error {
	if _, active := t.service.Runtime.ActiveExecution(id); active {
		return hub.ErrVersionBusy
	}
	return t.service.Store.SelectLine(ctx, id, lineID)
}

// VersionPolicy says why a user Turn cannot be edited, regenerated or forked
// ("" when it can), for a disabled button's explanation.
func (c *ConversationService) VersionPolicy(ctx context.Context, id, turnID string) (string, error) {
	versionable, err := c.service.Store.TurnVersionable(ctx, id, turnID)
	if err != nil {
		return "", err
	}
	return versionable.Reason, nil
}

// VersionsDisabled says why the whole conversation has no versions or forks
// ("" when it can have them), so the interface disables the actions once.
func (c *ConversationService) VersionsDisabled(ctx context.Context, id string) (string, error) {
	return c.service.Store.VersionsDisabled(ctx, id)
}

// Versions lists the switchers of the conversation's current version, keyed by
// the user Turn each one belongs to. A conversation without versions has none.
func (c *ConversationService) Versions(ctx context.Context, id string) (map[string]VersionSwitch, error) {
	detail, err := c.service.Store.Detail(ctx, id)
	if err != nil {
		return nil, err
	}
	return c.VersionsOf(ctx, id, detail.Session.ActiveLineID)
}

// VersionsOf is Versions for a caller that already holds the conversation's
// active line (Session.ActiveLineID): it reads only the lines.
func (c *ConversationService) VersionsOf(ctx context.Context, id, activeLineID string) (map[string]VersionSwitch, error) {
	if activeLineID == "" {
		return nil, nil
	}
	lines, err := c.service.Store.Lines(ctx, id)
	if err != nil {
		return nil, err
	}
	return versionSwitches(lines, activeLineID), nil
}

// versionSwitches derives the switchers of the active line from the lines'
// fork positions. A version is a distinct Turn at a position; the line that
// shows it is the active one when it does, else the most recently used.
func versionSwitches(lines []hub.Line, active string) map[string]VersionSwitch {
	var current *hub.Line
	for i := range lines {
		if lines[i].ID == active {
			current = &lines[i]
		}
	}
	if current == nil {
		return nil
	}
	result := map[string]VersionSwitch{}
	for _, position := range current.Groups {
		type version struct {
			turn, line string
			seq        uint64
			touched    int64
		}
		byTurn := map[string]version{}
		for _, line := range lines {
			for _, g := range line.Groups {
				if g.Group != position.Group {
					continue
				}
				candidate := version{turn: g.TurnID, line: line.ID, seq: g.Seq, touched: line.TouchedAt}
				known, seen := byTurn[g.TurnID]
				switch {
				case !seen, line.ID == active, known.line != active && candidate.touched > known.touched:
					byTurn[g.TurnID] = candidate
				}
			}
		}
		if len(byTurn) < 2 {
			continue
		}
		versions := make([]version, 0, len(byTurn))
		for _, v := range byTurn {
			versions = append(versions, v)
		}
		sort.Slice(versions, func(i, j int) bool { return versions[i].seq < versions[j].seq })
		view := VersionSwitch{TurnID: position.TurnID, Count: len(versions)}
		for i, v := range versions {
			choice := VersionChoice{TurnID: v.turn, LineID: v.line, Active: v.turn == position.TurnID}
			if choice.Active {
				view.Index = i + 1
				choice.LineID = active
			}
			view.Choices = append(view.Choices, choice)
		}
		result[position.TurnID] = view
	}
	return result
}

// ForkResult is the new conversation of a fork. Degraded says its engine
// cannot continue from a native copy, so the model sees only a Turn index.
type ForkResult struct {
	Session  hub.Session
	Degraded bool
}

// Fork creates a new conversation continuing from the end of a Turn. Nothing is
// copied: the model's context comes from the engine's native fork, or from a
// bounded Turn index when the engine cannot. The new conversation records
// where it came from, for the link back.
func (c *ConversationService) Fork(ctx context.Context, id, turnID string) (ForkResult, error) {
	if _, active := c.service.Runtime.ActiveExecution(id); active {
		return ForkResult{}, hub.ErrSessionRunning
	}
	detail, err := c.service.Store.Detail(ctx, id)
	if err != nil {
		return ForkResult{}, err
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return ForkResult{}, err
	}
	native := c.service.nativeForkable(ctx, id, turnID, branch, true)
	created, err := c.service.Store.ForkSession(ctx, id, hub.SessionFork{TurnID: turnID, NativeFork: native})
	if err != nil {
		return ForkResult{}, err
	}
	return ForkResult{Session: created, Degraded: !native && branch.NativeSessionID != ""}, nil
}

// nativeForkable decides whether a fork before (or, with keepThrough, through)
// a Turn can continue the engine's own native conversation: the engine
// declares it, the cut has a reliable absolute anchor, and the engine agrees
// that cut is reachable. Anything less is a fork from the Turn index, which
// the caller reports; a wrong context is never handed to a model.
func (s *HubService) nativeForkable(ctx context.Context, id, turnID string, branch hub.Branch, keepThrough bool) bool {
	descriptor, ok := s.Runtime.Descriptor(branch.Target.EngineID)
	if !ok || descriptor.Runtime == nil || !descriptor.Runtime.Capabilities().Fork || branch.NativeSessionID == "" {
		return false
	}
	anchor, anchored, err := s.Store.ForkAnchor(ctx, id, turnID, keepThrough)
	if err != nil || !anchored {
		return false
	}
	if checker, ok := descriptor.Runtime.(engine.ForkChecker); ok {
		return checker.CheckFork(ctx, anchor.Point()) == nil
	}
	return true
}

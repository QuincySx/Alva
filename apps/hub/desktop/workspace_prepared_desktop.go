// INPUT: Window-owned typed selections, prepared reference receipts and real asynchronous admission returns.
// OUTPUT: First-Create validation, captured submissions and staged-claim cleanup after actual in-flight operations.
// POS: [L7 Hub UI] Owns composer claims until consumption/disposal, preserving saved originals and actual execution owners.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"reflect"
	"strings"
	"sync"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

// This window owns only its unsubmitted staging claims. Closing detaches the
// observer immediately, while pending file/preparation/admission calls retain
// those claims until their real return (including receipts arriving after close).
type workspaceReferenceOwner struct {
	mu      sync.Mutex
	service *product.HubService
	closed  bool
	active  int
	claims  map[product.DraftReferenceSelection]string
}

func (o *workspaceReferenceOwner) begin() func() {
	o.mu.Lock()
	defer o.mu.Unlock()
	if o.closed {
		return nil
	}
	o.active++
	return func() {
		o.mu.Lock()
		o.active--
		claims := o.releaseWhenClosedLocked()
		o.mu.Unlock()
		o.release(claims)
	}
}

func (o *workspaceReferenceOwner) retain(id string, references []product.DraftReferenceSelection) {
	o.mu.Lock()
	if o.claims == nil {
		o.claims = make(map[product.DraftReferenceSelection]string)
	}
	for _, reference := range references {
		o.claims[reference] = id
	}
	claims := o.releaseWhenClosedLocked()
	o.mu.Unlock()
	o.release(claims)
}

func (o *workspaceReferenceOwner) forget(references []product.DraftReferenceSelection) {
	o.mu.Lock()
	defer o.mu.Unlock()
	for _, reference := range references {
		delete(o.claims, reference)
	}
}

// A preparation receipt rejected before UI adoption was never available for a
// Send capture. Remove only that new claim; its saved original remains intact.
func (o *workspaceReferenceOwner) discard(id string, reference product.DraftReferenceSelection) {
	o.forget([]product.DraftReferenceSelection{reference})
	o.release(map[product.DraftReferenceSelection]string{reference: id})
}

func (o *workspaceReferenceOwner) close() {
	o.mu.Lock()
	o.closed = true
	claims := o.releaseWhenClosedLocked()
	o.mu.Unlock()
	o.release(claims)
}

func (o *workspaceReferenceOwner) releaseWhenClosedLocked() map[product.DraftReferenceSelection]string {
	if !o.closed || o.active != 0 {
		return nil
	}
	claims := o.claims
	o.claims = nil
	return claims
}

func (o *workspaceReferenceOwner) release(claims map[product.DraftReferenceSelection]string) {
	for reference, id := range claims {
		switch reference.Kind {
		case "attachment":
			o.service.RemoveDraftAttachment(id, reference.ID)
		case "workspace":
			o.service.RemoveDraftWorkspaceReference(id, reference.ID)
		}
	}
}

// These existing preparation APIs are read-only for home drafts. Reusing them
// keeps canonical workspace, Goal revision and builtin identity checks in the
// product service before a new session is created; Send still revalidates.
func validateWorkspaceHomeSelections(ctx context.Context, service *product.HubService, workspace string, target hub.Target, selections []product.ComposerPreparation) error {
	for _, selected := range selections {
		if selected.Kind == product.ComposerGoalEntry {
			if selected.GoalRef == nil {
				return errors.New("目标引用已变化，请移除后重新选择。草稿已保留。")
			}
			current, err := service.PrepareComposerGoal(ctx, "", workspace, selected.GoalRef.GoalID)
			if err != nil {
				return err
			}
			if !reflect.DeepEqual(current, selected) {
				return errors.New("目标引用已变化，请移除后重新选择。草稿已保留。")
			}
		} else if selected.SkillRef != nil && selected.SkillRef.Workspace != "" {
			current, err := service.PrepareComposerHomeSelection(ctx, workspace, product.ComposerSelectionRequest{EntryID: selected.EntryID, Arguments: selected.Arguments})
			if err != nil {
				return err
			}
			if !reflect.DeepEqual(current, selected) {
				return errors.New("项目 Skill 来源已变化，请移除后重新选择。草稿已保留。")
			}
		} else if err := service.RevalidateComposerPreparation(ctx, "", selected); err != nil {
			return err
		}
		if target.EngineID != "alva" && (selected.Kind == product.ComposerGoalEntry || selected.Intent == product.ComposerIntentPlan) {
			return errors.New("目标或计划引用需要 Alva Agent。草稿已保留。")
		}
	}
	return ctx.Err()
}

func (p *NativePage) captureWorkspaceSelections() []product.ComposerPreparation {
	return append([]product.ComposerPreparation(nil), p.composerSelections[p.selected]...)
}

func (p *NativePage) captureWorkspaceReferences() []product.DraftReferenceSelection {
	return append([]product.DraftReferenceSelection(nil), p.workspaceReferences[p.selected]...)
}

func (p *NativePage) retainWorkspaceReferences(id string, references []product.DraftReferenceSelection) {
	if len(references) == 0 {
		return
	}
	p.referenceOwner.retain(id, references)
	if p.workspaceReferences == nil {
		p.workspaceReferences = make(map[string][]product.DraftReferenceSelection)
	}
	p.workspaceReferences[id] = append(p.workspaceReferences[id], references...)
}

func (p *NativePage) consumeWorkspaceReferences(id string, references []product.DraftReferenceSelection) {
	p.referenceOwner.forget(references)
	selected := make(map[product.DraftReferenceSelection]bool, len(references))
	for _, reference := range references {
		selected[reference] = true
	}
	current := p.workspaceReferences[id][:0]
	for _, reference := range p.workspaceReferences[id] {
		if !selected[reference] {
			current = append(current, reference)
		}
	}
	if p.workspaceReferences != nil {
		p.workspaceReferences[id] = current
	}
}

func (p *NativePage) consumeWorkspaceSelections(id string, sent []product.ComposerPreparation) {
	if len(sent) == 0 {
		return
	}
	current := p.composerSelections[id]
	if len(current) >= len(sent) && reflect.DeepEqual(current[:len(sent)], sent) {
		p.composerSelections[id] = current[len(sent):]
	}
}

func (p *NativePage) buildWorkspaceSelections(c *ui.Context) {
	t := c.Theme()
	for i, selection := range p.composerSelections[p.selected] {
		label := "/" + strings.TrimPrefix(strings.TrimPrefix(selection.EntryID, "builtin:"), "skill:")
		if selection.Kind == product.ComposerGoalEntry {
			label = selection.Text
		}
		// cmp_alva_badge.Neutral remains a static label. The adjacent IconButton
		// owns removal, so the marker does not pretend to execute the command.
		ui.Row(c.Key("selection-" + selection.EntryID)).MaxWidthPercent(100).MinWidth(0).Gap(4).AlignItems(ui.Center).Children(func() {
			ui.Row(c).Label("Prepared reference badge "+selection.EntryID).MinWidth(0).Shrink(1).
				Padding(4, 8).Gap(8).Radius(99).AlignItems(ui.Center).Justify(ui.Center).
				Background(workspacePalette(c).subtle).TextColor(t.TextMuted).Tooltip(label).Children(func() {
				ui.Text(c, label).FontSize(workspaceFontCaption).FontWeight(400).LineHeight(1.5).TextColor(t.TextMuted).
					TextAlign(ui.Center).SingleLine().MinWidth(0).Shrink(1)
			})
			workspaceIconAction(c, "remove-selection-"+selection.EntryID, "Remove prepared reference "+label, "x").Shrink(0).
				Disabled(p.busy || p.inputBlocked()).Tooltip("移除草稿引用；保留文字和已保存原件").OnClick(func() {
				items := p.composerSelections[p.selected]
				p.composerSelections[p.selected] = append(items[:i], items[i+1:]...)
			})
		})
	}
}

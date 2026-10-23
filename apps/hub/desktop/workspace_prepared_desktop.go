// INPUT: Frozen typed command/Skill selections belonging to one native composer draft.
// OUTPUT: Next-submit revalidation and exact acknowledged-selection consumption.
// POS: [L7 Hub UI] Selection does not execute; Send retains changed drafts and rejects stale Skill sources.
//go:build desktop

package hubdesktop

import (
	"reflect"
	"strings"

	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

func (p *NativePage) captureWorkspaceSelections() []product.ComposerPreparation {
	return append([]product.ComposerPreparation(nil), p.composerSelections[p.selected]...)
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
	for i, selection := range p.composerSelections[p.selected] {
		label := "/" + strings.TrimPrefix(strings.TrimPrefix(selection.EntryID, "builtin:"), "skill:")
		ui.Button(c.Key("selection-"+label), label+" ×").FontSize(12).
			Disabled(p.busy || p.inputBlocked()).Tooltip("移除下一次提交的命令或 Skill 选择；草稿文字保留").
			OnClick(func() {
				items := p.composerSelections[p.selected]
				p.composerSelections[p.selected] = append(items[:i], items[i+1:]...)
			})
	}
}

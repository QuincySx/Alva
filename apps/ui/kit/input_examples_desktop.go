// INPUT: Alva Input (Kind x State), TextArea (State) and Field (State) axes.
// OUTPUT: Gallery pages for 28, 6 and 7 design variants, including the masked Password kind.
// POS: [L7 Kit] Select kind is the Select component; Focus and ErrorFocus are forced focus, Loading is Busy, ReadOnly is a Props bool.
//go:build desktop

package kit

import (
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/egoist/mygo/ui"
)

var inputStates = []string{"Default", "Focus", "Error", "Disabled", "Loading", "ErrorFocus", "ReadOnly"}

type fieldState struct {
	forced                            force.State
	invalid, disabled, busy, readOnly bool
}

func inputState(s string) fieldState {
	st := fieldState{}
	switch s {
	case "Focus":
		st.forced = force.Focus
	case "Error":
		st.invalid = true
	case "ErrorFocus":
		st.invalid, st.forced = true, force.Focus
	case "Disabled":
		st.disabled = true
	case "Loading":
		st.busy = true
	case "ReadOnly":
		st.readOnly = true
	}
	return st
}

func inputPage() Page {
	return Page{
		Name: "Input", Design: "Input", Category: "输入",
		Axes: []Axis{axis("Kind", "Text", "Search", "Select", "Password"), axis("State", inputStates...)},
		Cell: func(c *ui.Context, sel []string) {
			st := inputState(sel[1])
			force.With(st.forced, func() {
				switch sel[0] {
				case "Select":
					value := cellSlot("Input", sel, "select", "fast")
					Select(c, value, []Option[string]{{Value: "fast", Label: "快速"}, {Value: "deep", Label: "深入"}},
						SelectProps{Label: "模型", Invalid: st.invalid, Disabled: st.disabled, Busy: st.busy, ReadOnly: st.readOnly, Key: "s"})
				default:
					text := cellSlot("Input", sel, "text", "")
					p := InputProps{Label: "名称", Placeholder: "输入内容", Invalid: st.invalid, Disabled: st.disabled, Busy: st.busy, ReadOnly: st.readOnly, Width: sampleWidth, Key: "i"}
					if sel[0] == "Search" {
						p.Label, p.Placeholder, p.Leading = "搜索", "搜索", icon.Search
					}
					if st.readOnly {
						text = cellSlot("Input", sel, "readonly", "只读内容")
					}
					if sel[0] == "Password" {
						reveal := cellSlot("Input", sel, "reveal", false)
						secret := cellSlot("Input", sel, "secret", "sample-not-a-real-key")
						p.Label, p.Placeholder = "API Key", "粘贴 Key"
						Password(c, secret, reveal, p)
						return
					}
					Input(c, text, p)
				}
			})
		},
	}
}

func textAreaPage() Page {
	return Page{
		Name: "TextArea", Design: "TextArea", Category: "输入",
		Axes: []Axis{axis("State", "Default", "Focus", "Error", "Disabled", "ErrorFocus", "ReadOnly")},
		Cell: func(c *ui.Context, sel []string) {
			st := inputState(sel[0])
			value := cellSlot("TextArea", sel, "text", "")
			if st.readOnly {
				value = cellSlot("TextArea", sel, "readonly", "只读的多行内容")
			}
			force.With(st.forced, func() {
				TextArea(c, value, TextAreaProps{Label: "说明", Placeholder: "写点什么", Invalid: st.invalid, Disabled: st.disabled, ReadOnly: st.readOnly, MinLines: 2, Key: "t"}).Width(sampleWidth)
			})
		},
	}
}

func fieldPage() Page {
	return Page{
		Name: "Field", Design: "Field", Category: "输入",
		Axes: []Axis{axis("State", inputStates...)},
		Cell: func(c *ui.Context, sel []string) {
			st := inputState(sel[0])
			value := cellSlot("Field", sel, "text", "")
			if st.readOnly {
				value = cellSlot("Field", sel, "readonly", "只读内容")
			}
			p := FieldProps{Title: "名称", Hint: "显示在列表里", Disabled: st.disabled, ReadOnly: st.readOnly, Busy: st.busy, Key: "f"}
			if st.invalid {
				p.Error = "名称已存在"
			}
			force.With(st.forced, func() {
				Field(c, p, func() { Input(c, value, InputProps{Placeholder: "输入内容", Width: sampleWidth, Key: "i"}) }).Width(sampleWidth)
			})
		},
	}
}

func selectPage() Page {
	return Page{
		Name: "Select", Design: "SelectOption", Category: "输入",
		Axes: []Axis{axis("State", "Default", "Hover", "Selected", "Focus", "SelectedFocus", "Disabled", "SelectedDisabled")},
		Cell: func(c *ui.Context, sel []string) {
			state := sel[0]
			forced := force.None
			switch state {
			case "Hover":
				forced = force.Hover
			case "Focus", "SelectedFocus":
				forced = force.Focus
			}
			o := Option[string]{Value: "project", Label: "当前项目", Description: "仅此项目", Disabled: state == "Disabled" || state == "SelectedDisabled"}
			selectOptionStatic(c, o, len(state) >= 8 && state[:8] == "Selected", forced)
		},
		Live: func(c *ui.Context) {
			value := cellSlot("Select", nil, "live", "project")
			Select(c, value, []Option[string]{
				{Value: "project", Label: "当前项目", Description: "仅此项目"},
				{Value: "all", Label: "全部项目", Description: "所有工作区"},
				{Value: "locked", Label: "受限项目", Disabled: true},
			}, SelectProps{Label: "范围", Key: "live"}).Width(sampleWidth)
		},
	}
}

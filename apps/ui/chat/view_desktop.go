// INPUT:  MyGo ui/markdown and the UI-thread-confined presentation model
// OUTPUT: NativeView, NewNativeView, Build
// POS: [L6 shared UI] Native self-drawn chat surface with virtualized keyed rows and explicit actions
//go:build desktop

package chat

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
)

// NativeView holds persistent widget state, never build-pass Elements.
// Send/Stop callbacks execute on the UI thread, not inside runtime goroutines.
type NativeView struct {
	Model          *Model
	Send           func()
	Stop           func()
	Busy           bool
	LargePasteHint string
	list           ui.ListState
	markdown       *markdown.Options
}

func NewNativeView(model *Model) *NativeView {
	return &NativeView{Model: model, list: ui.ListState{FollowEnd: true}, markdown: &markdown.Options{}}
}

func (v *NativeView) Build(c *ui.Context) {
	m := v.Model
	ui.Column(c).Fill().Gap(10).Children(func() {
		ui.Row(c).Gap(8).Children(func() {
			ui.Text(c, m.StatusText()).Role(ui.RoleStatus).FontSize(13).Grow(1)
			if len(m.Messages) > 0 && !v.AtEnd() {
				ui.Button(c, "Jump to latest").OnClick(v.ScrollToEnd)
			}
		})
		if m.Error != "" {
			ui.Text(c, "Error: "+m.Error).FontSize(13)
		}
		if m.Notice != "" {
			ui.Text(c, m.Notice).FontSize(12).TextColor(c.Theme().TextMuted)
		}
		v.BuildChat(c)
	})
}

// BuildChat shares native chat controls without importing any Hub domain.
func (v *NativeView) BuildChat(c *ui.Context) {
	m := v.Model
	v.list.Key = func(i int) any { return m.Messages[i].ID }
	v.list.Label = func(i int) string { return m.Messages[i].Role }
	ui.Column(c).Grow(1).MinHeight(0).FillWidth().Gap(10).Children(func() {
		ui.List(c.Key("messages"), &v.list, len(m.Messages), func(i int) {
			row := m.Messages[i]
			ui.Column(c).Gap(6).Padding(8).Children(func() {
				label := row.Role
				if row.Role == "Alva" {
					label += " · " + string(row.Status)
				}
				ui.Text(c, label).FontSize(12).Bold().TextColor(c.Theme().TextMuted)
				if row.Role == "You" {
					ui.Text(c, row.Text)
				} else if row.Text == "" && (row.Status == Running || row.Status == Stopping) {
					ui.Text(c, "Waiting for output…").TextColor(c.Theme().TextMuted)
				} else if row.Status == Running || row.Status == Stopping {
					markdown.Stream(c, row.Text, v.markdown)
				} else {
					markdown.View(c, row.Text, v.markdown)
				}
				if row.Error != "" {
					ui.Text(c, "Run error: "+row.Error).FontSize(12)
				}
			})
		}).Label("Message history").Grow(1).Gap(8).Justify(ui.End).Children(func() {
			if len(m.Messages) == 0 {
				ui.Text(c, "Type a message below. Nothing has been sent.").TextColor(c.Theme().TextMuted).Padding(12)
			}
		})
		ui.TextArea(c.Key("composer"), &m.Draft).
			Label("Message").Placeholder("Message — Enter adds a line; use Send to run").
			Height(88).Disabled(m.ActiveRunID != "" || v.Busy).OnPaste(func(text string) bool {
			if len(text) > MaxDraftBytes {
				m.Error = "Pasted text exceeds 32 KiB. Use a durable session to attach a selected file as a reference; nothing was uploaded or sent."
				if v.LargePasteHint != "" {
					m.Error = v.LargePasteHint
				}
				return true
			}
			return false
		})
		ui.Row(c).Gap(8).Children(func() {
			ui.Text(c, fmt.Sprintf("%d / %d KiB · explicit Send only", len(m.Draft)/1024, MaxDraftBytes/1024)).FontSize(12).TextColor(c.Theme().TextMuted).Grow(1)
			ui.Button(c, "Stop").Disabled(m.ActiveRunID == "" || m.Status == Stopping || v.Busy).OnClick(func() {
				if v.Stop != nil {
					v.Stop()
				}
			})
			ui.PrimaryButton(c, "Send").Disabled(!m.CanSend() || v.Busy).OnClick(func() {
				if v.Send != nil {
					v.Send()
				}
			})
		})
	})
}

func (v *NativeView) AtEnd() bool  { return v.list.AtEnd() }
func (v *NativeView) ScrollToEnd() { v.list.ScrollToEnd() }
func (v *NativeView) SetMarkdownOptions(options *markdown.Options) {
	if options == nil {
		options = &markdown.Options{}
	}
	v.markdown = options
}

// INPUT:  MyGo ui/markdown and the UI-thread-confined presentation model
// OUTPUT: NativeView, NewNativeView, Build and independently composed BuildMessages with optional row rendering/spacing and visible-row observation
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
	// RenderMessage optionally renders a BuildMessages row. Returning false keeps
	// the shared presentation. BuildChat always uses its original presentation.
	// It may run during virtual row layout and must only build presentation.
	RenderMessage func(*ui.Context, Message) bool
	// MessageGap optionally sets BuildMessages row spacing. Zero or negative
	// keeps its default sixteen pixels; BuildChat keeps its original spacing.
	MessageGap float32
	list       ui.ListState
	markdown   *markdown.Options
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
		v.buildMessages(c, false)
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

// BuildMessages reuses the bounded, keyed transcript without prescribing a composer.
// Products keep their own input controls while sharing scrolling and Markdown.
func (v *NativeView) BuildMessages(c *ui.Context) { v.buildMessages(c, true) }

func (v *NativeView) buildMessages(c *ui.Context, conversation bool) {
	m := v.Model
	justify, gap := ui.End, float32(8)
	if conversation {
		justify, gap = ui.Start, 16
		if v.MessageGap > 0 {
			gap = v.MessageGap
		}
	}
	v.list.Key = func(i int) any { return m.Messages[i].ID }
	v.list.Label = func(i int) string { return m.Messages[i].Role }
	ui.List(c.Key("messages"), &v.list, len(m.Messages), func(i int) {
		row := m.Messages[i]
		if conversation {
			ui.Row(c).FillWidth().Justify(ui.Center).Children(func() {
				ui.Column(c).FillWidth().MaxWidth(760).Children(func() {
					if v.RenderMessage != nil && v.RenderMessage(c, row) {
						return
					}
					// The virtual list may invoke this callback during layout,
					// after buildMessages returns. Scope typography to the actual
					// Markdown construction and preserve its registered renderers.
					if v.markdown == nil {
						v.markdown = &markdown.Options{}
					}
					previous := v.markdown.Theme
					theme := markdown.Theme{}
					if previous != nil {
						theme = *previous
					}
					theme.FontSize, theme.LineHeight = c.Theme().FontSize, 1.75
					v.markdown.Theme = &theme
					defer func() { v.markdown.Theme = previous }()
					message := ui.Column(c).MinWidth(0).Gap(12)
					if row.Role == "You" {
						message.FillWidth().Padding(12).Radius(16).Background(c.Theme().SurfaceHover)
					} else {
						message.FillWidth()
					}
					message.Children(func() {
						if row.Role != "You" {
							ui.Row(c).Label("Message header " + row.ID).Height(32).AlignItems(ui.Center).Children(func() {
								ui.Text(c, row.Role).FontSize(14).FontWeight(600).LineHeight(1.5)
							})
						}
						if row.Role == "You" {
							ui.Text(c, row.Text).FontSize(c.Theme().FontSize).FontWeight(400).LineHeight(1.75)
						} else if row.Text == "" && (row.Status == Running || row.Status == Stopping) {
							ui.Text(c, "正在整理思路…").FontSize(c.Theme().FontSize).LineHeight(1.75).TextColor(c.Theme().TextMuted)
						} else if row.Status == Running || row.Status == Stopping {
							markdown.Stream(c, row.Text, v.markdown)
						} else {
							markdown.View(c, row.Text, v.markdown)
						}
						if row.Error != "" {
							ui.Text(c, row.Error).TextColor(c.Theme().Danger).FontSize(12)
						}
					})
				})
			})
			return
		}
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
	}).Label("Message history").Grow(1).Gap(gap).Justify(justify).Children(func() {
		if len(m.Messages) == 0 && !conversation {
			ui.Text(c, "Type a message below. Nothing has been sent.").TextColor(c.Theme().TextMuted).Padding(12)
		}
	})
}

func (v *NativeView) AtEnd() bool  { return v.list.AtEnd() }
func (v *NativeView) ScrollToEnd() { v.list.ScrollToEnd() }

// VisibleRows reports the first and last message rows shown in the last frame;
// last is below first when none were shown. Products page history by it.
func (v *NativeView) VisibleRows() (first, last int) { return v.list.Visible() }
func (v *NativeView) SetMarkdownOptions(options *markdown.Options) {
	if options == nil {
		options = &markdown.Options{}
	}
	v.markdown = options
}

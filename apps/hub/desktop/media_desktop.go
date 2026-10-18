// INPUT: Product-bound cold media pages, verified manifests/previews and native self-drawn controls
// OUTPUT: Explicit source/derivative/selection/receipt navigation with one bounded JPEG preview
// POS: [L7 Hub product] Cold evidence reads; only explicit local Play decodes selected derivatives without model or tool replay
//go:build desktop

package hubdesktop

import (
	"encoding/json"
	"fmt"

	mediaextension "github.com/QuincySx/Alva/extension/media"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/egoist/mygo/ui"
)

type MediaPane struct {
	Branch, Mode, SourceURI, SelectionID string
	Page                                 session.ArtifactIndexPage
	Cursor                               uint64
	Back                                 []uint64
	JSON, ManifestURI                    string
	Bitmap                               *ui.Bitmap
	PreviewBytes                         int
	Request                              uint64
	PlaybackURI                          string
	PlaybackDuration                     int64
	PlaybackKind                         string
}

func (p *NativePage) openMedia() {
	if p.selected == "" || p.paneBusy {
		return
	}
	p.setPane("media")
	p.media = MediaPane{Branch: p.detail.Session.ActiveBranchID, Mode: "sources"}
	p.loadMedia(0, 0)
}

func (p *NativePage) mediaNavigate(mode, subject string) {
	if p.paneBusy {
		return
	}
	p.stopPlayback()
	branch := p.media.Branch
	p.media = MediaPane{Branch: branch, Mode: mode}
	if mode == "artifacts" {
		p.media.SourceURI = subject
	}
	if mode == "receipts" {
		p.media.SelectionID = subject
	}
	p.loadMedia(0, 0)
}

func (p *NativePage) loadMedia(after, high uint64) {
	if p.selected == "" || p.paneBusy {
		return
	}
	p.stopPlayback()
	p.media.PlaybackURI, p.media.PlaybackDuration, p.media.PlaybackKind = "", 0, ""
	id, generation := p.selected, p.generation
	branch, mode, source, selection := p.media.Branch, p.media.Mode, p.media.SourceURI, p.media.SelectionID
	p.media.Request++
	request := p.media.Request
	p.media.JSON, p.media.ManifestURI, p.media.Bitmap, p.media.PreviewBytes = "", "", nil, 0
	p.paneBusy = true
	go func() {
		var page session.ArtifactIndexPage
		var err error
		switch mode {
		case "sources":
			page, err = p.Service.MediaSources(p.ctx, id, branch, after, high)
		case "selections":
			page, err = p.Service.MediaSelections(p.ctx, id, branch, after, high)
		case "artifacts":
			page, err = p.Service.MediaIndex(p.ctx, id, branch, source, after, high)
		case "receipts":
			page, err = p.Service.MediaReceipts(p.ctx, id, branch, selection, after, high)
		}
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation || p.pane != "media" || p.media.Branch != branch || p.media.Mode != mode || p.media.Request != request {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.problem = ""
			p.media.Page, p.media.Cursor = page, after
		})
	}()
}

// readMedia reads metadata only unless preview was explicitly requested.
// Exactly one bitmap is retained; navigating/closing releases its reference.
func (p *NativePage) readMedia(uri string, preview bool) {
	if p.selected == "" || p.paneBusy {
		return
	}
	p.stopPlayback()
	p.media.PlaybackURI, p.media.PlaybackDuration, p.media.PlaybackKind = "", 0, ""
	id, generation, branch, mode := p.selected, p.generation, p.media.Branch, p.media.Mode
	p.media.Request++
	request := p.media.Request
	p.media.JSON, p.media.ManifestURI, p.media.Bitmap, p.media.PreviewBytes = "", "", nil, 0
	p.paneBusy = true
	go func() {
		var value any
		var data []byte
		var bitmap *ui.Bitmap
		var playbackKind string
		var videoDuration int64
		var err error
		switch mode {
		case "artifacts":
			if preview {
				manifest, content, e := p.Service.MediaPreview(p.ctx, id, branch, uri)
				value, data, err = manifest, content, e
				if err == nil && manifest.Artifact.MediaType == "image/jpeg" {
					bitmap, err = ui.DecodeBitmap(data)
				}
			} else {
				value, err = p.Service.MediaManifest(p.ctx, id, branch, uri)
			}
			if manifest, ok := value.(mediaextension.Manifest); ok && err == nil {
				if manifest.Artifact.MediaType == "video/mp4" && manifest.Output.Operation.Kind == "transcode_video" {
					playbackKind = "video"
				}
				if manifest.Artifact.MediaType == "audio/wav" && manifest.Output.Operation.Kind == "transcode_audio" {
					playbackKind = "audio"
				}
				videoDuration = manifest.Output.DurationMillis
			}
		case "selections":
			value, err = p.Service.MediaSelection(p.ctx, id, branch, uri)
		case "receipts":
			value, err = p.Service.MediaReceipt(p.ctx, id, branch, uri)
		}
		var raw []byte
		if err == nil {
			raw, err = json.MarshalIndent(value, "", "  ")
		}
		bytesRead := len(data)
		// Do not retain video bytes, raw originals or several previews in UI state.
		data = nil
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation || p.pane != "media" || p.media.Branch != branch || p.media.Mode != mode || p.media.Request != request {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.problem = ""
			p.media.JSON, p.media.ManifestURI = string(raw), uri
			p.media.Bitmap, p.media.PreviewBytes = bitmap, bytesRead
			if playbackKind != "" {
				p.media.PlaybackURI, p.media.PlaybackDuration = uri, videoDuration
				p.media.PlaybackKind = playbackKind
				p.playerSeek = "0"
				p.playerPosition, p.playerDuration = 0, videoDuration
			}
		})
	}()
}

func (p *NativePage) buildMedia(c *ui.Context) {
	ui.Scroll(c).Grow(1).MinHeight(0).Gap(8).Children(func() {
		ui.Text(c, "Media evidence · read only").Bold()
		ui.Text(c, "Opening lists metadata only. Original uploads are unchanged and not automatic model input. Click a manifest for losses/frame times; Preview verifies derivative bytes. Only explicit Play starts an independently approved local decoder, never a model or media selection.").FontSize(11)
		ui.Row(c).Gap(6).Children(func() {
			ui.Button(c, "Original upload index").Disabled(p.paneBusy).OnClick(func() { p.mediaNavigate("sources", "") })
			ui.Button(c, "Selections and delivery receipts").Disabled(p.paneBusy).OnClick(func() { p.mediaNavigate("selections", "") })
			ui.Button(c, "Refresh media index").Disabled(p.paneBusy).OnClick(func() { p.media.Back = nil; p.loadMedia(0, 0) })
			ui.Button(c, "Close media evidence").OnClick(func() { p.setPane("") })
		})
		ui.Text(c, "Branch: "+p.media.Branch+" · page: "+p.media.Mode).FontSize(11)
		ui.Row(c).Gap(6).Children(func() {
			for _, branch := range p.detail.Branches {
				branch := branch
				ui.Button(c.Key(branch.ID), "Inspect media "+branch.Target.EngineID).Disabled(p.paneBusy || branch.Target.EngineID != "alva" || branch.NativeSessionID == "" || branch.ID == p.media.Branch).OnClick(func() {
					p.media = MediaPane{Branch: branch.ID, Mode: "sources"}
					p.loadMedia(0, 0)
				})
			}
		})
		if p.media.SourceURI != "" {
			ui.Text(c, "Source reference: "+p.media.SourceURI).FontSize(11)
		}
		if p.media.SelectionID != "" {
			ui.Text(c, "Selection: "+p.media.SelectionID).FontSize(11)
		}
		for _, entry := range p.media.Page.Entries {
			entry := entry
			ui.Text(c, fmt.Sprintf("%s · %s · %d bytes · SHA-256 %s", entry.Info.Name, entry.Info.MediaType, entry.Info.Bytes, entry.Info.SHA256)).FontSize(12)
			ui.Text(c, entry.Info.Processing).FontSize(11)
			switch p.media.Mode {
			case "sources":
				ui.Button(c.Key(entry.Info.ID), "List derived media for "+entry.Info.Name).Disabled(p.paneBusy).OnClick(func() { p.mediaNavigate("artifacts", entry.Info.Reference().URI) })
			case "artifacts":
				ui.Row(c).Gap(6).Children(func() {
					ui.Button(c.Key("manifest-"+entry.Info.ID), "Read processing manifest").Disabled(p.paneBusy).OnClick(func() { p.readMedia(entry.Info.Reference().URI, false) })
					ui.Button(c.Key("preview-"+entry.Info.ID), "Preview verified derivative").Disabled(p.paneBusy).OnClick(func() { p.readMedia(entry.Info.Reference().URI, true) })
				})
			case "selections":
				ui.Row(c).Gap(6).Children(func() {
					ui.Button(c.Key("selection-"+entry.Info.ID), "Read exact selection manifest").Disabled(p.paneBusy).OnClick(func() { p.readMedia(entry.Info.Reference().URI, false) })
					ui.Button(c.Key("receipts-"+entry.Info.ID), "Read delivery receipt index").Disabled(p.paneBusy).OnClick(func() { p.mediaNavigate("receipts", entry.Info.Name) })
				})
			case "receipts":
				ui.Button(c.Key(entry.Info.ID), "Read receipt state").Disabled(p.paneBusy).OnClick(func() { p.readMedia(entry.Info.Reference().URI, false) })
			}
		}
		if len(p.media.Page.Entries) == 0 && !p.paneBusy {
			ui.Text(c, "No committed entries in this page range. This does not prove media was never read or delivered through another path.").FontSize(11)
		}
		ui.Row(c).Gap(6).Children(func() {
			ui.Button(c, "Previous media page").Disabled(p.paneBusy || len(p.media.Back) == 0).OnClick(func() {
				n := len(p.media.Back) - 1
				previous := p.media.Back[n]
				p.media.Back = p.media.Back[:n]
				p.loadMedia(previous, p.media.Page.HighWater)
			})
			ui.Button(c, "Next media page").Disabled(p.paneBusy || !p.media.Page.HasMore).OnClick(func() {
				p.media.Back = append(p.media.Back, p.media.Cursor)
				p.loadMedia(p.media.Page.Next, p.media.Page.HighWater)
			})
		})
		ui.Text(c, "Prepared != sent. model_chain_invocation_intent is a durable intent before invoking the model chain, not proof of network acceptance. Response observation is not visual understanding. No automatic retry or replay; selected originals/audio/full video are never implied.").FontSize(11)
		if p.media.PreviewBytes > 0 {
			ui.Text(c, fmt.Sprintf("Explicit preview verified %d immutable derivative bytes. Local preview is not model delivery.", p.media.PreviewBytes)).FontSize(11)
		}
		if p.media.Bitmap != nil {
			ui.Image(c, p.media.Bitmap).Width(400).MaxHeight(400).Fit(ui.Contain).Label("Verified derived media preview")
		} else if p.media.PreviewBytes > 0 && !p.playerBusy {
			ui.Text(c, "Verification did not autoplay. Explicit Play reads this selected derivative again.").FontSize(11)
		}
		p.buildPlayback(c)
		if p.media.JSON != "" {
			ui.Button(c, "Close media detail and release preview").OnClick(func() {
				p.stopPlayback()
				p.media.JSON, p.media.ManifestURI, p.media.Bitmap, p.media.PreviewBytes = "", "", nil, 0
				p.media.PlaybackURI, p.media.PlaybackDuration, p.media.PlaybackKind = "", 0, ""
			})
			ui.Text(c, p.media.JSON).FontSize(11)
		}
	})
}

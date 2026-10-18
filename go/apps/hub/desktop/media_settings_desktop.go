// INPUT: Explicit native decoder paths and independent next-turn product preferences
// OUTPUT: Default-off media capability and separate decoder host-access consent controls
// POS: [L7 Hub product] Native operator configuration never discovers, starts or uploads media on save
//go:build desktop

package hubdesktop

import (
	"fmt"

	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/egoist/mygo/ui"
)

func (p *NativePage) openMediaSettings() {
	if p.selected == "" || p.paneBusy {
		return
	}
	p.setPane("media-settings")
	id, generation := p.selected, p.generation
	p.paneBusy = true
	go func() {
		value, err := p.Service.Store.Settings(p.ctx, id)
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation || p.pane != "media-settings" {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.settings = value
			p.decoderFFmpeg, p.decoderProbe = value.MediaDecoder.FFmpeg, value.MediaDecoder.FFprobe
		})
	}()
}

func (p *NativePage) mediaSetting(action string, enabled bool) {
	if p.selected == "" || p.paneBusy || p.model.ActiveRunID != "" {
		return
	}
	id, generation := p.selected, p.generation
	decoder := hub.MediaDecoder{FFmpeg: p.decoderFFmpeg, FFprobe: p.decoderProbe}
	p.paneBusy = true
	go func() {
		var err error
		switch action {
		case "media":
			err = p.Service.Store.SetMediaEnabled(p.ctx, id, enabled)
		case "decoder":
			err = p.Service.Store.SetMediaDecoderEnabled(p.ctx, id, enabled, enabled)
		case "save":
			err = p.Service.Store.ReplaceMediaDecoder(p.ctx, id, decoder)
		}
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.problem = ""
			if p.pane == "media-settings" {
				p.openMediaSettings()
			}
		})
	}()
}

func (p *NativePage) buildMediaSettings(c *ui.Context) {
	ui.Scroll(c).Grow(1).MinHeight(0).Gap(8).Children(func() {
		ui.Text(c, "Media tools · explicit next-turn opt-in").Bold()
		ui.Text(c, fmt.Sprintf("Enabled=%v. Originals stay unchanged; upload only adds a reference. Tool actions require the actual Guard; processing/selecting images is not implicit approval to read all uploads or send original video/audio. Applies only to the next Alva turn, not other engines or SDK children.", p.settings.MediaEnabled)).FontSize(12)
		if p.settings.MediaEnabled {
			ui.Button(c, "Disable media tools next turn").Disabled(p.paneBusy || p.model.ActiveRunID != "").OnClick(func() { p.mediaSetting("media", false) })
		} else {
			ui.Button(c, "Enable explicit media tools next turn").Disabled(p.paneBusy || p.model.ActiveRunID != "").OnClick(func() { p.mediaSetting("media", true) })
		}
		ui.Text(c, "PNG/JPEG/GIF use bounded standard-library image processing without external programs: first decoded image, <=512px, JPEG quality75, white transparency, explicit losses. This is not an OS/RSS decoder sandbox and does not claim OCR or image understanding.").FontSize(11)
		ui.Text(c, "Image delivery additionally requires this process operator's --image-capable declaration for the exact live provider/model route. Unknown routes remain unsupported even when media tools are enabled. Only explicitly selected derived JPEGs enter one request copy; history does not replay images.").FontSize(11)
		ui.Text(c, "Optional video/WebP decoder · full native host access").Bold()
		ui.Text(c, "Enter absolute ffmpeg and ffprobe executable paths. No PATH discovery, download, install or inherited credentials. Saving clears approval and disables the decoder; independent approval enables it only for subsequent turns. Executable files are not hashed: same-path binary replacement is not detected.").FontSize(11)
		ui.TextInput(c.Key("media-ffmpeg"), &p.decoderFFmpeg).Label("Explicit ffmpeg absolute path").Disabled(p.paneBusy || p.model.ActiveRunID != "")
		ui.TextInput(c.Key("media-ffprobe"), &p.decoderProbe).Label("Explicit ffprobe absolute path").Disabled(p.paneBusy || p.model.ActiveRunID != "")
		ui.Button(c, "Save decoder paths disabled").Disabled(p.paneBusy || p.model.ActiveRunID != "").OnClick(func() { p.mediaSetting("save", false) })
		ui.Text(c, fmt.Sprintf("Saved decoder: enabled=%v, host approved=%v · ffmpeg=%s · ffprobe=%s", p.settings.MediaDecoder.Enabled, p.settings.MediaDecoder.HostAccessApproved, p.settings.MediaDecoder.FFmpeg, p.settings.MediaDecoder.FFprobe)).FontSize(11)
		if p.settings.MediaDecoder.Enabled {
			ui.Button(c, "Disable external decoder next turn").Disabled(p.paneBusy || p.model.ActiveRunID != "").OnClick(func() { p.mediaSetting("decoder", false) })
		} else {
			ui.Button(c, "Approve full host decoder access next turn").Disabled(p.paneBusy || p.model.ActiveRunID != "" || p.settings.MediaDecoder.FFmpeg == "" || p.settings.MediaDecoder.FFprobe == "").OnClick(func() { p.mediaSetting("decoder", true) })
		}
		ui.Text(c, "Actual transforms retain hashes, source/output dimensions, real extraction timestamps, selected strategy, codec/fps/range and losses. No scene detection or audio transcription. Preview and request intent are not evidence of network acceptance or model understanding.").FontSize(11)
	})
}

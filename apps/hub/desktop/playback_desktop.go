// INPUT: Explicit Play/Pause/seek actions and verified audio/video derivatives
// OUTPUT: One local voice or bitmap, one pending UI update and truthful cleanup ownership
// POS: [L7 Hub product] Optional local playback never uses WebView, autoplay or durable model receipts
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"fmt"
	"strconv"
	"time"

	mediaextension "github.com/QuincySx/Alva/extension/media"
	nativemedia "github.com/QuincySx/Alva/platform/system/media"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/egoist/mygo/ui"
)

func (p *NativePage) playMedia() {
	if p.paneBusy || p.playerBusy || p.selected == "" || p.media.PlaybackURI == "" {
		return
	}
	seek, err := strconv.ParseInt(p.playerSeek, 10, 64)
	if err != nil || seek < 0 || seek >= p.media.PlaybackDuration {
		p.problem = "Seek must be a millisecond position within this verified derivative."
		return
	}
	id, generation, branch, uri, kind := p.selected, p.generation, p.media.Branch, p.media.PlaybackURI, p.media.PlaybackKind
	makeVoice, outputError := p.audioVoice, p.audioOutputError
	p.playerToken++
	token := p.playerToken
	ctx, cancel := context.WithCancel(p.ctx)
	p.playerCancel = cancel
	p.playerBusy = true
	p.playerStopping = false
	p.playerFrames = 0
	p.problem = ""
	p.playerPosition, p.playerDuration = seek, p.media.PlaybackDuration
	go func() {
		var manifest mediaextension.Manifest
		var player localPlayback
		var runErr error
		var run func(context.Context, int64, func(context.Context, nativemedia.PlaybackFrame) error) error
		if kind == "audio" {
			var data []byte
			manifest, data, runErr = p.Service.AudioPlayback(ctx, id, branch, uri)
			if runErr == nil {
				var audio *audioPlayback
				audio, runErr = newAudioPlayback(data, manifest.Output.DurationMillis, makeVoice, outputError)
				if runErr == nil {
					player = audio
					run = func(ctx context.Context, seek int64, emit func(context.Context, nativemedia.PlaybackFrame) error) error {
						return audio.Run(ctx, seek, func(ctx context.Context, position, duration int64) error {
							return emit(ctx, nativemedia.PlaybackFrame{PositionMillis: position, DurationMillis: duration})
						})
					}
				}
			}
		} else {
			var video *nativemedia.Playback
			manifest, video, runErr = p.Service.MediaPlayback(ctx, id, branch, uri)
			if video != nil {
				player, run = video, video.Run
			}
		}
		if runErr == nil {
			p.dispatch(func() {
				if p.playerToken == token {
					p.player = player
					if p.selected == id && p.generation == generation && p.pane == "media" && p.media.PlaybackURI == uri {
						p.media.PreviewBytes = int(manifest.Artifact.Bytes)
					}
				}
			})
			runErr = run(ctx, seek, func(frameCtx context.Context, frame nativemedia.PlaybackFrame) error {
				ack := make(chan struct{})
				p.dispatch(func() {
					defer close(ack)
					if p.playerToken != token || p.selected != id || p.generation != generation || p.pane != "media" || p.media.PlaybackURI != uri {
						return
					}
					if frame.Image == nil {
						// Audio reports only the mixer's position; no fake video frame.
					} else if p.media.Bitmap == nil {
						p.media.Bitmap = ui.NewBitmap(frame.Image)
					} else {
						p.media.Bitmap.Update(frame.Image)
					}
					p.playerPosition, p.playerDuration = frame.PositionMillis, frame.DurationMillis
					p.playerFrames++
				})
				select {
				case <-ack:
					return frameCtx.Err()
				case <-frameCtx.Done():
					return frameCtx.Err()
				}
			})
		}
		if player != nil {
			cleanup, stop := context.WithTimeout(context.Background(), 5*time.Second)
			runErr = errors.Join(runErr, player.Close(cleanup))
			stop()
		}
		cancel()
		complete := player == nil || player.CleanupComplete()
		p.dispatch(func() {
			if p.playerToken != token {
				return
			}
			p.playerBusy, p.playerStopping = !complete, false
			p.player = player
			if complete {
				p.player = nil
				p.playerCancel = nil
			}
			sameVideo := p.selected == id && p.generation == generation && p.pane == "media" && p.media.PlaybackURI == uri
			if runErr == nil && sameVideo {
				p.playerPosition = p.playerDuration
				p.playerSeek = "0"
			} else if abi.OnlyCancellation(runErr) && sameVideo {
				p.playerSeek = strconv.FormatInt(min(p.playerPosition, max(0, p.playerDuration-1)), 10)
			} else if runErr != nil && !abi.OnlyCancellation(runErr) {
				p.problem = "Local playback failed: " + runErr.Error()
			}
			if !complete {
				p.problem = "Local playback cleanup is incomplete. Keep this window open and retry Pause/cleanup; no new playback will start."
			}
		})
	}()
}

func (p *NativePage) stopPlayback() {
	if !p.playerBusy && p.player == nil {
		return
	}
	if p.playerCancel != nil {
		p.playerCancel()
	}
	if p.playerStopping {
		return
	}
	p.playerStopping = true
	player, token := p.player, p.playerToken
	if player == nil {
		return
	} // Constructor observes its cancelled owner and settles.
	go func() {
		cleanup, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		err := player.Close(cleanup)
		complete := player.CleanupComplete()
		p.dispatch(func() {
			if p.playerToken != token {
				return
			}
			p.playerStopping = false
			if complete {
				p.playerBusy = false
				p.player = nil
				p.playerCancel = nil
			} else {
				p.problem = "Local playback cleanup is not complete; retry cleanup before closing or starting another playback."
			}
			if err != nil {
				p.problem = "Local decoder cleanup failed: " + err.Error()
			}
		})
	}()
}

func (p *NativePage) buildPlayback(c *ui.Context) {
	if p.media.PlaybackURI == "" {
		return
	}
	if p.media.PlaybackKind == "audio" {
		ui.Text(c, "Local audio playback").Bold()
		ui.Text(c, "Play this derived mono 16kHz WAV (at most 60 seconds). Uses the system audio output. Seek and position refer to the mixer, not an acoustic or device-playhead receipt. Pause closes this voice; already queued audio may take a short buffer to drain. No audio is sent to a model.").FontSize(11)
		ui.TextInput(c.Key("audio-seek"), &p.playerSeek).Label("Local audio seek milliseconds").Disabled(p.playerBusy)
		ui.Row(c).Gap(6).Children(func() {
			ui.Button(c, "Play selected audio derivative").Disabled(p.playerBusy || p.paneBusy).OnClick(p.playMedia)
			ui.Button(c, "Pause and close local audio").Disabled(!p.playerBusy || p.playerStopping).OnClick(p.stopPlayback)
		})
		ui.Text(c, fmt.Sprintf("Mixer position: %d / %d ms · playback/cleanup pending=%v", p.playerPosition, p.playerDuration, p.playerBusy)).FontSize(11)
		return
	}
	ui.Text(c, "Local silent video playback · explicit native decoder").Bold()
	ui.Text(c, "Play reads this verified transcode only (<=8MiB/60s/720px/5fps, no audio). Requires separate saved decoder host approval. One bitmap is updated on the UI thread; no frames are sent to a model. Playback clock is nominal 5fps, NOT original/source PTS evidence. Decoder diagnostics are non-persistent and not a processing/delivery receipt.").FontSize(11)
	ui.TextInput(c.Key("video-seek"), &p.playerSeek).Label("Local video seek milliseconds").Disabled(p.playerBusy)
	ui.Row(c).Gap(6).Children(func() {
		ui.Button(c, "Play selected silent derivative").Disabled(p.playerBusy || p.paneBusy).OnClick(p.playMedia)
		ui.Button(c, "Pause and reap local decoder").Disabled(!p.playerBusy || p.playerStopping).OnClick(p.stopPlayback)
	})
	ui.Text(c, fmt.Sprintf("Nominal playback position: %d / %d ms · running/cleanup pending=%v", p.playerPosition, p.playerDuration, p.playerBusy)).FontSize(11)
}

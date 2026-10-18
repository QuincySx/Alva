// INPUT: Explicit real ffmpeg fixture, generated silent derivatives and cancellable frame consumers
// OUTPUT: Incremental RGBA/seek, immutable bytes, finite pacing and actual playback/processor owner cleanup regressions
// POS: [platform / OS primitive] No user video, paid service, full-frame accumulation or OS-window acceptance claim
package media

import (
	"bytes"
	"context"
	"errors"
	"os"
	"sync/atomic"
	"testing"
	"time"
)

func playbackFixture(t *testing.T) (*Processor, PlaybackInput) {
	t.Helper()
	p := localProcessor(t)
	output, err := p.TranscodeVideo(t.Context(), videoFixture(t, p), "video/mp4")
	if err != nil {
		t.Fatal(err)
	}
	return p, PlaybackInput{Data: output.Data, SHA256: output.Operation.OutputSHA256, Width: output.Width, Height: output.Height, DurationMillis: output.DurationMillis, Codec: output.Codec, FrameRate: output.FrameRate}
}
func TestActualPlaybackStreamsBoundedFramesAndSeekWithoutRetainingWholeVideo(t *testing.T) {
	p, input := playbackFixture(t)
	original := bytes.Clone(input.Data)
	player, err := p.NewPlayback(input)
	if err != nil {
		t.Fatal(err)
	}
	if files, e := os.ReadDir(p.config.TempDirectory); e != nil || len(files) != 0 {
		t.Fatal("constructor started decoder/snapshot", e)
	}
	frames := 0
	last := int64(-1)
	start := time.Now()
	err = player.Run(t.Context(), 600, func(ctx context.Context, frame PlaybackFrame) error {
		if ctx.Err() != nil || frame.Image.Bounds().Dx() != input.Width || frame.Image.Bounds().Dy() != input.Height || len(frame.Image.Pix) != input.Width*input.Height*4 || frame.PositionMillis < 600 || frame.PositionMillis <= last || frame.DurationMillis != input.DurationMillis {
			t.Error("bad actual bounded frame", frame.PositionMillis)
		}
		last = frame.PositionMillis
		frames++
		return nil
	})
	if err != nil || frames < 2 || frames > 12 || time.Since(start) < time.Duration(frames-1)*200*time.Millisecond || !player.CleanupComplete() {
		t.Fatal("playback not paced or physically complete", frames, err)
	}
	if !bytes.Equal(input.Data, original) {
		t.Fatal("playback changed immutable derivative bytes")
	}
	if err = player.Run(t.Context(), 0, func(context.Context, PlaybackFrame) error { return nil }); err == nil {
		t.Fatal("single-use decoder was replayed")
	}
	if err = player.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	if files, e := os.ReadDir(p.config.TempDirectory); e != nil || len(files) != 0 {
		t.Fatal("playback left readable temp snapshots", files, e)
	}
}
func TestPlaybackPauseAndContextBlockedConsumerReallyCloseAndReap(t *testing.T) {
	p, input := playbackFixture(t)
	player, err := p.NewPlayback(input)
	if err != nil {
		t.Fatal(err)
	}
	first := make(chan struct{})
	done := make(chan error, 1)
	var frames atomic.Int32
	go func() {
		done <- player.Run(context.Background(), 0, func(ctx context.Context, frame PlaybackFrame) error {
			frames.Add(1)
			close(first)
			<-ctx.Done()
			return ctx.Err()
		})
	}()
	select {
	case <-first:
	case <-time.After(5 * time.Second):
		t.Fatal("no actual playback frame")
	}
	cleanup, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err = player.Close(cleanup); err != nil {
		t.Fatal("Pause failed actual cleanup", err)
	}
	if err = <-done; !errors.Is(err, context.Canceled) || !player.CleanupComplete() || frames.Load() != 1 {
		t.Fatal("Pause leaked/advanced or hid cancellation", err, frames.Load())
	}
	player.mu.Lock()
	child := player.child
	player.mu.Unlock()
	if child == nil || !child.CleanupComplete() {
		t.Fatal("Close fabricated reaped state")
	}
	if files, e := os.ReadDir(p.config.TempDirectory); e != nil || len(files) != 0 {
		t.Fatal("pause left snapshots", files, e)
	}
	// Seeking is a new explicit decode after physical cleanup, not old stream replay.
	p, err = NewProcessor(p.config)
	if err != nil {
		t.Fatal(err)
	}
	seek, err := p.NewPlayback(input)
	if err != nil {
		t.Fatal(err)
	}
	ctx, stop := context.WithCancel(t.Context())
	defer stop()
	err = seek.Run(ctx, 1000, func(_ context.Context, frame PlaybackFrame) error {
		if frame.PositionMillis != 1000 {
			t.Error("seek nominal clock lost", frame.PositionMillis)
		}
		stop()
		return nil
	})
	if !errors.Is(err, context.Canceled) || !seek.CleanupComplete() {
		t.Fatal("seek cancellation did not reap", err)
	}
}
func TestPlaybackRefusesUnverifiedOrOutOfBudgetMedia(t *testing.T) {
	p, input := playbackFixture(t)
	for _, bad := range []PlaybackInput{{Data: input.Data, SHA256: "wrong", Width: input.Width, Height: input.Height, DurationMillis: input.DurationMillis, Codec: input.Codec, FrameRate: input.FrameRate}, {Data: input.Data, SHA256: input.SHA256, Width: 721, Height: 16, DurationMillis: 1000, Codec: "h264", FrameRate: "5/1"}, {Data: input.Data, SHA256: input.SHA256, Width: 16, Height: 16, DurationMillis: 60001, Codec: "h264", FrameRate: "5/1"}} {
		if _, err := p.NewPlayback(bad); err == nil {
			t.Fatal("unverified playback accepted")
		}
	}
	player, err := p.NewPlayback(input)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if err = player.Run(ctx, 0, func(context.Context, PlaybackFrame) error { t.Error("cancelled admission emitted frame"); return nil }); !errors.Is(err, context.Canceled) || !player.CleanupComplete() {
		t.Fatal("cancelled playback admission lost cancellation/cleanup", err)
	}
}

func TestProcessorCloseCancelsPlaybackConsumerWithoutPlaybackClose(t *testing.T) {
	p, input := playbackFixture(t)
	player, err := p.NewPlayback(input)
	if err != nil {
		t.Fatal(err)
	}
	first := make(chan struct{})
	done := make(chan error, 1)
	go func() {
		done <- player.Run(context.Background(), 0, func(ctx context.Context, _ PlaybackFrame) error {
			close(first)
			<-ctx.Done()
			return ctx.Err()
		})
	}()
	select {
	case <-first:
	case <-time.After(5 * time.Second):
		t.Fatal("no actual playback frame")
	}
	cleanup, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	// This is intentionally NOT player.Close: the owner must cancel emit's
	// context on its own rather than waiting for the 100-second Run timeout.
	if err = p.Close(cleanup); err != nil {
		t.Fatal("processor owner could not join cooperative playback", err)
	}
	select {
	case err = <-done:
	case <-cleanup.Done():
		t.Fatal("processor Close left Run blocked in emit", cleanup.Err())
	}
	if !errors.Is(err, context.Canceled) || !p.CleanupComplete() || !player.CleanupComplete() {
		t.Fatal("processor owner lost cancellation/physical cleanup", err)
	}
	if files, e := os.ReadDir(p.config.TempDirectory); e != nil || len(files) != 0 {
		t.Fatal("owner shutdown left playback snapshots", files, e)
	}
}

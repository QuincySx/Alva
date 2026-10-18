// INPUT: Actual local ffmpeg-derived artifact, product decoder consent and headless MyGo Play/Pause/seek controls
// OUTPUT: No-autoplay, one native bitmap, actual Pause/reap, seek/navigation cleanup and drain-before-reap fixture regressions
// POS: [L7 Hub product] Real decoder/native controls, not OS window pixels, model video input or durable playback receipt acceptance
//go:build desktop

package hubdesktop

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	mediaextension "github.com/QuincySx/Alva/extension/media"
	"github.com/QuincySx/Alva/platform/storage/session"
	nativemedia "github.com/QuincySx/Alva/platform/system/media"
	"github.com/QuincySx/Alva/platform/system/process"
	"github.com/egoist/mygo/ui"
)

func nativeVideoFixture(t *testing.T) (string, string, []byte, nativemedia.Output) {
	t.Helper()
	var ffmpeg, ffprobe string
	for _, dir := range []string{"/opt/homebrew/bin", "/usr/bin"} {
		a, b := filepath.Join(dir, "ffmpeg"), filepath.Join(dir, "ffprobe")
		if _, e := os.Stat(a); e != nil {
			continue
		}
		if _, e := os.Stat(b); e != nil {
			continue
		}
		ffmpeg, ffprobe = a, b
		break
	}
	if ffmpeg == "" {
		t.Skip("explicit local decoder fixture unavailable; no production discovery/install")
	}
	directory := filepath.Join(t.TempDir(), "private")
	if err := os.Mkdir(directory, 0700); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	child, err := process.Start(process.Config{Command: ffmpeg, Cwd: directory, Args: []string{"-nostdin", "-hide_banner", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=96x64:rate=5", "-t", "2", "-an", "-c:v", "libx264", "-threads", "1", "-pix_fmt", "yuv420p", "-movflags", "frag_keyframe+empty_moov", "-f", "mp4", "pipe:1"}})
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		cleanup, cleanupCancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cleanupCancel()
		if err := child.Close(cleanup); err != nil {
			t.Error("fixture encoder cleanup:", err)
		}
	}()
	stop := context.AfterFunc(ctx, child.BeginClose)
	defer stop()
	_ = child.Stdin.Close()
	stderr := make(chan error, 1)
	go func() { _, e := io.Copy(io.Discard, io.LimitReader(child.Stderr, 64<<10)); stderr <- e }()
	data, err := io.ReadAll(io.LimitReader(child.Stdout, 1<<20))
	if err != nil {
		t.Fatal(err)
	}
	// Wait reclaims the owned process and closes its pipes. Join both readers
	// first, otherwise a valid stderr EOF can race with that pipe closure.
	if err = <-stderr; err != nil {
		t.Fatal(err)
	}
	if err = child.Wait(ctx); err != nil {
		t.Fatal(err)
	}
	if code, ok := child.ExitCode(); !ok || code != 0 || !child.CleanupComplete() {
		t.Fatal("fixture encoder not complete", code)
	}
	processor, err := nativemedia.NewProcessor(nativemedia.ProcessorConfig{FFmpeg: ffmpeg, FFprobe: ffprobe, TempDirectory: directory})
	if err != nil {
		t.Fatal(err)
	}
	out, err := processor.TranscodeVideo(t.Context(), data, "video/mp4")
	if err != nil {
		t.Fatal(err)
	}
	return ffmpeg, ffprobe, data, out
}
func awaitPlayback(t *testing.T, tester *ui.Tester, p *NativePage, ready func() bool) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if ready() {
			return
		}
		if p.problem != "" {
			t.Fatal(p.problem)
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatalf("actual native playback did not settle: problem=%q busy=%v stopping=%v player=%v frames=%d bitmap=%v uri=%q seek=%q pane=%q", p.problem, p.playerBusy, p.playerStopping, p.player != nil, p.playerFrames, p.media.Bitmap != nil, p.media.PlaybackURI, p.playerSeek, p.pane)
}
func TestNativeVideoPlaybackIsExplicitSelfDrawnSeekableAndReaped(t *testing.T) {
	ffmpeg, ffprobe, original, out := nativeVideoFixture(t)
	service, requests := fixture(t)
	ctx := context.Background()
	s, err := service.Create(ctx, "actual video", "")
	if err != nil {
		t.Fatal(err)
	}
	upload, err := service.UploadReader(ctx, s.ID, "clip.mp4", bytes.NewReader(original))
	if err != nil {
		t.Fatal(err)
	}
	detail, err := service.Store.Detail(ctx, s.ID)
	if err != nil {
		t.Fatal(err)
	}
	branch, err := product.ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	derived, err := service.Repository.PutArtifact(ctx, s.ID, branch.NativeSessionID, session.ArtifactInput{Kind: "media-derivative", Name: upload.Info.ID, MediaType: out.MediaType, Data: out.Data, Processing: out.Operation.Description})
	if err != nil {
		t.Fatal(err)
	}
	manifest := mediaextension.Manifest{Version: 1, Source: upload.Reference, Artifact: derived.Reference(), Output: out, Processing: "actual silent video derivative; no model delivery"}
	raw, _ := json.Marshal(manifest)
	_, err = service.Repository.PutArtifact(ctx, s.ID, branch.NativeSessionID, session.ArtifactInput{Kind: "media-manifest", Name: upload.Info.ID, MediaType: "application/json", Data: raw, Text: true, Processing: "actual video provenance"})
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(service)
	p.selected, p.detail = s.ID, detail
	p.applyDetail()
	tester := ui.NewTester(p.Build, 1600, 2000)
	for _, label := range []string{"Media evidence", "List derived media for clip.mp4", "Read processing manifest"} {
		if err = tester.Click(label); err != nil {
			t.Fatal(err)
		}
		settlePage(t, tester, p)
	}
	if p.playerBusy || p.player != nil || p.media.Bitmap != nil {
		t.Fatal("cold metadata read autoplayed video")
	}
	if err = tester.Click("Play selected silent derivative"); err != nil {
		t.Fatal(err)
	}
	awaitPlayback(t, tester, p, func() bool { return !p.playerBusy })
	if p.problem == "" {
		t.Fatal("unapproved decoder played")
	}
	p.problem = ""
	if err = service.Store.ReplaceMediaDecoder(ctx, s.ID, hub.MediaDecoder{FFmpeg: ffmpeg, FFprobe: ffprobe}); err != nil {
		t.Fatal(err)
	}
	if err = service.Store.SetMediaDecoderEnabled(ctx, s.ID, true, true); err != nil {
		t.Fatal(err)
	}
	tester.Frame() // reflect the explicitly cleared error before locating controls.
	if err = tester.Click("Play selected silent derivative"); err != nil {
		t.Fatal(err)
	}
	awaitPlayback(t, tester, p, func() bool { return p.media.Bitmap != nil && p.player != nil })
	player := p.player
	if p.media.PreviewBytes != len(out.Data) || !p.playerBusy {
		t.Fatal("actual explicit video read/bitmap not retained")
	}
	if err = tester.Click("Pause and reap local decoder"); err != nil {
		t.Fatal(err)
	}
	awaitPlayback(t, tester, p, func() bool { return !p.playerBusy })
	if !player.CleanupComplete() {
		t.Fatal("Pause did not reap actual decoder")
	}
	if err = tester.Click("Local video seek milliseconds"); err != nil {
		t.Fatal(err)
	}
	tester.Key(ui.Cmd, ui.KeyA)
	tester.Type("1000")
	if err = tester.Click("Play selected silent derivative"); err != nil {
		t.Fatal(err)
	}
	awaitPlayback(t, tester, p, func() bool { return p.playerBusy && p.playerPosition >= 1000 && p.player != nil && p.playerFrames > 0 })
	seekPlayer := p.player
	if err = tester.Click("Chat"); err != nil {
		t.Fatal(err)
	}
	awaitPlayback(t, tester, p, func() bool { return !p.playerBusy })
	if !seekPlayer.CleanupComplete() || p.media.Bitmap != nil {
		t.Fatal("navigation retained video decoder/bitmap")
	}
	if service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("local playback started model execution")
	}
}

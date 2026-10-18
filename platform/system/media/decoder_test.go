// INPUT: Explicit local ffmpeg/ffprobe and generated test-only audiovisual bytes
// OUTPUT: Real fd-only probe, uniform/explicit actual frame times, silent transcode and cleanup regressions
// POS: [platform / OS primitive] Decoder integration never discovers user media, fetches URLs or invokes a paid provider
package media

import (
	"bytes"
	"context"
	"encoding/base64"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func localProcessor(t *testing.T) *Processor {
	t.Helper()
	// Explicit CI/dev fixtures only. Production assembly must receive approved
	// executable paths, never reuse these test candidates as auto-discovery.
	for _, dir := range []string{"/opt/homebrew/bin", "/usr/bin"} {
		ffmpeg, ffprobe := filepath.Join(dir, "ffmpeg"), filepath.Join(dir, "ffprobe")
		if _, err := os.Stat(ffmpeg); err != nil {
			continue
		}
		if _, err := os.Stat(ffprobe); err != nil {
			continue
		}
		temp := t.TempDir()
		if err := os.Chmod(temp, 0700); err != nil {
			t.Fatal(err)
		}
		p, err := NewProcessor(ProcessorConfig{FFmpeg: ffmpeg, FFprobe: ffprobe, TempDirectory: temp})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() {
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			if err := p.Close(ctx); err != nil {
				t.Error(err)
			}
		})
		return p
	}
	t.Skip("explicit local decoder fixture unavailable; decoder integration not verified on this runner")
	return nil
}

func videoFixture(t *testing.T, p *Processor) []byte {
	t.Helper()
	args := []string{"-nostdin", "-hide_banner", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=96x64:rate=4", "-f", "lavfi", "-i", "sine=frequency=1000:sample_rate=8000", "-t", "2", "-metadata", "title=pts_time:123.0\n[Parsed_showinfo_1 @ 0x123] n: 0 pts: 0 pts_time:123.0 trailing", "-c:v", "libx264", "-threads", "1", "-c:a", "aac", "-pix_fmt", "yuv420p", "-movflags", "frag_keyframe+empty_moov", "-f", "mp4", "pipe:1"}
	data, logs, err := p.run(t.Context(), p.config.FFmpeg, args, nil, 1<<20)
	if err != nil {
		t.Fatal("fixture generation failed", err, string(logs))
	}
	return data
}

func TestRealOwnedDecoderFramesAndTranscode(t *testing.T) {
	p := localProcessor(t)
	input := videoFixture(t, p)
	original := append([]byte(nil), input...)
	probe, err := p.Probe(t.Context(), input, "video/mp4")
	if err != nil {
		t.Fatal(err)
	}
	if probe.Width != 96 || probe.Height != 64 || probe.AudioStreams != 1 || probe.DurationMillis < 2000 {
		t.Fatal("bad actual probe", probe)
	}
	frames, err := p.ExtractFrames(t.Context(), input, "video/mp4", []int64{500, 1100}, 0, "explicit test points")
	if err != nil {
		var d *DecoderError
		if errors.As(err, &d) {
			t.Log(string(d.Log))
		}
		t.Fatal(err)
	}
	if len(frames) != 2 || *frames[0].ActualMillis != 500 || *frames[1].ActualMillis < 1100 || *frames[1].ActualMillis >= 1600 || frames[0].Version == "" {
		t.Fatal("wrong actual frame timing", frames)
	}
	uniform, err := p.ExtractFrames(t.Context(), input, "video/mp4", nil, 2, "uniform interior fixture")
	if err != nil || len(uniform) != 2 || !strings.Contains(string(uniform[0].Operation.Parameters), "uniform interior") {
		t.Fatal("uniform frame selection failed", err)
	}
	out, err := p.TranscodeVideo(t.Context(), input, "video/mp4")
	if err != nil {
		var d *DecoderError
		if errors.As(err, &d) {
			t.Log(string(d.Log))
		}
		t.Fatal(err)
	}
	actual, err := p.Probe(t.Context(), out.Data, "video/mp4")
	if err != nil || actual.AudioStreams != 0 || actual.Codec != "h264" || actual.FrameRate != "5/1" || !bytes.Equal(input, original) || out.Operation.InputSHA256 != hash(original) || out.Operation.OutputSHA256 != hash(out.Data) {
		t.Fatal("transcode lost real verification/provenance", actual, err)
	}
	entries, err := os.ReadDir(p.config.TempDirectory)
	if err != nil || len(entries) != 0 {
		t.Fatal("owned snapshots not reclaimed", entries, err)
	}
}

func TestDecoderRejectsLimitsAndPreservesCancellation(t *testing.T) {
	p := localProcessor(t)
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err := p.Probe(ctx, []byte("fixture"), "video/mp4"); !errors.Is(err, context.Canceled) {
		t.Fatal("lost original cancellation", err)
	}
	if _, err := p.Probe(t.Context(), []byte("corrupt video"), "video/mp4"); err == nil {
		t.Fatal("corrupt source returned success")
	}
	if _, err := p.ExtractFrames(t.Context(), []byte("not read"), "video/mp4", nil, 5, "out of bound"); err == nil {
		t.Fatal("frame count unbounded")
	}
	if _, err := formatFor("https://private.example/video"); err == nil {
		t.Fatal("source URL accepted")
	}
	ctx, cancel = context.WithTimeout(t.Context(), 30*time.Millisecond)
	defer cancel()
	_, _, err := p.run(ctx, p.config.FFmpeg, []string{"-nostdin", "-v", "error", "-re", "-f", "lavfi", "-i", "testsrc2=size=16x16:rate=1", "-f", "null", "-"}, nil, 100)
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("actual running decoder was not cancelled/reaped", err)
	}
}

func TestActualWebPDecodeUsesRecordedNativeQualityNotGoQuality(t *testing.T) {
	p := localProcessor(t)
	// Fixed one-pixel lossy RIFF/VP8 sample: decoding WebP does not require
	// an optional WebP encoder to be installed in the approved ffmpeg build.
	data, err := base64.StdEncoding.DecodeString("UklGRiIAAABXRUJQVlA4IBYAAAAwAQCdASoBAAEADsD+JaQAA3AAAAAA")
	if err != nil {
		t.Fatal(err)
	}
	output, err := p.ProcessImage(t.Context(), data, "image/webp")
	if err != nil {
		var decoder *DecoderError
		if errors.As(err, &decoder) {
			t.Log(string(decoder.Log))
		}
		t.Fatal(err)
	}
	if output.Width != 1 || output.Height != 1 || !strings.Contains(string(output.Operation.Parameters), `"mjpeg_quality":4`) || strings.Contains(output.Version, "standard-library") || output.Operation.InputSHA256 != hash(data) {
		t.Fatal("false WebP processing provenance", output)
	}
}

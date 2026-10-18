// INPUT: Actual local audiovisual decoder fixtures and deliberately malformed PCM envelopes
// OUTPUT: Real audio extraction/resampling, 60-second bound, canonical validation and provenance regressions
// POS: [platform / OS primitive] Tests create no audio device, user media access or model request
package media

import (
	"bytes"
	"context"
	"errors"
	"testing"
)

func TestCanonicalAudioRejectsSpoofedMetadataAndUnboundedSamples(t *testing.T) {
	valid := audioWAV(make([]byte, 32000))
	if duration, err := ValidateAudio(valid); err != nil || duration != 1000 {
		t.Fatal(duration, err)
	}
	for _, offset := range []int{0, 4, 8, 12, 16, 20, 22, 24, 28, 32, 34, 36, 40} {
		bad := bytes.Clone(valid)
		bad[offset] ^= 1
		if _, err := ValidateAudio(bad); err == nil {
			t.Fatalf("accepted changed envelope byte %d", offset)
		}
	}
	for _, bad := range [][]byte{nil, valid[:45], append(bytes.Clone(valid), 0), audioWAV(make([]byte, 1920002))} {
		if _, err := ValidateAudio(bad); err == nil {
			t.Fatal("accepted empty, truncated, trailing or over-budget PCM")
		}
	}
}

func TestActualAudioDerivativeUsesSamplesPreservesSourceAndCapsDuration(t *testing.T) {
	p := localProcessor(t)
	video := videoFixture(t, p)
	before := bytes.Clone(video)
	out, err := p.TranscodeAudio(t.Context(), video, "video/mp4")
	if err != nil {
		t.Fatal(err)
	}
	duration, err := ValidateAudio(out.Data)
	if err != nil || duration < 2000 || duration > 2300 || out.DurationMillis != duration || out.Operation.InputSHA256 != hash(video) || out.Operation.OutputSHA256 != hash(out.Data) || !bytes.Equal(before, video) || out.Version == "" || out.SampleRate != 16000 || out.Channels != 1 || out.Codec != "pcm_s16le" || len(out.Losses) == 0 {
		t.Fatal("invalid actual sample evidence", duration, err, out)
	}
	actual, err := p.Probe(t.Context(), out.Data, "audio/wav")
	if err != nil || actual.AudioStreams != 1 || actual.SampleRate != 16000 || actual.Channels != 1 || actual.DurationMillis != duration {
		t.Fatal("actual ffprobe disagrees", actual, err)
	}
	// Longer source is deliberately clipped, rather than retaining unbounded
	// PCM or believing its declared duration in the output manifest.
	long := audioWAV(make([]byte, 16000*2*61))
	out, err = p.TranscodeAudio(t.Context(), long, "audio/wav")
	if err != nil || len(out.Data) != MaxAudioOutput || out.DurationMillis != 60000 {
		t.Fatal("60-second audio bound failed", len(out.Data), out.DurationMillis, err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err = p.TranscodeAudio(ctx, long, "audio/wav"); !errors.Is(err, context.Canceled) {
		t.Fatal("lost caller cancellation", err)
	}
	if _, err = p.TranscodeAudio(t.Context(), []byte("not WAV"), "audio/wav"); err == nil {
		t.Fatal("corrupt source produced successful audio")
	}
}

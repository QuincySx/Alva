// INPUT: Real media Tool/Guard/native artifacts and a local approved ffmpeg fixture
// OUTPUT: Actual audio tool processing, reference-only evidence and no audio-to-image selection regression
// POS: [L4 extension] Audio derivation does not play sound, transcribe, borrow another owner or deliver audio to a model
package media

import (
	"encoding/binary"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/platform/storage"
	nativemedia "github.com/QuincySx/Alva/platform/system/media"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestActualAudioToolRetainsVerifiedSamplesAndRejectsModelSelection(t *testing.T) {
	f := newFixture(t, true, true)
	var ffmpeg, ffprobe string
	for _, dir := range []string{"/opt/homebrew/bin", "/usr/bin"} {
		a, b := filepath.Join(dir, "ffmpeg"), filepath.Join(dir, "ffprobe")
		if _, err := os.Stat(a); err != nil {
			continue
		}
		if _, err := os.Stat(b); err != nil {
			continue
		}
		ffmpeg, ffprobe = a, b
		break
	}
	if ffmpeg == "" {
		t.Skip("explicit local decoder fixture unavailable")
	}
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	processor, err := nativemedia.NewProcessor(nativemedia.ProcessorConfig{FFmpeg: ffmpeg, FFprobe: ffprobe, TempDirectory: dir})
	if err != nil {
		t.Fatal(err)
	}
	f.plugin.config.Processor = processor
	input := make([]byte, 44+32000)
	copy(input, "RIFF")
	binary.LittleEndian.PutUint32(input[4:], uint32(len(input)-8))
	copy(input[8:], "WAVEfmt ")
	binary.LittleEndian.PutUint32(input[16:], 16)
	binary.LittleEndian.PutUint16(input[20:], 1)
	binary.LittleEndian.PutUint16(input[22:], 1)
	binary.LittleEndian.PutUint32(input[24:], 16000)
	binary.LittleEndian.PutUint32(input[28:], 32000)
	binary.LittleEndian.PutUint16(input[32:], 2)
	binary.LittleEndian.PutUint16(input[34:], 16)
	copy(input[36:], "data")
	binary.LittleEndian.PutUint32(input[40:], 32000)
	f.source, err = f.repo.PutArtifact(t.Context(), "owner", f.log.ID(), storage.ArtifactInput{Kind: "input-attachment", Name: "original.wav", MediaType: "audio/wave", Processing: "original only", Data: input})
	if err != nil {
		t.Fatal(err)
	}
	_, execution := f.direct(t)
	result, err := f.invoke(t, execution, "audio", map[string]any{"action": "transcode_audio", "source_uri": f.source.Reference().URI})
	if err != nil {
		t.Fatal(err)
	}
	var refs []wire.Reference
	if len(result.Content) != 1 || result.Content[0].Kind != wire.ContentText || json.Unmarshal([]byte(result.Content[0].Text), &refs) != nil || len(refs) != 1 {
		t.Fatal("processing did not return one small manifest reference", result)
	}
	manifest, data, err := f.plugin.library.ReadDerivative(t.Context(), refs[0].URI)
	if err != nil || manifest.Artifact.MediaType != "audio/wav" || manifest.Output.DurationMillis != 1000 || manifest.Output.SampleRate != 16000 || manifest.Output.Operation.InputSHA256 != f.source.SHA256 {
		t.Fatal("lost audio provenance", manifest, err)
	}
	if duration, err := nativemedia.ValidateAudio(data); err != nil || duration != 1000 {
		t.Fatal("not actual canonical samples", duration, err)
	}
	if _, err = f.invoke(t, execution, "select-audio", map[string]any{"action": "select", "manifest_uris": []string{refs[0].URI}, "reason": "must refuse audio"}); err == nil {
		t.Fatal("audio entered image-only delivery selection")
	}
	if f.approvals != 2 {
		t.Fatal("actual processing/selection Guard was bypassed", f.approvals)
	}
	// The metadata itself is cold-readable, but a forged declared duration
	// must fail before any player or binary preview can consume the bytes.
	manifest.Output.DurationMillis++
	raw, _ := json.Marshal(manifest)
	spoof, err := f.repo.PutArtifact(t.Context(), "owner", f.log.ID(), storage.ArtifactInput{Kind: "media-manifest", Name: f.source.ID, MediaType: "application/json", Data: raw, Text: true, Processing: "negative fixture"})
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err = f.plugin.library.ReadDerivative(t.Context(), spoof.Reference().URI); err == nil {
		t.Fatal("forged audio duration reached preview")
	}
}

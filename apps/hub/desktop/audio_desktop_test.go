// INPUT: Real canonical WAV artifacts, headless MyGo controls and an explicit non-audible test voice
// OUTPUT: Cold reads, actual derivative verification, explicit Play/Pause/seek/navigation and output-failure regressions
// POS: [L7 Hub product] Control tests never open the user's sound device or claim acoustic acceptance
//go:build desktop

package hubdesktop

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/QuincySx/Alva/apps/hub/product"
	mediaextension "github.com/QuincySx/Alva/extension/media"
	"github.com/QuincySx/Alva/platform/storage"
	nativemedia "github.com/QuincySx/Alva/platform/system/media"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type testAudioVoice struct {
	mu              sync.Mutex
	playing, closed bool
	position        time.Duration
}

func (v *testAudioVoice) Play()                { v.mu.Lock(); defer v.mu.Unlock(); v.playing = true }
func (v *testAudioVoice) Playing() bool        { v.mu.Lock(); defer v.mu.Unlock(); return v.playing }
func (v *testAudioVoice) Seek(p time.Duration) { v.mu.Lock(); defer v.mu.Unlock(); v.position = p }
func (v *testAudioVoice) Position() time.Duration {
	v.mu.Lock()
	defer v.mu.Unlock()
	return v.position
}
func (v *testAudioVoice) Close() { v.mu.Lock(); defer v.mu.Unlock(); v.closed, v.playing = true, false }

func canonicalTestWAV() []byte {
	data := make([]byte, 44+32000*2)
	copy(data, "RIFF")
	binary.LittleEndian.PutUint32(data[4:], uint32(len(data)-8))
	copy(data[8:], "WAVEfmt ")
	binary.LittleEndian.PutUint32(data[16:], 16)
	binary.LittleEndian.PutUint16(data[20:], 1)
	binary.LittleEndian.PutUint16(data[22:], 1)
	binary.LittleEndian.PutUint32(data[24:], 16000)
	binary.LittleEndian.PutUint32(data[28:], 32000)
	binary.LittleEndian.PutUint16(data[32:], 2)
	binary.LittleEndian.PutUint16(data[34:], 16)
	copy(data[36:], "data")
	binary.LittleEndian.PutUint32(data[40:], uint32(len(data)-44))
	return data
}

func TestNativeAudioIsExplicitSeekableClosedAndNeverModelInput(t *testing.T) {
	service, requests := fixture(t)
	s, err := service.Create(t.Context(), "audio", "")
	if err != nil {
		t.Fatal(err)
	}
	data := canonicalTestWAV()
	upload, err := service.UploadReader(t.Context(), s.ID, "sound.wav", bytes.NewReader(data))
	if err != nil {
		t.Fatal(err)
	}
	detail, err := service.Store.Detail(t.Context(), s.ID)
	if err != nil {
		t.Fatal(err)
	}
	branch, err := product.ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	h := sha256.Sum256(data)
	digest := hex.EncodeToString(h[:])
	out := nativemedia.Output{Data: data, MediaType: "audio/wav", Version: "canonical test-only PCM", DurationMillis: 2000, Codec: "pcm_s16le", SampleRate: 16000, Channels: 1, Operation: wire.MediaOperation{Kind: "transcode_audio", Description: "test fixture; no playback", InputSHA256: upload.Info.SHA256, OutputSHA256: digest}, Losses: []string{"fixture mono samples"}}
	artifact, err := service.Repository.PutArtifact(t.Context(), s.ID, branch.NativeSessionID, storage.ArtifactInput{Kind: "media-derivative", Name: upload.Info.ID, MediaType: out.MediaType, Data: data, Processing: out.Operation.Description})
	if err != nil {
		t.Fatal(err)
	}
	manifest := mediaextension.Manifest{Version: 1, Source: upload.Reference, Artifact: artifact.Reference(), Output: out, Processing: "verified PCM fixture, no model delivery"}
	raw, _ := json.Marshal(manifest)
	entry, err := service.Repository.PutArtifact(t.Context(), s.ID, branch.NativeSessionID, storage.ArtifactInput{Kind: "media-manifest", Name: upload.Info.ID, MediaType: "application/json", Data: raw, Text: true, Processing: manifest.Processing})
	if err != nil {
		t.Fatal(err)
	}
	foreign, err := service.Create(t.Context(), "foreign", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err = service.AudioPlayback(t.Context(), foreign.ID, branch.ID, entry.Reference().URI); err == nil {
		t.Fatal("foreign product read audio")
	}
	if _, _, err = service.AudioPlayback(t.Context(), s.ID, branch.ID, upload.Reference.URI); err == nil {
		t.Fatal("original accepted as playable derivative")
	}
	p := NewNativePage(service)
	p.selected, p.detail = s.ID, detail
	p.applyDetail()
	voices := []*testAudioVoice{}
	p.audioVoice = func(samples []float32) audioVoice {
		if len(samples) != 32000 {
			t.Error("invalid sample count", len(samples))
		}
		v := &testAudioVoice{}
		voices = append(voices, v)
		return v
	}
	p.audioOutputError = func() error { return nil }
	tester := ui.NewTester(p.Build, 1600, 2000)
	for _, label := range []string{"Media evidence", "List derived media for sound.wav", "Read processing manifest"} {
		if err = tester.Click(label); err != nil {
			t.Fatal(err)
		}
		settlePage(t, tester, p)
	}
	if len(voices) != 0 || p.playerBusy || p.media.PlaybackKind != "audio" {
		t.Fatal("metadata autoplayed or did not expose explicit audio")
	}
	if err = tester.Click("Play selected audio derivative"); err != nil {
		t.Fatal(err)
	}
	awaitPlayback(t, tester, p, func() bool { return p.player != nil && p.playerFrames > 0 })
	first := p.player
	if p.media.Bitmap != nil || p.media.PreviewBytes != len(data) {
		t.Fatal("audio became image or bypassed actual artifact read")
	}
	if err = tester.Click("Pause and close local audio"); err != nil {
		t.Fatal(err)
	}
	awaitPlayback(t, tester, p, func() bool { return !p.playerBusy })
	if !first.CleanupComplete() || voices[0].Playing() {
		t.Fatal("Pause retained voice")
	}
	if err = tester.Click("Local audio seek milliseconds"); err != nil {
		t.Fatal(err)
	}
	tester.Key(ui.Cmd, ui.KeyA)
	tester.Type("1000")
	if err = tester.Click("Play selected audio derivative"); err != nil {
		t.Fatal(err)
	}
	awaitPlayback(t, tester, p, func() bool { return p.player != nil && p.playerPosition == 1000 })
	second := p.player
	if err = tester.Click("Chat"); err != nil {
		t.Fatal(err)
	}
	awaitPlayback(t, tester, p, func() bool { return !p.playerBusy })
	if !second.CleanupComplete() || service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("navigation leaked audio or invoked model")
	}
}

func TestAudioFailureDoesNotClaimAudibleSuccessAndCloseJoinsObserver(t *testing.T) {
	voice := &testAudioVoice{}
	p, err := newAudioPlayback(canonicalTestWAV(), 2000, func([]float32) audioVoice { return voice }, func() error { return errors.New("no output") })
	if err != nil {
		t.Fatal(err)
	}
	if err = p.Run(t.Context(), 0, func(context.Context, int64, int64) error { t.Fatal("failed output emitted success"); return nil }); err == nil || !p.CleanupComplete() {
		t.Fatal("silent device fallback reported success", err)
	}
	if err = p.Close(t.Context()); err != nil || voice.Playing() {
		t.Fatal("failed voice retained", err)
	}
	if _, err = newAudioPlayback(canonicalTestWAV(), 1999, nil, nil); err == nil {
		t.Fatal("forged duration accepted")
	}
	// The real default backend can be constructed and closed cold; it must
	// never need/open a sound device until explicit Play.
	cold, err := newAudioPlayback(canonicalTestWAV(), 2000, nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	if err = cold.Close(t.Context()); err != nil || !cold.CleanupComplete() {
		t.Fatal(err)
	}
}

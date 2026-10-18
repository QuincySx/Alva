// INPUT: Explicit approved decoder, bounded uploaded audio/video and canonical PCM derivative bytes
// OUTPUT: TranscodeAudio and strict canonical WAV validation with actual sample-derived duration
// POS: [platform / OS primitive] Bounded local derivation only; no audio device, transcription or model delivery
package media

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"

	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

const AudioSampleRate = 16000
const MaxAudioOutput = 44 + AudioSampleRate*2*60

// ValidateAudio accepts only the exact 44-byte WAV envelope produced here,
// never arbitrary RIFF metadata, compressed codecs, extra chunks or trailing
// data. Duration is rounded from the actual mono 16-bit sample count.
func ValidateAudio(data []byte) (int64, error) {
	if len(data) < 76 || len(data) > MaxAudioOutput || len(data)%2 != 0 {
		return 0, errors.New("audio derivative requires 1..60000ms canonical mono 16kHz PCM WAV")
	}
	n := len(data) - 44
	want := audioHeader(n)
	if !bytes.Equal(data[:44], want[:]) {
		return 0, errors.New("audio derivative is not the canonical bounded PCM envelope")
	}
	return (int64(n)*1000 + AudioSampleRate) / (AudioSampleRate * 2), nil
}

func audioWAV(pcm []byte) []byte {
	out := make([]byte, 44+len(pcm))
	header := audioHeader(len(pcm))
	copy(out, header[:])
	copy(out[44:], pcm)
	return out
}

func audioHeader(size int) (out [44]byte) {
	copy(out[:], "RIFF")
	binary.LittleEndian.PutUint32(out[4:], uint32(size+36))
	copy(out[8:], "WAVEfmt ")
	binary.LittleEndian.PutUint32(out[16:], 16)
	binary.LittleEndian.PutUint16(out[20:], 1)
	binary.LittleEndian.PutUint16(out[22:], 1)
	binary.LittleEndian.PutUint32(out[24:], AudioSampleRate)
	binary.LittleEndian.PutUint32(out[28:], AudioSampleRate*2)
	binary.LittleEndian.PutUint16(out[32:], 2)
	binary.LittleEndian.PutUint16(out[34:], 16)
	copy(out[36:], "data")
	binary.LittleEndian.PutUint32(out[40:], uint32(size))
	return
}

// TranscodeAudio decodes just the first audio stream, to a fixed raw PCM
// format, then constructs the WAV envelope itself. It does not trust muxer
// duration tags or expose a general decoder argument / protocol surface.
func (p *Processor) TranscodeAudio(ctx context.Context, input []byte, mediaType string) (Output, error) {
	end, err := p.begin(ctx)
	if err != nil {
		return Output{}, err
	}
	defer end()
	probe, err := p.probe(ctx, input, mediaType)
	if err != nil {
		return Output{}, err
	}
	if probe.AudioStreams < 1 {
		return Output{}, errors.New("selected source has no audio stream")
	}
	version, err := p.version(ctx)
	if err != nil {
		return Output{}, err
	}
	args, err := decoderInput(mediaType)
	if err != nil {
		return Output{}, err
	}
	args = append([]string{"-nostdin", "-hide_banner", "-v", "error", "-threads", "1", "-filter_threads", "1"}, args...)
	args = append(args, "-map", "0:a:0", "-vn", "-sn", "-dn", "-t", "60", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", "-f", "s16le", "pipe:1")
	pcm, _, err := p.run(ctx, p.config.FFmpeg, args, input, MaxAudioOutput-44)
	if err != nil {
		return Output{}, err
	}
	data := audioWAV(pcm)
	duration, err := ValidateAudio(data)
	if err != nil {
		return Output{}, err
	}
	return Output{Data: data, MediaType: "audio/wav", Version: version, DurationMillis: duration, Codec: "pcm_s16le", SampleRate: AudioSampleRate, Channels: 1,
		Operation: wire.MediaOperation{Kind: "transcode_audio", Description: "First audio stream, at most its first 60 seconds, downmixed to mono and resampled to 16kHz signed 16-bit PCM in a canonical WAV envelope. Duration comes from actual output samples. No transcription, model delivery or playback.", InputSHA256: hash(input), OutputSHA256: hash(data), Parameters: []byte(`{"maximum_duration_millis":60000,"audio_stream":0,"sample_rate":16000,"channels":1,"sample_bits":16,"duration_source":"actual_pcm_samples"}`)},
		Losses:    []string{"Only the first audio stream and at most its first 60 seconds are retained; video, subtitles, extra streams and metadata are removed.", "Mono downmix, 16kHz resampling and 16-bit quantization lose spatial and frequency detail; no audio quality, loudness normalization or privacy-scrub guarantee.", "Sample-derived duration is rounded to milliseconds. No transcription, sound recognition or semantic understanding was performed."}}, nil
}

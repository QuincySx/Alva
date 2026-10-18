// INPUT: Explicit bounded uploaded bytes, context and standard-library image codecs
// OUTPUT: ProbeImage and ProcessImage with truthful format/dimension/hash/quality provenance
// POS: [platform / OS primitive] Native deterministic image derivative without filesystem discovery, model delivery or original mutation
package media

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"image"
	"image/color"
	_ "image/gif"
	"image/jpeg"
	_ "image/png"
	"runtime"

	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

const MaxInput = 20 << 20
const MaxPixels = 16_000_000
const MaxEdge = 8192
const MaxImageOutput = 256 << 10

var processingJobs = make(chan struct{}, 2)

func beginProcessing(ctx context.Context) (func(), error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	select {
	case processingJobs <- struct{}{}:
		return func() { <-processingJobs }, nil
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

type Probe struct {
	MediaType      string `json:"media_type"`
	Width          int    `json:"width"`
	Height         int    `json:"height"`
	DurationMillis int64  `json:"duration_millis,omitempty"`
	Codec          string `json:"codec,omitempty"`
	FrameRate      string `json:"frame_rate,omitempty"`
	AudioStreams   int    `json:"audio_streams"`
	SampleRate     int    `json:"sample_rate,omitempty"`
	Channels       int    `json:"channels,omitempty"`
	Processing     string `json:"processing"`
}

type Output struct {
	Data            []byte              `json:"-"`
	MediaType       string              `json:"media_type"`
	Width           int                 `json:"width"`
	Height          int                 `json:"height"`
	Version         string              `json:"version"`
	Operation       wire.MediaOperation `json:"operation"`
	Losses          []string            `json:"losses"`
	RequestedMillis *int64              `json:"requested_millis,omitempty"`
	ActualMillis    *int64              `json:"actual_millis,omitempty"`
	DurationMillis  int64               `json:"duration_millis,omitempty"`
	Codec           string              `json:"codec,omitempty"`
	FrameRate       string              `json:"frame_rate,omitempty"`
	SampleRate      int                 `json:"sample_rate,omitempty"`
	Channels        int                 `json:"channels,omitempty"`
}

func hash(data []byte) string { v := sha256.Sum256(data); return hex.EncodeToString(v[:]) }

func checkDimensions(w, h int) error {
	if w < 1 || h < 1 || w > MaxEdge || h > MaxEdge || int64(w)*int64(h) > MaxPixels {
		return errors.New("media source exceeds 8192px edge or 16 megapixel decode budget")
	}
	return nil
}

func ProbeImage(ctx context.Context, data []byte, mediaType string) (Probe, error) {
	if err := ctx.Err(); err != nil {
		return Probe{}, err
	}
	if len(data) == 0 || len(data) > MaxInput {
		return Probe{}, errors.New("media requires 1..20 MiB explicitly selected original bytes")
	}
	config, format, err := image.DecodeConfig(bytes.NewReader(data))
	if err != nil {
		return Probe{}, errors.New("unsupported or invalid PNG/JPEG/GIF image header; no media understanding performed")
	}
	actual := map[string]string{"png": "image/png", "jpeg": "image/jpeg", "gif": "image/gif"}[format]
	if actual == "" || actual != mediaType {
		return Probe{}, errors.New("declared media type does not match decoded image format")
	}
	if err = checkDimensions(config.Width, config.Height); err != nil {
		return Probe{}, err
	}
	return Probe{MediaType: actual, Width: config.Width, Height: config.Height, Processing: "Decoded image header only. No semantic understanding, animation detection, compression or model delivery."}, nil
}

type boundedImageWriter struct{ data []byte }

func (w *boundedImageWriter) Write(data []byte) (int, error) {
	if len(w.data)+len(data) > MaxImageOutput {
		return 0, errors.New("JPEG derivative exceeds 256 KiB output limit")
	}
	w.data = append(w.data, data...)
	return len(data), nil
}

func ProcessImage(ctx context.Context, data []byte, mediaType string) (Output, error) {
	end, err := beginProcessing(ctx)
	if err != nil {
		return Output{}, err
	}
	defer end()
	probe, err := ProbeImage(ctx, data, mediaType)
	if err != nil {
		return Output{}, err
	}
	source, format, err := image.Decode(bytes.NewReader(data))
	if err != nil {
		return Output{}, errors.New("image pixel decode failed; no derivative completed")
	}
	if err = ctx.Err(); err != nil {
		return Output{}, err
	}
	bounds := source.Bounds()
	if err = checkDimensions(bounds.Dx(), bounds.Dy()); err != nil {
		return Output{}, err
	}
	if bounds.Dx() != probe.Width || bounds.Dy() != probe.Height {
		return Output{}, errors.New("decoded image dimensions differ from checked header")
	}
	w, h := probe.Width, probe.Height
	if w > 512 || h > 512 {
		if w >= h {
			h = max(1, h*512/w)
			w = 512
		} else {
			w = max(1, w*512/h)
			h = 512
		}
	}
	result := image.NewRGBA(image.Rect(0, 0, w, h))
	// Nearest source pixel, with explicit white alpha compositing. This is not
	// a claim of colour-managed or high-quality photographic resampling.
	for y := 0; y < h; y++ {
		if err = ctx.Err(); err != nil {
			return Output{}, err
		}
		for x := 0; x < w; x++ {
			r, g, b, a := source.At(bounds.Min.X+x*probe.Width/w, bounds.Min.Y+y*probe.Height/h).RGBA()
			result.SetRGBA(x, y, color.RGBA{R: uint8((r + 65535 - a) >> 8), G: uint8((g + 65535 - a) >> 8), B: uint8((b + 65535 - a) >> 8), A: 255})
		}
	}
	var writer boundedImageWriter
	if err = jpeg.Encode(&writer, result, &jpeg.Options{Quality: 75}); err != nil {
		return Output{}, err
	}
	if err = ctx.Err(); err != nil {
		return Output{}, err
	}
	operation := wire.MediaOperation{Kind: "process_image", Description: "First decoded " + format + " image only, nearest-neighbour fit within 512px without cropping/upscaling, transparent pixels composited on white, lossy JPEG quality 75; not delivered to a model.", InputSHA256: hash(data), OutputSHA256: hash(writer.data), Parameters: []byte(`{"maximum_edge":512,"jpeg_quality":75,"resampler":"nearest","alpha_background":"white","crop":false,"first_decoded_image_only":true}`)}
	return Output{Data: writer.data, MediaType: "image/jpeg", Width: w, Height: h, Version: runtime.Version() + " standard-library image/jpeg", Operation: operation, Losses: []string{"Lossy JPEG re-encoding; output bytes are not guaranteed smaller than the original.", "Only the first decoded image is retained; animation has not been detected or preserved.", "Nearest-neighbour resize may lose fine detail; transparency becomes white. No colour/EXIF/ICC metadata fidelity or privacy-scrub guarantee."}}, nil
}

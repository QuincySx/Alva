// INPUT: Locally generated bounded PNG/GIF/JPEG bytes and explicit cancellation/header corruption
// OUTPUT: Real derivative dimensions, hashes, white alpha, first-frame and failure-budget regressions
// POS: [platform / OS primitive] Image processor acceptance does not require a model, user file or native window
package media

import (
	"bytes"
	"context"
	"errors"
	"image"
	"image/color"
	"image/gif"
	"image/jpeg"
	"image/png"
	"testing"
)

func TestImageDerivePreservesOriginalAndHonestLosses(t *testing.T) {
	im := image.NewNRGBA(image.Rect(0, 0, 1024, 512))
	im.Set(0, 0, color.NRGBA{R: 255, A: 255})
	var raw bytes.Buffer
	if err := png.Encode(&raw, im); err != nil {
		t.Fatal(err)
	}
	original := append([]byte(nil), raw.Bytes()...)
	probe, err := ProbeImage(t.Context(), original, "image/png")
	if err != nil || probe.Width != 1024 {
		t.Fatal(probe, err)
	}
	result, err := ProcessImage(t.Context(), original, "image/png")
	if err != nil {
		t.Fatal(err)
	}
	decoded, err := jpeg.Decode(bytes.NewReader(result.Data))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(raw.Bytes(), original) || result.Width != 512 || result.Height != 256 || decoded.Bounds().Dx() != 512 || len(result.Data) > MaxImageOutput || result.Operation.InputSHA256 != hash(original) || result.Operation.OutputSHA256 != hash(result.Data) || len(result.Losses) < 3 {
		t.Fatal("incorrect derivative/provenance", result)
	}
	r, g, b, _ := decoded.At(500, 200).RGBA()
	if r < 64000 || g < 64000 || b < 64000 {
		t.Fatal("transparent pixel not white")
	}
}

func TestAnimatedGIFFirstDecodedFrameIsExplicit(t *testing.T) {
	palette := color.Palette{color.Black, color.White}
	first := image.NewPaletted(image.Rect(0, 0, 8, 8), palette)
	second := image.NewPaletted(first.Bounds(), palette)
	for i := range second.Pix {
		second.Pix[i] = 1
	}
	var raw bytes.Buffer
	if err := gif.EncodeAll(&raw, &gif.GIF{Image: []*image.Paletted{first, second}, Delay: []int{10, 10}}); err != nil {
		t.Fatal(err)
	}
	result, err := ProcessImage(t.Context(), raw.Bytes(), "image/gif")
	if err != nil {
		t.Fatal(err)
	}
	decoded, err := jpeg.Decode(bytes.NewReader(result.Data))
	if err != nil {
		t.Fatal(err)
	}
	r, _, _, _ := decoded.At(4, 4).RGBA()
	if r > 1000 || result.Operation.Kind != "process_image" {
		t.Fatal("not first image")
	}
}

func TestImageBudgetsAndCancellationAreFailuresNotEmptySuccess(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := ProcessImage(ctx, []byte("anything"), "image/png"); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	for _, data := range [][]byte{nil, []byte("corrupt"), make([]byte, MaxInput+1)} {
		if _, err := ProcessImage(t.Context(), data, "image/png"); err == nil {
			t.Fatal("bad input accepted")
		}
	}
	for _, dims := range [][2]int{{0, 2}, {8193, 1}, {8000, 8000}} {
		if checkDimensions(dims[0], dims[1]) == nil {
			t.Fatal("pixel/edge budget bypass")
		}
	}
	var raw bytes.Buffer
	_ = png.Encode(&raw, image.NewGray(image.Rect(0, 0, 1, 1)))
	if _, err := ProcessImage(t.Context(), raw.Bytes(), "image/jpeg"); err == nil {
		t.Fatal("false media type")
	}
}

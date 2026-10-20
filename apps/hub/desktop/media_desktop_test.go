// INPUT: MyGo headless controls, actual immutable image artifacts and product decoder settings
// OUTPUT: Cold metadata navigation, explicit verified JPEG preview/release and no-start preference regressions
// POS: [L7 Hub product] Native control tests do not claim OS window acceptance or model understanding
//go:build desktop

package hubdesktop

import (
	"bytes"
	"context"
	"encoding/json"
	"image"
	"image/png"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"

	"github.com/QuincySx/Alva/apps/hub/product"
	mediaextension "github.com/QuincySx/Alva/extension/media"
	"github.com/QuincySx/Alva/platform/storage"
	nativemedia "github.com/QuincySx/Alva/platform/system/media"
)

func TestNativeMediaMetadataIsColdAndJPEGPreviewIsExplicit(t *testing.T) {
	service, requests := fixture(t)
	ctx := context.Background()
	s, err := service.Create(ctx, "cold media", "")
	if err != nil {
		t.Fatal(err)
	}
	var raw bytes.Buffer
	if err = png.Encode(&raw, image.NewRGBA(image.Rect(0, 0, 17, 9))); err != nil {
		t.Fatal(err)
	}
	upload, err := service.UploadReader(ctx, s.ID, "actual.png", bytes.NewReader(raw.Bytes()))
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
	output, err := nativemedia.ProcessImage(ctx, raw.Bytes(), "image/png")
	if err != nil {
		t.Fatal(err)
	}
	derived, err := service.Repository.PutArtifact(ctx, s.ID, branch.NativeSessionID, storage.ArtifactInput{Kind: "media-derivative", Name: upload.Info.ID, MediaType: output.MediaType, Data: output.Data, Processing: output.Operation.Description})
	if err != nil {
		t.Fatal(err)
	}
	manifest := mediaextension.Manifest{Version: 1, Source: upload.Reference, Artifact: derived.Reference(), Output: output, Processing: "actual local processing fixture; no model delivery"}
	data, _ := json.Marshal(manifest)
	_, err = service.Repository.PutArtifact(ctx, s.ID, branch.NativeSessionID, storage.ArtifactInput{Kind: "media-manifest", Name: upload.Info.ID, MediaType: "application/json", Text: true, Data: data, Processing: "actual image provenance only"})
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	page.selected, page.detail = s.ID, detail
	page.applyDetail()
	tester := ui.NewTester(page.Build, 1600, 1600)
	if err = tester.Click("Media evidence"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" || len(page.media.Page.Entries) != 1 || page.media.JSON != "" || page.media.Bitmap != nil || page.media.PreviewBytes != 0 {
		t.Fatal("opening media hydrated body", page.problem)
	}
	if err = tester.Click("List derived media for actual.png"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" || len(page.media.Page.Entries) != 1 || page.media.JSON != "" || page.media.Bitmap != nil {
		t.Fatal("derived index hydrated media", page.problem)
	}
	if err = tester.Click("Read processing manifest"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" || page.media.JSON == "" || page.media.Bitmap != nil || page.media.PreviewBytes != 0 {
		t.Fatal("manifest read decoded media", page.problem)
	}
	if err = tester.Click("Preview verified derivative"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" || page.media.Bitmap == nil || page.media.PreviewBytes != len(output.Data) {
		t.Fatal("explicit real JPEG not drawn", page.problem)
	}
	w, h := page.media.Bitmap.Size()
	if w != 17 || h != 9 {
		t.Fatal("preview dimensions", w, h)
	}
	if err = tester.Click("Chat"); err != nil {
		t.Fatal(err)
	}
	if page.media.Bitmap != nil || page.media.JSON != "" || page.media.PreviewBytes != 0 {
		t.Fatal("hidden pane retained media preview")
	}
	if service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("evidence read started execution")
	}
}

func TestNativeMediaSettingsSaveDisabledAndRequireIndependentDecoderConsent(t *testing.T) {
	service, requests := fixture(t)
	s, err := service.Create(context.Background(), "media preferences", "")
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	page.selected = s.ID
	page.detail, err = service.Store.Detail(context.Background(), s.ID)
	if err != nil {
		t.Fatal(err)
	}
	page.applyDetail()
	tester := ui.NewTester(page.Build, 1600, 1800)
	if err = tester.Click("Session settings"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if err = tester.Click("Media tools and explicit decoder approval"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.settings.MediaEnabled {
		t.Fatal("media enabled by opening preferences")
	}
	if err = tester.Click("Enable explicit media tools next turn"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	for _, field := range []struct{ label, path string }{
		{"Explicit ffmpeg absolute path", filepath.Join(t.TempDir(), "not-started-ffmpeg")},
		{"Explicit ffprobe absolute path", filepath.Join(t.TempDir(), "not-started-ffprobe")},
	} {
		if err = tester.Click(field.label); err != nil {
			t.Fatal(err)
		}
		tester.Type(field.path)
	}
	if err = tester.Click("Save decoder paths disabled"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" || !page.settings.MediaEnabled || page.settings.MediaDecoder.Enabled || page.settings.MediaDecoder.HostAccessApproved {
		t.Fatal("decoder save self-approved", page.problem)
	}
	if err = tester.Click("Approve full host decoder access next turn"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" || !page.settings.MediaDecoder.Enabled || !page.settings.MediaDecoder.HostAccessApproved {
		t.Fatal("separate host consent not saved", page.problem)
	}
	if err = tester.Click("Save decoder paths disabled"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" || page.settings.MediaDecoder.Enabled || page.settings.MediaDecoder.HostAccessApproved {
		t.Fatal("decoder edits retained prior approval", page.problem)
	}
	if service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("preference save/approve executed work")
	}
}

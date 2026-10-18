// INPUT: Authenticated owned branch IDs and explicit derivative/selection artifact references
// OUTPUT: Cold media indices, manifests, fixed-high-water receipts and hash-verified binary previews
// POS: [L7 Hub product] HTTP evidence transport does not process media, start a model or resubmit selections
package server

import (
	"bytes"
	"net/http"
	"time"
)

func (s *Server) media(w http.ResponseWriter, r *http.Request, id, action string) {
	if r.Method != http.MethodGet {
		fail(w, 405)
		return
	}
	var fields []string
	switch action {
	case "sources", "selections":
		fields = []string{"branch", "after", "high_water"}
	case "selection", "receipt":
		fields = []string{"branch", "uri"}
	case "artifacts":
		fields = []string{"branch", "source_uri", "after", "high_water"}
	case "manifest", "preview":
		fields = []string{"branch", "manifest_uri"}
	case "receipts":
		fields = []string{"branch", "selection_id", "after", "high_water"}
	default:
		fail(w, 404)
		return
	}
	q, err := query(r, fields...)
	if err != nil || !validID(q.Get("branch")) || len(q.Get("source_uri")) > 2048 || len(q.Get("manifest_uri")) > 2048 || len(q.Get("selection_id")) > 32 || len(q.Get("uri")) > 2048 {
		fail(w, 400)
		return
	}
	after, err := integer(q, "after", 0, 0, 1<<63-1)
	if err != nil {
		fail(w, 400)
		return
	}
	high, err := integer(q, "high_water", 0, 0, 1<<63-1)
	if err != nil {
		fail(w, 400)
		return
	}
	switch action {
	case "sources":
		value, err := s.service.MediaSources(r.Context(), id, q.Get("branch"), uint64(after), uint64(high))
		reply(w, value, err)
	case "selections":
		value, err := s.service.MediaSelections(r.Context(), id, q.Get("branch"), uint64(after), uint64(high))
		reply(w, value, err)
	case "selection":
		value, err := s.service.MediaSelection(r.Context(), id, q.Get("branch"), q.Get("uri"))
		reply(w, value, err)
	case "receipt":
		value, err := s.service.MediaReceipt(r.Context(), id, q.Get("branch"), q.Get("uri"))
		reply(w, value, err)
	case "artifacts":
		value, err := s.service.MediaIndex(r.Context(), id, q.Get("branch"), q.Get("source_uri"), uint64(after), uint64(high))
		reply(w, value, err)
	case "manifest":
		value, err := s.service.MediaManifest(r.Context(), id, q.Get("branch"), q.Get("manifest_uri"))
		reply(w, value, err)
	case "receipts":
		value, err := s.service.MediaReceipts(r.Context(), id, q.Get("branch"), q.Get("selection_id"), uint64(after), uint64(high))
		reply(w, value, err)
	case "preview":
		manifest, data, err := s.service.MediaPreview(r.Context(), id, q.Get("branch"), q.Get("manifest_uri"))
		if err != nil {
			operationError(w, err)
			return
		}
		name := "derived-image.jpg"
		if manifest.Artifact.MediaType == "video/mp4" {
			name = "derived-video.mp4"
		} else if manifest.Artifact.MediaType == "audio/wav" {
			name = "derived-audio.wav"
		}
		w.Header().Set("Content-Type", manifest.Artifact.MediaType)
		w.Header().Set("Content-Disposition", `inline; filename="`+name+`"`)
		w.Header().Set("X-Content-SHA256", manifest.Artifact.SHA256)
		// Binary, not base64 JSON; the full bounded immutable derivative was
		// hash-verified before ranges/conditional HTTP handling can return it.
		http.ServeContent(w, r, name, time.Time{}, bytes.NewReader(data))
	}
}

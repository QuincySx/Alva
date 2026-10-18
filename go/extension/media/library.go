// INPUT: Actual native repository authority, immutable original/derivative BLOBs and explicit processing outputs, platform/system/httpjson.
// OUTPUT: Library bounded metadata/manifests, complete verified derivatives and immutable selection/receipt events
// POS: [L4 extension] Media evidence reuses native artifacts without a second transcript, process discovery or model replay
package media

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"image/jpeg"
	"io"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/platform/storage/session"
	"github.com/QuincySx/Alva/go/platform/system/httpjson"
	nativemedia "github.com/QuincySx/Alva/go/platform/system/media"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type Library struct {
	repository *session.Repository
	log        *session.Session
	owner, id  string
}
type Manifest struct {
	Version    int                `json:"version"`
	Source     wire.Reference     `json:"source"`
	Artifact   wire.Reference     `json:"artifact"`
	Output     nativemedia.Output `json:"output"`
	Processing string             `json:"processing"`
}
type Selection struct {
	ID         string     `json:"id"`
	RunID      string     `json:"run_id"`
	CallID     string     `json:"call_id"`
	Route      string     `json:"route"`
	Reason     string     `json:"reason"`
	Manifests  []Manifest `json:"manifests"`
	Processing string     `json:"processing"`
}
type Receipt struct {
	Version     int       `json:"version"`
	SelectionID string    `json:"selection_id"`
	RunID       string    `json:"run_id"`
	State       string    `json:"state"`
	Time        time.Time `json:"time"`
	ProviderID  string    `json:"provider_id,omitempty"`
	ModelID     string    `json:"model_id,omitempty"`
	Processing  string    `json:"processing"`
}

func NewLibrary(ctx context.Context, repository *session.Repository, owner, id string) (*Library, error) {
	if repository == nil || owner == "" || id == "" {
		return nil, errors.New("actual native media authority required")
	}
	log, err := repository.OpenSession(ctx, owner, id)
	if err != nil {
		return nil, err
	}
	return &Library{repository: repository, log: log, owner: owner, id: id}, nil
}
func (l *Library) source(ctx context.Context, uri string) (session.ArtifactInfo, error) {
	info, err := l.repository.ArtifactMetadata(ctx, l.owner, l.id, uri)
	if err != nil {
		return session.ArtifactInfo{}, err
	}
	if info.Kind != "input-attachment" {
		return session.ArtifactInfo{}, errors.New("media source must be an original upload in this actual native session")
	}
	return info, nil
}
func (l *Library) Original(ctx context.Context, uri string) (session.ArtifactInfo, []byte, error) {
	info, err := l.source(ctx, uri)
	if err != nil {
		return session.ArtifactInfo{}, nil, err
	}
	_, data, err := l.repository.ReadArtifactFull(ctx, l.owner, l.id, uri, session.MaxArtifactBytes)
	return info, data, err
}

func (l *Library) ListArtifacts(ctx context.Context, sourceURI string, after, highWater uint64) (session.ArtifactIndexPage, error) {
	source, err := l.source(ctx, sourceURI)
	if err != nil {
		return session.ArtifactIndexPage{}, err
	}
	return l.repository.PageArtifacts(ctx, l.owner, l.id, "media-manifest", source.ID, after, highWater, 4)
}

func (l *Library) Sources(ctx context.Context, after, highWater uint64) (session.ArtifactIndexPage, error) {
	return l.repository.PageArtifacts(ctx, l.owner, l.id, "input-attachment", "", after, highWater, 4)
}
func (l *Library) Selections(ctx context.Context, after, highWater uint64) (session.ArtifactIndexPage, error) {
	return l.repository.PageArtifacts(ctx, l.owner, l.id, "media-selection", "", after, highWater, 4)
}

func (l *Library) ReadSelection(ctx context.Context, uri string) (Selection, error) {
	info, data, err := l.repository.ReadArtifactFull(ctx, l.owner, l.id, uri, 16<<10)
	if err != nil {
		return Selection{}, err
	}
	if info.Kind != "media-selection" {
		return Selection{}, errors.New("reference is not a media selection")
	}
	var value Selection
	if err = decodeJSON(data, &value); err != nil {
		return Selection{}, err
	}
	decoded, err := hex.DecodeString(value.ID)
	if err != nil || len(decoded) != 16 || value.ID != info.Name || len(value.Manifests) < 1 || len(value.Manifests) > 2 || value.RunID == "" || value.CallID == "" || value.Route == "" || len(value.Route) > 1024 || value.Reason == "" || len(value.Reason) > 200 {
		return Selection{}, errors.New("invalid selection identity or bounds")
	}
	seen := map[string]bool{}
	for _, manifest := range value.Manifests {
		if manifest.Version != 1 || manifest.Source.Validate() != nil || manifest.Artifact.Validate() != nil || manifest.Artifact.MediaType != "image/jpeg" || manifest.Artifact.Bytes > nativemedia.MaxImageOutput || manifest.Output.Operation.InputSHA256 != manifest.Source.SHA256 || manifest.Output.Operation.OutputSHA256 != manifest.Artifact.SHA256 || seen[manifest.Artifact.URI] {
			return Selection{}, errors.New("invalid selected derivative snapshot")
		}
		seen[manifest.Artifact.URI] = true
		source, e := l.source(ctx, manifest.Source.URI)
		if e != nil {
			return Selection{}, e
		}
		derived, e := l.repository.ArtifactMetadata(ctx, l.owner, l.id, manifest.Artifact.URI)
		if e != nil {
			return Selection{}, e
		}
		if source.SHA256 != manifest.Source.SHA256 || source.Bytes != manifest.Source.Bytes || source.MediaType != manifest.Source.MediaType || derived.Kind != "media-derivative" || derived.Name != source.ID || derived.SHA256 != manifest.Artifact.SHA256 || derived.Bytes != manifest.Artifact.Bytes || derived.MediaType != manifest.Artifact.MediaType {
			return Selection{}, errors.New("selected source/derivative ownership or hash mismatch")
		}
	}
	return value, nil
}

func (l *Library) ReadReceipt(ctx context.Context, uri string) (Receipt, error) {
	info, data, err := l.repository.ReadArtifactFull(ctx, l.owner, l.id, uri, 16<<10)
	if err != nil {
		return Receipt{}, err
	}
	if info.Kind != "media-receipt" {
		return Receipt{}, errors.New("reference is not a media receipt")
	}
	var value Receipt
	if err = decodeJSON(data, &value); err != nil {
		return Receipt{}, err
	}
	if value.Version != 1 || value.SelectionID != info.Name || value.Time.IsZero() {
		return Receipt{}, errors.New("invalid receipt identity/version")
	}
	states := map[string]bool{"prepared": true, "model_chain_invocation_intent": true, "response_observed_not_proof_of_understanding": true, "request_error_or_incomplete_acceptance_unknown": true, "not_submitted_cancelled": true, "run_ended_before_submission_no_replay": true, "request_preparation_cancelled_not_submitted": true, "request_preparation_failed_not_submitted": true, "actual_model_mismatch_not_submitted": true}
	if !states[value.State] {
		return Receipt{}, errors.New("unknown receipt state")
	}
	selection, err := l.repository.PageArtifacts(ctx, l.owner, l.id, "media-selection", value.SelectionID, 0, 0, 1)
	if err != nil {
		return Receipt{}, err
	}
	if len(selection.Entries) != 1 || selection.HasMore {
		return Receipt{}, errors.New("receipt lacks unique owned selection")
	}
	selected, err := l.ReadSelection(ctx, selection.Entries[0].Info.Reference().URI)
	if err != nil {
		return Receipt{}, err
	}
	if selected.RunID != value.RunID {
		return Receipt{}, errors.New("receipt native run mismatch")
	}
	return value, nil
}

func decodeJSON(data []byte, out any) error {
	if httpjson.ValidateObject(data, out) != nil {
		return errors.New("invalid media evidence JSON")
	}
	d := json.NewDecoder(bytes.NewReader(data))
	d.DisallowUnknownFields()
	if err := d.Decode(out); err != nil {
		return errors.New("invalid media evidence shape")
	}
	if d.Decode(new(any)) != io.EOF {
		return errors.New("trailing media evidence")
	}
	return nil
}

func (l *Library) ReadManifest(ctx context.Context, uri string) (Manifest, error) {
	info, data, err := l.repository.ReadArtifactFull(ctx, l.owner, l.id, uri, 16<<10)
	if err != nil {
		return Manifest{}, err
	}
	if info.Kind != "media-manifest" {
		return Manifest{}, errors.New("selected reference is not a media manifest")
	}
	var value Manifest
	if err = decodeJSON(data, &value); err != nil {
		return Manifest{}, err
	}
	if value.Version != 1 || value.Source.Validate() != nil || value.Artifact.Validate() != nil || value.Output.Operation.InputSHA256 != value.Source.SHA256 || value.Output.Operation.OutputSHA256 != value.Artifact.SHA256 {
		return Manifest{}, errors.New("invalid media provenance")
	}
	source, err := l.source(ctx, value.Source.URI)
	if err != nil {
		return Manifest{}, err
	}
	derived, err := l.repository.ArtifactMetadata(ctx, l.owner, l.id, value.Artifact.URI)
	if err != nil {
		return Manifest{}, err
	}
	if source.ID != info.Name || source.SHA256 != value.Source.SHA256 || source.Bytes != value.Source.Bytes || source.MediaType != value.Source.MediaType || derived.Kind != "media-derivative" || derived.Name != source.ID || derived.SHA256 != value.Artifact.SHA256 || derived.Bytes != value.Artifact.Bytes || derived.MediaType != value.Artifact.MediaType || value.Output.MediaType != derived.MediaType {
		return Manifest{}, errors.New("media evidence source/derivative mismatch")
	}
	return value, nil
}

func (l *Library) ReadDerivative(ctx context.Context, manifestURI string) (Manifest, []byte, error) {
	manifest, err := l.ReadManifest(ctx, manifestURI)
	if err != nil {
		return Manifest{}, nil, err
	}
	maximum := nativemedia.MaxImageOutput
	if manifest.Artifact.MediaType == "video/mp4" {
		maximum = 8 << 20
	} else if manifest.Artifact.MediaType == "audio/wav" {
		maximum = nativemedia.MaxAudioOutput
	} else if manifest.Artifact.MediaType != "image/jpeg" {
		return Manifest{}, nil, errors.New("unsupported derivative preview type")
	}
	_, data, err := l.repository.ReadArtifactFull(ctx, l.owner, l.id, manifest.Artifact.URI, maximum)
	if err == nil && manifest.Artifact.MediaType == "image/jpeg" {
		config, e := jpeg.DecodeConfig(bytes.NewReader(data))
		if e != nil || config.Width < 1 || config.Height < 1 || config.Width > 512 || config.Height > 512 || config.Width != manifest.Output.Width || config.Height != manifest.Output.Height {
			return Manifest{}, nil, errors.New("stored JPEG did not match verified derivative dimensions")
		}
	}
	if err == nil && manifest.Artifact.MediaType == "audio/wav" {
		duration, e := nativemedia.ValidateAudio(data)
		if e != nil || manifest.Output.Operation.Kind != "transcode_audio" || manifest.Output.DurationMillis != duration || manifest.Output.Codec != "pcm_s16le" || manifest.Output.SampleRate != nativemedia.AudioSampleRate || manifest.Output.Channels != 1 || manifest.Output.Width != 0 || manifest.Output.Height != 0 {
			return Manifest{}, nil, errors.New("stored audio did not match the canonical verified derivative")
		}
	}
	return manifest, data, err
}

func (l *Library) saveOutputs(ctx context.Context, source session.ArtifactInfo, outputs []nativemedia.Output) ([]wire.Reference, error) {
	existing, err := l.repository.PageArtifacts(ctx, l.owner, l.id, "media-manifest", source.ID, 0, 0, 64)
	if err != nil {
		return nil, err
	}
	if len(outputs) == 0 || len(outputs) > 4 || existing.HasMore || len(existing.Entries)+len(outputs) > 64 {
		return nil, errors.New("source derivative manifest budget exhausted (64)")
	}
	var refs []wire.Reference
	for _, out := range outputs {
		if out.Operation.InputSHA256 != source.SHA256 || len(out.Data) == 0 {
			return nil, errors.New("processor output source mismatch or empty result")
		}
		info, err := l.repository.PutArtifact(ctx, l.owner, l.id, session.ArtifactInput{Kind: "media-derivative", Name: source.ID, MediaType: out.MediaType, Processing: out.Operation.Description, Data: out.Data})
		if err != nil {
			return nil, err
		}
		if info.SHA256 != out.Operation.OutputSHA256 {
			return nil, errors.New("processor output hash mismatch")
		}
		manifest := Manifest{Version: 1, Source: source.Reference(), Artifact: info.Reference(), Output: out, Processing: "Completed explicit derivative only. Original bytes unchanged; no model delivery or semantic understanding implied. Inspect output losses, parameters and exact source/output hashes."}
		data, err := json.Marshal(manifest)
		if err != nil {
			return nil, err
		}
		if len(data) > 16<<10 {
			return nil, errors.New("media manifest exceeds 16 KiB")
		}
		entry, err := l.repository.PutArtifact(ctx, l.owner, l.id, session.ArtifactInput{Kind: "media-manifest", Name: source.ID, MediaType: "application/json", Processing: "Complete derivative processing/provenance manifest; no media delivery", Data: data, Text: true})
		if err != nil {
			return nil, err
		}
		refs = append(refs, entry.Reference())
	}
	return refs, nil
}

func (l *Library) prepare(ctx context.Context, runID, callID, route, reason string, manifests []string) (Selection, wire.Reference, error) {
	if runID == "" || callID == "" || route == "" || len(route) > 1024 || len(reason) == 0 || len(reason) > 200 || !utf8.ValidString(reason+route) || len(manifests) < 1 || len(manifests) > 2 {
		return Selection{}, wire.Reference{}, errors.New("selection requires actual run/call/route, reason and 1..2 explicit derived JPEG manifests")
	}
	page, err := l.repository.PageArtifacts(ctx, l.owner, l.id, "media-selection", "", 0, 0, 64)
	if err != nil {
		return Selection{}, wire.Reference{}, err
	}
	if page.HasMore {
		tail, e := l.repository.PageArtifacts(ctx, l.owner, l.id, "media-selection", "", page.Next, page.HighWater, 64)
		if e != nil {
			return Selection{}, wire.Reference{}, e
		}
		if tail.HasMore || len(page.Entries)+len(tail.Entries) >= 128 {
			return Selection{}, wire.Reference{}, errors.New("native media selection budget exhausted (128)")
		}
	}
	var random [16]byte
	if _, err = rand.Read(random[:]); err != nil {
		return Selection{}, wire.Reference{}, err
	}
	value := Selection{ID: hex.EncodeToString(random[:]), RunID: runID, CallID: callID, Route: route, Reason: reason, Processing: "Prepared selection only. Not yet submitted. Only listed derived JPEGs may enter one request copy; original media, audio, video and other frames are excluded. Network acceptance and visual understanding remain unknown."}
	seen := map[string]bool{}
	for _, uri := range manifests {
		manifest, data, e := l.ReadDerivative(ctx, uri)
		if e != nil {
			return Selection{}, wire.Reference{}, e
		}
		if manifest.Artifact.MediaType != "image/jpeg" || len(data) > nativemedia.MaxImageOutput || seen[manifest.Artifact.URI] {
			return Selection{}, wire.Reference{}, errors.New("select unique <=256 KiB derived JPEGs only")
		}
		seen[manifest.Artifact.URI] = true
		value.Manifests = append(value.Manifests, manifest)
	}
	data, err := json.Marshal(value)
	if err != nil {
		return Selection{}, wire.Reference{}, err
	}
	if len(data) > 16<<10 {
		return Selection{}, wire.Reference{}, errors.New("selection provenance exceeds 16 KiB")
	}
	entry, err := l.repository.PutArtifact(ctx, l.owner, l.id, session.ArtifactInput{Kind: "media-selection", Name: value.ID, MediaType: "application/json", Processing: value.Processing, Data: data, Text: true})
	if err != nil {
		return Selection{}, wire.Reference{}, err
	}
	if _, err = l.receipt(ctx, value, "prepared", nil); err != nil {
		return Selection{}, wire.Reference{}, err
	}
	return value, entry.Reference(), nil
}

func (l *Library) receipt(ctx context.Context, selection Selection, state string, identity abi.ModelIdentity) (wire.Reference, error) {
	value := Receipt{Version: 1, SelectionID: selection.ID, RunID: selection.RunID, State: state, Time: time.Now().UTC(), Processing: "State records only this media middleware's actual progress. Prepared is not sent; model_chain_invocation_intent is durable intent before invoking the actual model, not proof the call or network acceptance happened. Response observation is not proof of visual understanding. No automatic replay."}
	if identity != nil {
		value.ProviderID = identity.ProviderID()
		value.ModelID = identity.ModelID()
	}
	data, err := json.Marshal(value)
	if err != nil {
		return wire.Reference{}, err
	}
	info, err := l.repository.PutArtifact(ctx, l.owner, l.id, session.ArtifactInput{Kind: "media-receipt", Name: selection.ID, MediaType: "application/json", Processing: value.Processing, Data: data, Text: true})
	if err != nil {
		return wire.Reference{}, err
	}
	// The exact immutable receipt reference also appears in native typed facts;
	// failure stops execution rather than silently dropping the audit edge.
	payload, _ := json.Marshal(map[string]any{"selection_id": selection.ID, "state": state, "receipt": info.Reference()})
	if _, err = l.log.AppendRecords(ctx, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "alva.media", Type: "delivery", SchemaVersion: 1, Payload: payload}}); err != nil {
		return wire.Reference{}, &abi.SessionError{Operation: "commit media receipt", SessionID: l.id, Err: err}
	}
	return info.Reference(), nil
}

func (l *Library) Receipts(ctx context.Context, selectionID string, after, highWater uint64) (session.ArtifactIndexPage, error) {
	if len(selectionID) != 32 {
		return session.ArtifactIndexPage{}, errors.New("explicit selection identity required")
	}
	owned, err := l.repository.PageArtifacts(ctx, l.owner, l.id, "media-selection", selectionID, 0, 0, 1)
	if err != nil {
		return session.ArtifactIndexPage{}, err
	}
	if len(owned.Entries) != 1 || owned.HasMore {
		return session.ArtifactIndexPage{}, errors.New("selection unavailable in this actual native session")
	}
	return l.repository.PageArtifacts(ctx, l.owner, l.id, "media-receipt", selectionID, after, highWater, 5)
}

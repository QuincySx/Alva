// INPUT: Actual native run/session, explicit image-capable route, Guard and reference-only media library
// OUTPUT: hub_media Plugin/Tool and one-request-only selected-image middleware; lifecycle ownership is in lifetime.go
// POS: [L4 extension] Optional media policy after ordinary context projection, never automatic raw-upload or history replay
package media

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"math"
	"path/filepath"
	"sync"
	"sync/atomic"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/platform/storage/session"
	nativemedia "github.com/QuincySx/Alva/go/platform/system/media"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type Config struct {
	Repository                   *session.Repository
	Owner, SessionID, Cwd, Route string
	Guard                        *security.Guard
	ImageCapable                 bool
	ProviderID, ModelID          string
	Processor                    *nativemedia.Processor
}
type Plugin struct {
	config         Config
	library        *Library
	tool           *mediaTool
	mu             sync.Mutex
	runID          string
	life           context.Context
	cancel         context.CancelFunc
	ending, closed bool
	active         int
	idle           chan struct{}
	pending        *Selection
	gate           chan struct{}
}

func New(ctx context.Context, config Config) (*Plugin, error) {
	if config.Guard == nil || !filepath.IsAbs(config.Cwd) || config.Route == "" || len(config.Route) > 1024 || !utf8.ValidString(config.Route) || (config.ImageCapable && (config.ProviderID == "" || config.ModelID == "")) {
		return nil, errors.New("media requires explicit guard, workspace and configured route")
	}
	library, err := NewLibrary(ctx, config.Repository, config.Owner, config.SessionID)
	if err != nil {
		return nil, err
	}
	p := &Plugin{config: config, library: library, gate: make(chan struct{}, 1)}
	p.tool = &mediaTool{plugin: p}
	return p, nil
}
func (*Plugin) Name() string { return "media" }
func (p *Plugin) Finalize(_ context.Context, late core.LateContext) ([]abi.Tool, error) {
	if len(late.Assembly.Middleware) == 0 || late.Assembly.Middleware[len(late.Assembly.Middleware)-1] != p.Name() {
		return nil, errors.New("media must be the innermost model middleware; refusing a chain that can replay injected payloads")
	}
	return nil, nil
}
func (p *Plugin) Tool() abi.Tool { return p.tool }
func (p *Plugin) Register(_ context.Context, r *core.Registrar) error {
	if err := r.Tool(p.tool); err != nil {
		return err
	}
	// Selected media is injected innermost; Finalize verifies this position.
	if err := r.Middleware(p, math.MaxInt); err != nil {
		return err
	}
	return r.SystemPrompt("hub_media explicitly probes or derives only this native session's uploaded artifact references. Originals are not model inputs. Read processing manifests and losses before choosing <=2 derived JPEGs for one request only; a route must explicitly permit images. Selection is not network acceptance or visual understanding. All large data, manifests and receipts stay in native references; context_read/search retrieve selected text. No automatic decoder discovery, model delivery, startup replay, hidden audio or full-video understanding.")
}

type mediaTool struct{ plugin *Plugin }

func (*mediaTool) Name() string { return "hub_media" }
func (*mediaTool) Description() string {
	return "Explicit uploaded media reference operations: probe, process_image, extract_frames, transcode_video, transcode_audio, list_artifacts, read_manifest, select, receipt, cancel_selection. Never host paths or URLs. Process creates <=512px lossy JPEG, silent <=60s/720px/5fps video or <=60s mono 16kHz PCM WAV. Audio processing does not play sound or transcribe. Select 1..2 derived JPEG manifests for ONE request only on an explicitly image-capable route. Receipts distinguish preparation/chain invocation/response and unknown acceptance; not proof of understanding."
}
func (*mediaTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["action"],"properties":{"action":{"enum":["probe","process_image","extract_frames","transcode_video","transcode_audio","list_artifacts","read_manifest","select","receipt","cancel_selection"]},"source_uri":{"type":"string","maxLength":2048},"manifest_uri":{"type":"string","maxLength":2048},"selection_id":{"type":"string","maxLength":32},"manifest_uris":{"type":"array","minItems":1,"maxItems":2,"items":{"type":"string","maxLength":2048}},"timestamps_millis":{"type":"array","minItems":1,"maxItems":4,"items":{"type":"integer","minimum":0}},"frame_count":{"type":"integer","minimum":1,"maximum":4},"reason":{"type":"string","maxLength":200},"after":{"type":"integer","minimum":0},"high_water":{"type":"integer","minimum":0}}}`)
}
func (*mediaTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ExecutionMode: abi.SerialGlobal, ManagesOwnTimeout: true}
}
func (t *mediaTool) IsReadOnly(call wire.ToolCall) bool {
	var a struct{ Action string }
	if json.Unmarshal(call.Arguments, &a) != nil {
		return false
	}
	return a.Action == "list_artifacts" || a.Action == "read_manifest" || a.Action == "receipt"
}
func (t *mediaTool) DescribeEffect(call wire.ToolCall) abi.Effect {
	if t.IsReadOnly(call) {
		return abi.Effect{Kind: abi.EffectRead, Description: "Read only the actual native session's bounded media metadata, manifest or receipt; no decoding or submission"}
	}
	return abi.Effect{Kind: abi.EffectUnknown, Description: "Explicitly process selected native media with bounded decoder access or deliver only selected derived JPEGs to the configured model route; host access/cost and full arguments require consent. Originals unchanged; not an OS decoder sandbox."}
}

type arguments struct {
	Action       string   `json:"action"`
	SourceURI    string   `json:"source_uri"`
	ManifestURI  string   `json:"manifest_uri"`
	SelectionID  string   `json:"selection_id"`
	ManifestURIs []string `json:"manifest_uris"`
	Timestamps   []int64  `json:"timestamps_millis"`
	FrameCount   int      `json:"frame_count"`
	Reason       string   `json:"reason"`
	After        uint64   `json:"after"`
	HighWater    uint64   `json:"high_water"`
}

func (t *mediaTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	p := t.plugin
	actual, ok := e.ActualTool.(*mediaTool)
	if !ok || actual != t || t != p.tool {
		return wire.ToolResult{}, errors.New("media actual tool instance mismatch")
	}
	if call.Name != t.Name() || e.CallID != call.ID || e.CallID == "" || len(call.Arguments) > 16<<10 {
		return wire.ToolResult{}, errors.New("media call identity/input budget mismatch")
	}
	if err := p.check(e); err != nil {
		return wire.ToolResult{}, err
	}
	var a arguments
	if err := decodeJSON(call.Arguments, &a); err != nil {
		return wire.ToolResult{}, err
	}
	ctx, finish, err := p.begin(ctx, e.RunID)
	if err != nil {
		return wire.ToolResult{}, err
	}
	defer finish()
	if err := p.config.Guard.Authorize(ctx, e, call, t.DescribeEffect(call)); err != nil {
		return wire.ToolResult{}, err
	}
	ctx, cancel := context.WithTimeout(ctx, 2*time.Minute)
	defer cancel()
	select {
	case p.gate <- struct{}{}:
		defer func() { <-p.gate }()
	case <-ctx.Done():
		return wire.ToolResult{}, ctx.Err()
	}
	if err := p.check(e); err != nil {
		return wire.ToolResult{}, err
	}
	var value any
	switch a.Action {
	case "list_artifacts":
		value, err = p.library.ListArtifacts(ctx, a.SourceURI, a.After, a.HighWater)
	case "read_manifest":
		_, err = p.library.ReadManifest(ctx, a.ManifestURI)
		if err == nil {
			var info session.ArtifactInfo
			info, err = p.config.Repository.ArtifactMetadata(ctx, p.config.Owner, p.config.SessionID, a.ManifestURI)
			value = info.Reference()
		}
	case "receipt":
		value, err = p.library.Receipts(ctx, a.SelectionID, a.After, a.HighWater)
	case "cancel_selection":
		p.mu.Lock()
		pending := p.pending
		if pending == nil || pending.ID != a.SelectionID {
			p.mu.Unlock()
			return wire.ToolResult{}, errors.New("selection is not pending in this active run; submitted/old selections cannot be cancelled or replayed")
		}
		p.pending = nil
		p.mu.Unlock()
		value, err = p.library.receipt(ctx, *pending, "not_submitted_cancelled", nil)
	case "select":
		if !p.config.ImageCapable {
			return wire.ToolResult{}, errors.New("configured route has not explicitly declared image capability; no media submitted")
		}
		p.mu.Lock()
		busy := p.pending != nil
		p.mu.Unlock()
		if busy {
			return wire.ToolResult{}, errors.New("cancel pending selection before preparing another")
		}
		var selection Selection
		selection, valueRef, prepareErr := p.library.prepare(ctx, e.RunID, call.ID, p.config.Route, a.Reason, a.ManifestURIs)
		err = prepareErr
		if err == nil {
			p.mu.Lock()
			p.pending = &selection
			p.mu.Unlock()
			value = map[string]any{"selection_id": selection.ID, "status": "prepared_not_submitted", "selection": valueRef}
		}
	case "probe", "process_image", "extract_frames", "transcode_video", "transcode_audio":
		var source session.ArtifactInfo
		var input []byte
		source, input, err = p.library.Original(ctx, a.SourceURI)
		if err != nil {
			break
		}
		switch a.Action {
		case "probe":
			if source.MediaType == "image/png" || source.MediaType == "image/jpeg" || source.MediaType == "image/gif" {
				value, err = nativemedia.ProbeImage(ctx, input, source.MediaType)
			} else if p.config.Processor != nil {
				value, err = p.config.Processor.Probe(ctx, input, source.MediaType)
			} else {
				err = errors.New("explicit approved native decoder is not configured; no processing performed")
			}
		case "process_image":
			var output nativemedia.Output
			if source.MediaType == "image/webp" && p.config.Processor != nil {
				output, err = p.config.Processor.ProcessImage(ctx, input, source.MediaType)
			} else {
				output, err = nativemedia.ProcessImage(ctx, input, source.MediaType)
			}
			if err == nil {
				value, err = p.library.saveOutputs(ctx, source, []nativemedia.Output{output})
			}
		case "extract_frames":
			if p.config.Processor == nil {
				err = errors.New("explicit approved video decoder is not configured")
				break
			}
			var outputs []nativemedia.Output
			outputs, err = p.config.Processor.ExtractFrames(ctx, input, source.MediaType, a.Timestamps, a.FrameCount, a.Reason)
			if err == nil {
				value, err = p.library.saveOutputs(ctx, source, outputs)
			}
		case "transcode_video", "transcode_audio":
			if p.config.Processor == nil {
				err = errors.New("explicit approved video decoder is not configured")
				break
			}
			var output nativemedia.Output
			if a.Action == "transcode_audio" {
				output, err = p.config.Processor.TranscodeAudio(ctx, input, source.MediaType)
			} else {
				output, err = p.config.Processor.TranscodeVideo(ctx, input, source.MediaType)
			}
			if err == nil {
				value, err = p.library.saveOutputs(ctx, source, []nativemedia.Output{output})
			}
		}
	default:
		err = errors.New("unknown media action")
	}
	if err != nil {
		var decoder *nativemedia.DecoderError
		if errors.As(err, &decoder) {
			cleanup, stop := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
			recordErr := p.recordDecoderFailure(cleanup, call, a.SourceURI, decoder)
			stop()
			return wire.ToolResult{}, errors.Join(err, recordErr)
		}
		return wire.ToolResult{}, err
	}
	data, err := json.Marshal(value)
	if err != nil {
		return wire.ToolResult{}, err
	}
	if len(data) <= 4<<10 {
		return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(data)}}}, nil
	}
	refs, err := p.config.Repository.BoundReferences(ctx, p.config.Owner, p.config.SessionID)
	if err != nil {
		return wire.ToolResult{}, err
	}
	reference, err := refs.StoreText(ctx, string(data))
	if err != nil {
		return wire.ToolResult{}, err
	}
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentReference, Reference: &reference}}}, nil
}

func (p *Plugin) recordDecoderFailure(ctx context.Context, call wire.ToolCall, sourceURI string, decoder *nativemedia.DecoderError) error {
	frame := map[string]any{"base64": base64.StdEncoding.EncodeToString(decoder.Log), "complete": false}
	raw, err := json.Marshal(frame)
	if err != nil {
		return err
	}
	raw, err = session.RedactStreamFrame(raw)
	if err != nil {
		return err
	}
	info, err := p.config.Repository.PutArtifact(ctx, p.config.Owner, p.config.SessionID, session.ArtifactInput{Kind: "media-decoder-log", Name: call.ID, MediaType: "application/json", Processing: "Bounded decoder diagnostic stream; operation failed/cancelled/limited, output completeness unknown. Known credential markers redacted; no semantic understanding or model delivery.", Data: raw, Text: true})
	if err != nil {
		return &abi.SessionError{Operation: "retain media decoder diagnostics", SessionID: p.config.SessionID, Err: err}
	}
	payload, _ := json.Marshal(map[string]any{"call_id": call.ID, "source_uri": sourceURI, "diagnostics": info.Reference(), "status": "failed_or_interrupted_no_complete_derivative_claim"})
	_, err = p.library.log.AppendRecords(ctx, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "alva.media", Type: "processing_failure", SchemaVersion: 1, Payload: payload}})
	if err != nil {
		return &abi.SessionError{Operation: "commit media processing failure", SessionID: p.config.SessionID, Err: err}
	}
	return nil
}

func (p *Plugin) WrapModel(ctx context.Context, r abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
	if r.State == nil || !p.library.log.SameAuthority(r.State.Session()) || r.Workspace != p.config.Cwd {
		return errors.New("media model request ownership mismatch")
	}
	ctx, finish, err := p.begin(ctx, r.RunID)
	if err != nil {
		return err
	}
	defer finish()
	p.mu.Lock()
	if p.runID != r.RunID {
		p.mu.Unlock()
		return errors.New("media request does not belong to the active run")
	}
	pending := p.pending
	p.pending = nil
	p.mu.Unlock()
	if pending == nil {
		return next(ctx, r, request, emit)
	}
	identity, _ := r.InvocationModel.(abi.ModelIdentity)
	settle := func(state string) error {
		cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
		defer cancel()
		_, err := p.library.receipt(cleanup, *pending, state, identity)
		return err
	}
	if r.InvocationModel == nil || !p.config.ImageCapable || !p.matchesModel(r) {
		return errors.Join(errors.New("actual model changed or image capability is unknown; selected images not submitted"), settle("actual_model_mismatch_not_submitted"))
	}
	if err := ctx.Err(); err != nil {
		return errors.Join(err, settle("request_preparation_cancelled_not_submitted"))
	}
	provenance, err := json.Marshal(pending)
	if err != nil || len(provenance) > 16<<10 {
		return errors.Join(errors.New("selected media provenance exceeds request budget"), settle("request_preparation_failed_not_submitted"))
	}
	blocks := []wire.ContentBlock{{Kind: wire.ContentText, Text: "[Explicit selected derived images only; original media/video/audio and all unselected frames excluded. Processing/source/actual frame times follow. This synthetic input is one request copy only, not saved conversation.]\n" + string(provenance)}}
	for _, manifest := range pending.Manifests {
		_, data, e := p.config.Repository.ReadArtifactFull(ctx, p.config.Owner, p.config.SessionID, manifest.Artifact.URI, nativemedia.MaxImageOutput)
		if e != nil {
			return errors.Join(e, settle("request_preparation_failed_not_submitted"))
		}
		image := wire.Image{Payload: wire.MediaPayload{MediaType: "image/jpeg", Data: data, Provenance: &wire.MediaProvenance{Original: manifest.Source, Operations: []wire.MediaOperation{manifest.Output.Operation}, Description: manifest.Processing}}, Width: manifest.Output.Width, Height: manifest.Output.Height, Detail: "low"}
		blocks = append(blocks, wire.ContentBlock{Kind: wire.ContentImage, Image: &image})
	}
	request.Messages = wire.CloneMessages(request.Messages)
	request.Messages = append(request.Messages, wire.Message{Role: wire.RoleUser, Content: blocks})
	if _, err = p.library.receipt(ctx, *pending, "model_chain_invocation_intent", identity); err != nil {
		return err
	}
	var finished atomic.Bool
	err = next(ctx, r, request, func(chunk wire.StreamChunk) error {
		if chunk.Kind == wire.ChunkFinish {
			finished.Store(true)
		}
		return emit(chunk)
	})
	if err == nil {
		err = ctx.Err()
	}
	state := "response_observed_not_proof_of_understanding"
	if err != nil || !finished.Load() {
		state = "request_error_or_incomplete_acceptance_unknown"
	}
	return errors.Join(err, settle(state))
}

func (p *Plugin) matchesModel(r abi.RuntimeContext) bool {
	model := r.InvocationModel
	if model == nil {
		model = r.State.Model()
	}
	identity, ok := model.(abi.ModelIdentity)
	return ok && identity.ProviderID() == p.config.ProviderID && identity.ModelID() == p.config.ModelID
}

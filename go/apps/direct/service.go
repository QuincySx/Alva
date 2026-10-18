// INPUT: Explicit native data/model/workspace configuration, harness, Controller and native fact repository
// OUTPUT: Service cold session APIs, actual direct Run/Stop/approval and owned Close lifecycle
// POS: [L6 application] Persistent direct desktop binds the existing harness without EngineRuntime, Hub or historical configuration bridges.
package direct

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/extension/builtin"
	"github.com/QuincySx/Alva/go/harness"
	"github.com/QuincySx/Alva/go/integration/llm/provider"
	"github.com/QuincySx/Alva/go/platform/host"
	"github.com/QuincySx/Alva/go/platform/storage/session"
	"github.com/QuincySx/Alva/go/platform/system/httpjson"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

const owner = "alva-desktop"
const metadataNamespace = "alva.direct"
const metadataType = "session"

type Config struct {
	DataDirectory, Workspace string
	Demo                     bool
	ProviderKind             provider.Kind
	Provider                 provider.Config
}
type Metadata struct {
	Title            string `json:"title"`
	Workspace        string `json:"workspace"`
	RouteFingerprint string `json:"route_fingerprint"`
	Demo             bool   `json:"demo"`
}
type SessionRow struct {
	ID, Title, ActiveRunID string
	CreatedAt              time.Time
	HighWater              uint64
	MetadataAvailable      bool
}
type SessionPage struct {
	Sessions []SessionRow
	Next     string
	HasMore  bool
}
type Run struct {
	SessionID, ID string
	// Controller remains the actual execution owner; this pointer is for the
	// trusted application's existing bounded observation path, never a tool DTO.
	Controller *host.Controller
}
type binding struct {
	controller *host.Controller
	files      *builtin.Workspace
	lastRun    string
}
type Service struct {
	mu              sync.Mutex
	repository      *session.Repository
	config          Config
	model           abi.LanguageModel
	route           string
	approvals       *host.ApprovalBroker
	bindings        map[string]*binding
	drafts          map[string][]session.ArtifactInfo
	closing, closed bool
}

func Open(ctx context.Context, config Config) (*Service, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if config.DataDirectory == "" || !filepath.IsAbs(config.DataDirectory) {
		return nil, errors.New("explicit absolute direct data directory required")
	}
	var model abi.LanguageModel
	headers := make(map[string]string, len(config.Provider.Headers))
	for key, value := range config.Provider.Headers {
		headers[key] = value
	}
	config.Provider.Headers = headers
	if config.Demo {
		if config.Provider.Endpoint != "" || config.Provider.Model != "" || config.Provider.APIKey != "" || len(config.Provider.Headers) > 0 {
			return nil, errors.New("provider settings require explicit live mode")
		}
	} else {
		var err error
		model, err = provider.Build(config.ProviderKind, config.Provider)
		if err != nil {
			return nil, err
		}
	}
	if config.Workspace != "" {
		canonical, err := canonicalWorkspace(config.Workspace)
		if err != nil {
			return nil, err
		}
		config.Workspace = canonical
	}
	raw, _ := json.Marshal(struct {
		Demo            bool
		Kind            provider.Kind
		Endpoint, Model string
	}{config.Demo, config.ProviderKind, config.Provider.Endpoint, config.Provider.Model})
	fingerprint := sha256.Sum256(raw)
	repository, err := session.Open(ctx, config.DataDirectory)
	if err != nil {
		return nil, err
	}
	return &Service{repository: repository, config: config, model: model, route: hex.EncodeToString(fingerprint[:]), approvals: host.NewApprovalBroker(32), bindings: make(map[string]*binding), drafts: make(map[string][]session.ArtifactInfo)}, nil
}
func canonicalWorkspace(path string) (string, error) {
	if !filepath.IsAbs(path) {
		return "", errors.New("direct workspace must be explicitly absolute")
	}
	canonical, err := filepath.EvalSymlinks(path)
	if err != nil {
		return "", err
	}
	info, err := os.Stat(canonical)
	if err != nil || !info.IsDir() {
		return "", errors.New("direct workspace must be an existing directory")
	}
	return canonical, nil
}
func (s *Service) Demo() bool { return s.config.Demo }
func (s *Service) RouteLabel() string {
	if s.config.Demo {
		return "local demo"
	}
	return string(s.config.ProviderKind) + " · " + s.config.Provider.Model
}
func (s *Service) Create(ctx context.Context, title string) (string, error) {
	if strings.TrimSpace(title) == "" || len(title) > 256 || !utf8.ValidString(title) {
		return "", errors.New("session title requires 1..256 UTF-8 bytes")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closing {
		return "", session.ErrClosed
	}
	var random [16]byte
	if _, err := rand.Read(random[:]); err != nil {
		return "", err
	}
	id := hex.EncodeToString(random[:])
	log, err := s.repository.Create(ctx, owner, id, "")
	if err != nil {
		return "", err
	}
	raw, _ := json.Marshal(Metadata{Title: title, Workspace: s.config.Workspace, RouteFingerprint: s.route, Demo: s.config.Demo})
	_, err = log.AppendRecords(ctx, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: metadataNamespace, Type: metadataType, SchemaVersion: 1, Payload: raw}})
	// Return the actual allocated ID even if metadata commit failed. The
	// caller can inspect the incomplete session, never silently recreate/replay.
	return id, err
}
func (s *Service) metadata(ctx context.Context, id string) (Metadata, error) {
	entry, found, err := s.repository.LatestRecord(ctx, owner, id, metadataNamespace, metadataType)
	if err != nil {
		return Metadata{}, err
	}
	if !found || entry.Kind != abi.RecordExtension || entry.SchemaVersion != 1 {
		return Metadata{}, errors.New("direct session metadata unavailable; no runtime restored")
	}
	_, raw, err := s.repository.ReadArtifactFull(ctx, owner, id, entry.Value.URI, 16<<10)
	if err != nil {
		return Metadata{}, err
	}
	var record abi.SessionRecord
	if err := json.Unmarshal(raw, &record); err != nil {
		return Metadata{}, err
	}
	if record.ID != entry.ID || record.Sequence != entry.Sequence || record.Namespace != metadataNamespace || record.Type != metadataType || record.SchemaVersion != 1 || record.Kind != abi.RecordExtension {
		return Metadata{}, errors.New("direct metadata identity mismatch")
	}
	var value Metadata
	if err := httpjson.ValidateObject(record.Payload, &value); err != nil {
		return Metadata{}, err
	}
	if err := json.Unmarshal(record.Payload, &value); err != nil {
		return Metadata{}, err
	}
	if value.Title == "" || len(value.Title) > 256 || !utf8.ValidString(value.Title) || len(value.RouteFingerprint) != 64 {
		return Metadata{}, errors.New("invalid direct session metadata")
	}
	return value, nil
}
func (s *Service) Metadata(ctx context.Context, id string) (Metadata, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closing {
		return Metadata{}, session.ErrClosed
	}
	return s.metadata(ctx, id)
}
func (s *Service) List(ctx context.Context, after string) (SessionPage, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closing {
		return SessionPage{}, session.ErrClosed
	}
	page, err := s.repository.List(ctx, owner, after, 64)
	if err != nil {
		return SessionPage{}, err
	}
	result := SessionPage{Next: page.Next, HasMore: page.HasMore, Sessions: []SessionRow{}}
	for _, info := range page.Sessions {
		if info.ParentID != "" {
			continue
		}
		row := SessionRow{ID: info.ID, Title: "Incomplete direct metadata", CreatedAt: info.CreatedAt, HighWater: info.HighWater}
		meta, err := s.metadata(ctx, info.ID)
		if err == nil {
			row.Title = meta.Title
			row.MetadataAvailable = true
		}
		if b := s.bindings[info.ID]; b != nil && b.lastRun != "" {
			snapshot, err := b.controller.Snapshot(b.lastRun)
			if err != nil {
				return SessionPage{}, err
			}
			if snapshot.Status == host.RunRunning {
				row.ActiveRunID = b.lastRun
			}
		}
		result.Sessions = append(result.Sessions, row)
	}
	if err := ctx.Err(); err != nil {
		return SessionPage{}, err
	}
	return result, nil
}
func (s *Service) bind(ctx context.Context, id string) (*binding, error) {
	if b := s.bindings[id]; b != nil {
		return b, nil
	}
	if len(s.bindings) >= 16 {
		return nil, errors.New("direct retained-session budget exhausted; close this application before opening more runtimes")
	}
	meta, err := s.metadata(ctx, id)
	if err != nil {
		return nil, err
	}
	if meta.RouteFingerprint != s.route || meta.Demo != s.config.Demo {
		return nil, errors.New("saved direct route differs from explicit startup route; no history was sent")
	}
	log, err := s.repository.OpenSession(ctx, owner, id)
	if err != nil {
		return nil, err
	}
	plugins, err := harness.NativeSessionPlugins(ctx, s.repository, owner, id)
	if err != nil {
		return nil, err
	}
	var builder *harness.Builder
	if s.config.Demo {
		builder = harness.NewDemoBuilder(log)
	} else {
		builder = harness.NewBuilder(s.model, log)
	}
	b := &binding{}
	if meta.Workspace != "" {
		canonical, err := canonicalWorkspace(meta.Workspace)
		if err != nil || canonical != meta.Workspace {
			return nil, errors.New("saved direct workspace changed; no runtime restored")
		}
		builder.Workspace(canonical)
		if !s.config.Demo {
			files, err := builtin.OpenWorkspace(canonical)
			if err != nil {
				return nil, err
			}
			b.files = files
			guard, err := security.NewGuard(canonical, nil, files, security.NewPermissionManager(security.Ask, s.approvals))
			if err != nil {
				files.Close()
				return nil, err
			}
			references, err := s.repository.BoundReferences(ctx, owner, id)
			if err != nil {
				files.Close()
				return nil, err
			}
			tools := builtin.FileTools(files, guard, references, id)
			for _, tool := range tools {
				builder.Tool(tool)
			}
			builder.Plugin(harness.NewSecurityPlugin(guard, tools, plugins...))
		}
	}
	for _, plugin := range plugins {
		builder.Plugin(plugin)
	}
	runner, err := builder.Build(ctx)
	if err != nil {
		if b.files != nil {
			err = errors.Join(err, b.files.Close())
		}
		return nil, err
	}
	b.controller = host.NewController(runner, 256)
	s.bindings[id] = b
	return b, nil
}
func (s *Service) Start(ctx context.Context, id, prompt string) (Run, error) {
	if strings.TrimSpace(prompt) == "" || len(prompt) > 32<<10 || !utf8.ValidString(prompt) {
		return Run{}, errors.New("direct prompt requires 1..32KiB UTF-8 bytes")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closing {
		return Run{}, session.ErrClosed
	}
	for _, b := range s.bindings {
		if b.lastRun != "" {
			snapshot, err := b.controller.Snapshot(b.lastRun)
			if err != nil {
				return Run{}, err
			}
			if snapshot.Status == host.RunRunning {
				return Run{}, errors.New("a direct root is already active; observe or Stop its actual session")
			}
		}
	}
	b, err := s.bind(ctx, id)
	if err != nil {
		return Run{}, err
	}
	message := wire.TextMessage(wire.RoleUser, prompt)
	for _, artifact := range s.drafts[id] {
		reference := artifact.Reference()
		message.Content = append(message.Content, wire.ContentBlock{Kind: wire.ContentReference, Reference: &reference})
	}
	runID, err := b.controller.Start(ctx, []wire.Message{message})
	if err != nil {
		return Run{}, err
	}
	b.lastRun = runID
	delete(s.drafts, id)
	return Run{SessionID: id, ID: runID, Controller: b.controller}, nil
}
func (s *Service) Stop(id, runID string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	b := s.bindings[id]
	if b == nil || b.lastRun != runID {
		return errors.New("direct run does not belong to this selected session")
	}
	return b.controller.Stop(runID)
}

// CurrentRun observes only an already assembled in-process owner. It never
// restores a runner or treats historical records as active execution.
func (s *Service) CurrentRun(id string) (Run, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	b := s.bindings[id]
	if b == nil || b.lastRun == "" || s.closing {
		return Run{}, false
	}
	return Run{SessionID: id, ID: b.lastRun, Controller: b.controller}, true
}

func (s *Service) HasActive() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, b := range s.bindings {
		if b.lastRun != "" {
			snapshot, err := b.controller.Snapshot(b.lastRun)
			if err != nil || snapshot.Status == host.RunRunning {
				return true
			}
		}
	}
	return false
}
func (s *Service) PendingApprovals(id string) []security.ApprovalRequest {
	return s.approvals.Pending(id)
}
func (s *Service) RespondApproval(ctx context.Context, id, requestID string, allowed bool) error {
	return s.approvals.Respond(ctx, id, requestID, security.ApprovalDecision{Allowed: allowed, Scope: security.Once})
}
func (s *Service) Close(ctx context.Context) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return nil
	}
	s.closing = true
	var result error
	for _, b := range s.bindings {
		result = errors.Join(result, b.controller.Close(ctx))
	}
	if result != nil {
		return result
	}
	for _, b := range s.bindings {
		if b.files != nil {
			result = errors.Join(result, b.files.Close())
		}
	}
	if result != nil {
		return result
	}
	if err := s.approvals.Close(); err != nil {
		return err
	}
	if err := s.repository.Close(); err != nil {
		return err
	}
	s.closed = true
	s.drafts = nil
	return nil
}

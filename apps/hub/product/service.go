// INPUT: context, private files, hub supervisor/store and owner-scoped native artifact repository
// OUTPUT: HubService with durable session/history admission and reference-only attachment drafts
// POS: [L7 Hub product] UI-independent shared Hub product service, not an alternate native fact authority
package product

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/gateway"
	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/worktree"
	"github.com/QuincySx/Alva/extension/shell"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/host"
	"github.com/QuincySx/Alva/platform/storage"
	private "github.com/QuincySx/Alva/platform/system/privatefs"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type DraftAttachment struct {
	BranchID  string               `json:"branch_id"`
	Info      storage.ArtifactInfo `json:"info"`
	Reference wire.Reference       `json:"reference"`
}

// ServiceStore declares the required product routing capability explicitly;
// substituting a backend never loses native permission ownership at runtime.
type ServiceStore interface {
	hub.Store
	hub.WaitStore
	hub.NativeOwnerResolver
}

type HubService struct {
	Store              ServiceStore
	Runtime            *hub.Runtime
	Repository         storage.Repository
	DefaultTarget      hub.Target
	Targets            []hub.Target
	DataDirectory      string
	Demo               bool
	ValidateWaitRecipe func(context.Context, hub.Branch, hub.Settings) error
	Approvals          *host.ApprovalBroker
	Questions          *host.QuestionBroker
	Gateway            *gateway.Local
	Teams              *TeamCoordinator
	Automations        *AutomationCoordinator
	Worktrees          *worktree.Manager
	Shells             *shell.Sessions
	mu                 sync.Mutex
	attachments        map[string][]DraftAttachment
}

// PendingApprovals derives native ownership from committed product routing.
// An arbitrary native ID supplied by UI is never accepted as a grant.
func (s *HubService) PendingApprovals(ctx context.Context, id string) ([]security.ApprovalRequest, error) {
	if s.Approvals == nil {
		return nil, nil
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return nil, err
	}
	if detail.Session.Status != hub.Running {
		return nil, nil
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return nil, err
	}
	if branch.NativeSessionID == "" {
		return nil, nil
	}
	return s.pendingAtRoot(ctx, id, branch.NativeSessionID)
}
func (s *HubService) RespondApproval(ctx context.Context, id, requestID string, allowed bool) error {
	if s.Approvals == nil {
		return errors.New("approval service is unavailable")
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return err
	}
	if detail.Session.Status != hub.Running {
		return host.ErrApprovalExpired
	}
	return s.Runtime.RespondPermission(ctx, id, requestID, engine.PermissionDecision{Allow: allowed})
}

func NewHubService(store ServiceStore, runtime *hub.Runtime, repository storage.Repository, target hub.Target, directory string, demo bool) (*HubService, error) {
	if storage.Missing(store) || runtime == nil || storage.Missing(repository) {
		return nil, errors.New("product store, runtime and native repository required")
	}
	if err := target.Validate(); err != nil {
		return nil, err
	}
	dir, err := private.EnsureDir(directory)
	if err != nil {
		return nil, err
	}
	service := &HubService{Store: store, Runtime: runtime, Repository: repository, DefaultTarget: target, Targets: []hub.Target{target}, DataDirectory: dir, Demo: demo, attachments: make(map[string][]DraftAttachment)}
	if teams, ok := store.(hub.TeamStore); ok {
		service.Teams = NewTeamCoordinator(teams, runtime)
	}
	if schedules, ok := store.(hub.AutomationStore); ok {
		service.Automations = newAutomationCoordinator(service, schedules)
	}
	return service, nil
}
func (s *HubService) Create(ctx context.Context, title, workspace string) (hub.Session, error) {
	if workspace == "" {
		id, err := hub.NewID("workspace-")
		if err != nil {
			return hub.Session{}, err
		}
		workspace, err = private.EnsureDir(filepath.Join(s.DataDirectory, "workspaces", id))
		if err != nil {
			return hub.Session{}, err
		}
	}
	canonical, err := filepath.EvalSymlinks(workspace)
	if err != nil {
		return hub.Session{}, err
	}
	info, err := os.Stat(canonical)
	if err != nil || !info.IsDir() {
		return hub.Session{}, errors.New("workspace must be an existing directory")
	}
	return s.Store.CreateSession(ctx, title, canonical, s.DefaultTarget)
}

// ActiveBranch resolves only the persisted branch selected by product routing.
func ActiveBranch(detail hub.Detail) (hub.Branch, error) {
	for _, branch := range detail.Branches {
		if branch.ID == detail.Session.ActiveBranchID {
			return branch, nil
		}
	}
	return hub.Branch{}, errors.New("active branch is missing")
}
func (s *HubService) Submit(ctx context.Context, id, prompt string) (string, error) {
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return "", err
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return "", err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	prompt, err = s.draftPromptLocked(id, branch, prompt)
	if err != nil {
		return "", err
	}
	if ctx.Err() != nil {
		return "", ctx.Err()
	}
	// Runtime owns the admitted Turn. A UI admission timeout must not become an
	// accidental execution cancellation after the durable claim has committed.
	turnID, _, err := s.Runtime.StartOnBranch(context.Background(), id, branch.ID, prompt)
	if err != nil {
		return "", err
	}
	delete(s.attachments, id)
	return turnID, nil
}
func (s *HubService) draftPromptLocked(id string, branch hub.Branch, prompt string) (string, error) {
	files := s.attachments[id]
	if len(files) > 0 {
		for _, file := range files {
			if file.BranchID != branch.ID || file.Info.SessionID != branch.NativeSessionID {
				return "", errors.New("draft files belong to a parked branch; return to it or remove the draft references")
			}
		}
		manifest, err := json.Marshal(files)
		if err != nil {
			return "", err
		}
		prompt += "\n\n[User-selected files: references/processing manifests only. Original bytes were saved; not delivered to the model. Read/search only relevant ranges.]\n" + string(manifest)
	}
	return prompt, nil
}
func (s *HubService) Upload(ctx context.Context, id, path string) (DraftAttachment, error) {
	var result DraftAttachment
	if !filepath.IsAbs(path) {
		return result, errors.New("file picker must provide an absolute selected path")
	}
	info, err := os.Lstat(path)
	if err != nil {
		return result, err
	}
	if !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 || info.Size() > 20<<20 {
		return result, errors.New("selected file must be regular and within 20 MiB/file")
	}
	file, err := os.Open(path)
	if err != nil {
		return result, err
	}
	defer file.Close()
	opened, err := file.Stat()
	if err != nil || !os.SameFile(info, opened) {
		return result, errors.New("selected file changed before opening")
	}
	return s.UploadReader(ctx, id, filepath.Base(path), file)
}

// UploadReader accepts bytes, never a remote caller-selected host path. Native
// file-picker and authenticated HTTP multipart routes share this exact admission
// and persistence policy; the caller cannot assert media processing occurred.
func (s *HubService) UploadReader(ctx context.Context, id, name string, source io.Reader) (DraftAttachment, error) {
	var result DraftAttachment
	if source == nil || name == "" || len(name) > 256 || !utf8.ValidString(name) || strings.ContainsAny(name, "/\\\x00") || name == "." || name == ".." {
		return result, errors.New("bounded plain attachment filename and reader required")
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return result, err
	}
	if hub.BlocksInput(detail.Session.Status) {
		return result, errors.New("finish or stop the Turn before attaching files")
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return result, err
	}
	if branch.Target.EngineID != "alva" {
		return result, errors.New("native artifact reference tools are currently available only on Alva branches")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	files := s.attachments[id]
	if len(files) >= 8 {
		return result, errors.New("at most 8 files per Turn")
	}
	var total int64
	for _, file := range files {
		total += file.Info.Bytes
	}

	data, err := io.ReadAll(io.LimitReader(source, 20<<20+1))
	if err != nil {
		return result, err
	}
	if len(data) > 20<<20 || total+int64(len(data)) > 40<<20 {
		return result, errors.New("selected file exceeds 20 MiB/file or 40 MiB/Turn upload budget")
	}
	if branch.NativeSessionID == "" {
		nativeID, err := hub.NewID("native-")
		if err != nil {
			return result, err
		}
		if _, err = s.Repository.Create(ctx, id, nativeID, ""); err != nil {
			return result, err
		}
		if err = s.Store.BindNative(ctx, id, branch.ID, nativeID); err != nil {
			return result, err
		}
		branch.NativeSessionID = nativeID
	}
	mediaType := http.DetectContentType(data)
	text := utf8.Valid(data) && (strings.HasPrefix(mediaType, "text/") || strings.HasPrefix(mediaType, "application/json"))
	artifact, err := s.Repository.PutArtifact(ctx, id, branch.NativeSessionID, storage.ArtifactInput{Kind: "input-attachment", Name: name, MediaType: mediaType, Text: text, Data: data, Processing: "Original bytes saved with SHA-256. No compression, transcoding, frame extraction, OCR or media understanding performed. Only this metadata/reference is added to the submitted prompt."})
	if err != nil {
		return result, err
	}
	result = DraftAttachment{BranchID: branch.ID, Info: artifact, Reference: artifact.Reference()}
	s.attachments[id] = append(files, result)
	return result, nil
}
func (s *HubService) DraftAttachments(id string) []DraftAttachment {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]DraftAttachment(nil), s.attachments[id]...)
}
func (s *HubService) RemoveDraftAttachment(id, artifactID string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	files := s.attachments[id]
	for i, file := range files {
		if file.Info.ID == artifactID {
			s.attachments[id] = append(files[:i], files[i+1:]...)
			return
		}
	}
}

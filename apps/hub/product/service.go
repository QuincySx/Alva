// INPUT: context, private files, hub supervisor/store and owner-scoped native artifact repository
// OUTPUT: HubService with durable admission, explicit immutable draft selections and shared developer job owner
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

	"github.com/QuincySx/Alva/apps/developer"
	"github.com/QuincySx/Alva/apps/gateway"
	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/worktree"
	"github.com/QuincySx/Alva/extension/shell"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/host"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/system/document"
	private "github.com/QuincySx/Alva/platform/system/privatefs"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type DraftAttachment struct {
	DraftID   string               `json:"draft_id"`
	BranchID  string               `json:"branch_id"`
	Info      storage.ArtifactInfo `json:"info"`
	Reference wire.Reference       `json:"reference"`
}

// DraftReferenceSelection identifies one prepared draft and its immutable
// artifact version. Empty selections mean no files or project references.
type DraftReferenceSelection struct {
	Kind   string `json:"kind"`
	ID     string `json:"id"`
	SHA256 string `json:"sha256"`
}

func (d DraftAttachment) DraftSelection() DraftReferenceSelection {
	return DraftReferenceSelection{Kind: "attachment", ID: d.DraftID, SHA256: d.Info.SHA256}
}

// ServiceStore declares the required product routing capability explicitly;
// substituting a backend never loses native permission ownership at runtime.
type ServiceStore interface {
	hub.Store
	hub.WaitStore
	hub.NativeOwnerResolver
	hub.SessionModelOptionsStore
	hub.PendingStore
	hub.VersionStore
}

type HubService struct {
	Store               ServiceStore
	Runtime             *hub.Runtime
	Repository          storage.Repository
	DefaultTarget       hub.Target
	Targets             []hub.Target
	DataDirectory       string
	Demo                bool
	ValidateWaitRecipe  func(context.Context, hub.Branch, hub.Settings) error
	Approvals           *host.ApprovalBroker
	Questions           *host.QuestionBroker
	Gateway             *gateway.Local
	Developer           *developer.Service
	Preferences         *PreferencesService
	ComposerDrafts      *ComposerDraftsService
	Documents           *document.Processor
	Connections         *ConnectionsService
	Goals               *GoalsService
	Companions          *CompanionService
	WorkspaceViews      *WorkspaceViewsService
	DeveloperWorkspaces *DeveloperWorkspacesService
	ConversationTabs    *ConversationTabsService
	Teams               *TeamCoordinator
	Automations         *AutomationCoordinator
	Worktrees           *worktree.Manager
	Shells              *shell.Sessions
	// Transcripts is the narrow Message Stream reader; UI reads history only
	// through it, never through Store.History*.
	Transcripts *TranscriptService
	// Traces is the narrow execution-trace reader; UI reads native trace
	// metadata only through it, never through Store, Repository or Inspector.
	Traces *TraceService
	// Settings is the only settings entry for UI: application intent,
	// per-workspace approvals and session overrides, never Store directly.
	Settings *SettingsService
	// Context holds one-shot manual context compaction requests (M4).
	Context *ContextService
	// Conversations and Turns are the narrow product entries: desktop, HTTP
	// and CLI list/read/manage sessions and start/stop Turns only through
	// them, never through Store or Runtime (tests/architecture gate).
	Conversations *ConversationService
	Turns         *TurnService
	// mu serializes admission and source preparation; draftMu protects only the
	// small published draft lists so observers never wait for disk/admission I/O.
	mu                  sync.Mutex
	draftMu             sync.RWMutex
	attachments         map[string][]DraftAttachment
	workspaceReferences map[string][]DraftWorkspaceReference
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
	if branch.Target.EngineID != "alva" || branch.NativeSessionID == "" {
		return nil, nil
	}
	return s.pendingAtRoot(ctx, id, branch.NativeSessionID)
}
func (s *HubService) RespondApproval(ctx context.Context, id, requestID string, allowed bool) error {
	if s.Approvals == nil {
		return errors.New("approval service is unavailable")
	}
	binding, ready := s.Runtime.ActivePermission(id)
	if !ready || binding.EngineID != "alva" {
		return host.ErrApprovalExpired
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return err
	}
	if detail.Session.Status != hub.Running {
		return host.ErrApprovalExpired
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return err
	}
	if branch.Target.EngineID != binding.EngineID || branch.NativeSessionID != binding.NativeSessionID {
		return host.ErrApprovalExpired
	}
	// The pending request is gone once answered, so keep its call and effect
	// for the settled line.
	var asked security.ApprovalRequest
	for _, request := range s.Approvals.PendingSnapshot() {
		if request.ID == requestID {
			asked = request
			break
		}
	}
	// A child session's call ID may repeat the root's, so only the root's own
	// calls take their display from the root attempt; others show the effect.
	settledCall := settledCallID(asked, binding.NativeSessionID)
	err = s.Runtime.RespondPermissionBound(ctx, id, binding, requestID, engine.PermissionDecision{Allow: allowed})
	if errors.Is(err, hub.ErrSessionNotRunning) {
		return host.ErrApprovalExpired
	}
	if err == nil && asked.ID != "" {
		// Best effort: the decision already took effect, and a line that cannot
		// be stored must not turn a delivered consent into a failure.
		if recorder, ok := s.Store.(hub.ApprovalSettledStore); ok {
			_ = recorder.RecordApprovalSettled(context.WithoutCancel(ctx), id, binding.AttemptID, hub.ApprovalSettled{CallID: settledCall, Allowed: allowed, Effect: knownEffect(engine.Effect(asked.Effect.Kind))})
		}
	}
	return err
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
	service := &HubService{Store: store, Runtime: runtime, Repository: repository, DefaultTarget: target, Targets: []hub.Target{target}, DataDirectory: dir, Demo: demo, Context: NewContextService(), attachments: make(map[string][]DraftAttachment)}
	service.Conversations = &ConversationService{service: service}
	service.Turns = &TurnService{service: service}
	// A finished Turn releases its session to the next queued input.
	runtime.SetExitObserver(service.dispatchAfterExit)
	if teams, ok := store.(hub.TeamStore); ok {
		service.Teams = NewTeamCoordinator(teams, runtime)
	}
	if schedules, ok := store.(hub.AutomationStore); ok {
		service.Automations = newAutomationCoordinator(service, schedules)
	}
	if app, ok := store.(hub.AppSettingsStore); ok {
		settings, err := newSettingsService(service, app)
		if err != nil {
			return nil, err
		}
		service.Settings = settings
	}
	return service, nil
}
func (s *HubService) Create(ctx context.Context, title, workspace string) (hub.Session, error) {
	canonical, err := s.prepareSessionWorkspace(workspace)
	if err != nil {
		return hub.Session{}, err
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
func (s *HubService) Submit(ctx context.Context, id, prompt string, references []DraftReferenceSelection) (string, error) {
	return s.submitDraft(ctx, id, prompt, nil, references)
}
func (s *HubService) submitDraft(ctx context.Context, id, prompt string, selections []ComposerPreparation, references []DraftReferenceSelection) (string, error) {
	return s.submitDraftWithFiles(ctx, id, prompt, selections, references, nil, prompt)
}

func (s *HubService) submitDraftWithFiles(ctx context.Context, id, prompt string, selections []ComposerPreparation, references []DraftReferenceSelection, inputFiles []engine.InputFile, userText string) (string, error) {
	// Runtime owns the admitted Turn. A UI admission timeout must not become an
	// accidental execution cancellation after the durable claim has committed.
	start, err := s.admitDraftWithFiles(ctx, context.Background(), id, prompt, selections, references, inputFiles, userText)
	return start.TurnID, err
}

// admitDraftWithFiles is the one admission path for every entry. owner bounds
// the execution's lifetime; ctx bounds only admission.
func (s *HubService) admitDraftWithFiles(ctx, owner context.Context, id, prompt string, selections []ComposerPreparation, references []DraftReferenceSelection, inputFiles []engine.InputFile, userText string) (TurnStart, error) {
	return s.admitTurn(ctx, owner, id, prompt, selections, references, inputFiles, userText, "")
}

// admitTurn is admitDraftWithFiles plus the Turn origin: "" for an ordinary
// prompt, hub.TurnOriginContinue for the product's Continue action.
func (s *HubService) admitTurn(ctx, owner context.Context, id, prompt string, selections []ComposerPreparation, references []DraftReferenceSelection, inputFiles []engine.InputFile, userText, origin string) (TurnStart, error) {
	return s.admitWith(ctx, owner, id, prompt, selections, references, inputFiles, userText, origin, nil, "")
}

// admitWith is admitTurn plus the two product operations that change what an
// admission does inside its transaction: line starts a new version line cut
// before a Turn (edit, regenerate), queueID names the queued input this
// admission dispatches.
func (s *HubService) admitWith(ctx, owner context.Context, id, prompt string, selections []ComposerPreparation, references []DraftReferenceSelection, inputFiles []engine.InputFile, userText, origin string, line *hub.LineFork, queueID string) (TurnStart, error) {
	var start TurnStart
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.Companions != nil {
		binding, err := s.Companions.Session(ctx, id)
		if err != nil && !errors.Is(err, hub.ErrNotFound) {
			return start, err
		}
		if err == nil {
			readOnly, readErr := s.Companions.SessionReadOnly(ctx, id)
			if readErr != nil {
				return start, readErr
			}
			if readOnly {
				return start, hub.ErrCompanionDisabled
			}
		}
		if err == nil && binding.GroupID != "" {
			return start, errors.New("send from the companion group; this member session is for execution review, approvals and original-Turn continuation")
		}
	}
	var err error
	selections, err = s.rebindComposerGoalsLocked(ctx, id, selections)
	if err != nil {
		return start, err
	}
	selections, err = s.rebindComposerSkillsLocked(ctx, id, selections)
	if err != nil {
		return start, err
	}
	if err := s.prepareComposerDraftLocked(ctx, id, selections); err != nil {
		return start, err
	}
	prompt, err = s.composerGoalPrompt(ctx, id, selections, prompt)
	if err != nil {
		return start, err
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return start, err
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return start, err
	}
	if err = s.validatePlanTarget(ctx, id, branch); err != nil {
		return start, err
	}
	if err = s.ValidateTarget(branch.Target); err != nil {
		return start, err
	}
	inputSkills, err := s.composerSkillInputs(ctx, detail, selections)
	if err != nil {
		return start, err
	}
	if len(inputSkills) > 0 {
		descriptor, ok := s.Runtime.Descriptor(branch.Target.EngineID)
		if !ok || descriptor.Runtime == nil || !descriptor.Runtime.Capabilities().SkillInputs {
			return start, engine.ErrUnsupported
		}
	}
	prompt, err = s.draftPromptLocked(id, branch, detail.Session.Workspace, prompt, references)
	if err != nil {
		return start, err
	}
	if ctx.Err() != nil {
		return start, ctx.Err()
	}
	if line != nil {
		// Whether the engine can continue from a cut copy of its native
		// conversation decides how the new version starts.
		forked := *line
		forked.NativeFork = s.nativeForkable(ctx, id, line.TurnID, branch, false)
		line = &forked
	}
	if len(inputFiles) == 0 && len(inputSkills) == 0 && origin == "" && line == nil && queueID == "" {
		start.TurnID, start.ExecutionID, start.Done, err = s.Runtime.StartOnBranchOwned(owner, id, branch.ID, prompt)
	} else {
		start.TurnID, start.ExecutionID, start.Done, err = s.Runtime.StartComposerOnBranchOwned(owner, id, branch.ID, hub.ComposerTurnInput{Origin: origin, Prompt: prompt, UserText: userText, Files: inputFiles, Skills: inputSkills, Line: line, QueueID: queueID})
	}
	if err != nil {
		return TurnStart{}, err
	}
	s.consumeDraftReferences(id, references)
	return start, nil
}
func (s *HubService) draftPromptLocked(id string, branch hub.Branch, workspace, prompt string, references []DraftReferenceSelection) (string, error) {
	s.draftMu.RLock()
	defer s.draftMu.RUnlock()
	files, projects, err := s.selectedDraftReferencesLocked(id, references)
	if err != nil {
		return "", err
	}
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
	return s.workspaceReferencePromptLocked(id, branch, workspace, prompt, projects)
}

// Caller holds draftMu. Selection never defaults to a session-wide draft list.
func (s *HubService) selectedDraftReferencesLocked(id string, selections []DraftReferenceSelection) ([]DraftAttachment, []DraftWorkspaceReference, error) {
	if len(selections) > maxDraftReferenceCount {
		return nil, nil, errors.New("at most 8 selected file or project references per Turn")
	}
	var files []DraftAttachment
	var projects []DraftWorkspaceReference
	var bytes int64
	seen := make(map[string]bool, len(selections))
	for _, selection := range selections {
		if selection.ID == "" || seen[selection.ID] {
			return nil, nil, errors.New("unknown or duplicate draft reference selection")
		}
		seen[selection.ID] = true
		found := false
		switch selection.Kind {
		case "attachment":
			for _, draft := range s.attachments[id] {
				if draft.DraftSelection() == selection {
					files = append(files, draft)
					bytes += draft.Info.Bytes
					found = true
					break
				}
			}
		case "workspace":
			for _, draft := range s.workspaceReferences[id] {
				if draft.DraftSelection() == selection {
					projects = append(projects, draft)
					bytes += draft.Info.Bytes
					found = true
					break
				}
			}
		}
		if !found {
			return nil, nil, errors.New("draft reference is missing or its retained version changed")
		}
		if bytes > maxDraftReferenceBytes {
			return nil, nil, errors.New("selected files and project references exceed 40 MiB/Turn budget")
		}
	}
	return files, projects, nil
}

func (s *HubService) consumeDraftReferences(id string, selections []DraftReferenceSelection) {
	if len(selections) == 0 {
		return
	}
	s.draftMu.Lock()
	defer s.draftMu.Unlock()
	selected := make(map[DraftReferenceSelection]bool, len(selections))
	for _, selection := range selections {
		selected[selection] = true
	}
	files := s.attachments[id][:0]
	for _, file := range s.attachments[id] {
		if !selected[file.DraftSelection()] {
			files = append(files, file)
		}
	}
	if s.attachments != nil {
		s.attachments[id] = files
	}
	projects := s.workspaceReferences[id][:0]
	for _, project := range s.workspaceReferences[id] {
		if !selected[project.DraftSelection()] {
			projects = append(projects, project)
		}
	}
	if s.workspaceReferences != nil {
		s.workspaceReferences[id] = projects
	}
}

func (s *HubService) validatePlanTarget(ctx context.Context, id string, branch hub.Branch) error {
	settings, err := s.Store.Settings(ctx, id)
	if err != nil {
		return err
	}
	if settings.PlanMode && branch.Target.EngineID != "alva" {
		return engine.ErrUnsupported
	}
	return nil
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
// file-picker and authenticated HTTP byte routes share this exact admission
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
	count, total := s.draftReferenceBudgetLocked(id)
	if count >= maxDraftReferenceCount {
		return result, errors.New("shared session staging allows at most 8 files or project references")
	}

	data, err := io.ReadAll(io.LimitReader(source, 20<<20+1))
	if err != nil {
		return result, err
	}
	if len(data) > 20<<20 || total+int64(len(data)) > maxDraftReferenceBytes {
		return result, errors.New("selected file exceeds 20 MiB/file or the shared 40 MiB session staging budget")
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
	draftID, err := hub.NewID("draft-")
	if err != nil {
		return result, err
	}
	result = DraftAttachment{DraftID: draftID, BranchID: branch.ID, Info: artifact, Reference: artifact.Reference()}
	s.draftMu.Lock()
	s.attachments[id] = append(s.attachments[id], result)
	s.draftMu.Unlock()
	return result, nil
}
func (s *HubService) DraftAttachments(id string) []DraftAttachment {
	s.draftMu.RLock()
	defer s.draftMu.RUnlock()
	return append([]DraftAttachment(nil), s.attachments[id]...)
}
func (s *HubService) RemoveDraftAttachment(id, draftID string) {
	s.draftMu.Lock()
	defer s.draftMu.Unlock()
	files := s.attachments[id]
	for i, file := range files {
		if file.DraftID == draftID {
			s.attachments[id] = append(files[:i], files[i+1:]...)
			return
		}
	}
}

// settledCallID is the call whose display a settled line may borrow: only one
// asked by the root native session itself.
func settledCallID(asked security.ApprovalRequest, rootNative string) string {
	if asked.SessionID != rootNative {
		return ""
	}
	return asked.CallID
}

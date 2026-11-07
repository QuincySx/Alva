// INPUT: Explicit selected workspace, existing managed Skill repository and exact frozen preparation sources.
// OUTPUT: Pre-session composer catalogs/selections and branch-bound typed Skill inputs on explicit Send.
// POS: [L7 product service] Reuses project Skill parsing and confinement without global scans or enabling a whole catalog.
package product

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/protocol/skill"
)

func composerHomeEntries(ctx context.Context, workspace string, includeSkills bool) ([]ComposerCatalogEntry, error) {
	entries := composerBuiltins()
	if !includeSkills || workspace == "" {
		return entries, ctx.Err()
	}
	canonical, err := canonicalGoalWorkspace(workspace)
	if err != nil {
		return nil, err
	}
	repository, err := skill.OpenWithin(canonical, ManagedSkillDirectory, false)
	if errors.Is(err, os.ErrNotExist) {
		return entries, ctx.Err()
	}
	if err != nil {
		return nil, err
	}
	defer repository.Close()
	list, err := repository.List(ctx)
	if err != nil {
		return nil, err
	}
	for _, value := range list {
		entry := ComposerCatalogEntry{ID: "skill:" + value.Meta.Name, Kind: ComposerSkillEntry, Name: value.Meta.Name, Description: value.Meta.Description, Origin: "alva.project-skill", Scope: "workspace", Source: ManagedSkillDirectory + "/" + value.Meta.Name + "/SKILL.md", Available: value.Enabled, Arguments: composerArguments()}
		if !entry.Available {
			entry.UnavailableReason = "project Skill source is disabled or archived; select an available source"
		}
		entries = append(entries, entry)
	}
	return entries, ctx.Err()
}

func (s *HubService) ComposerHomeCatalog(ctx context.Context, workspace string, query ComposerCatalogQuery) (ComposerCatalogPage, error) {
	if err := validateComposerCatalogQuery(query); err != nil {
		return ComposerCatalogPage{}, err
	}
	entries, err := composerHomeEntries(ctx, workspace, query.Kind != ComposerCommandEntry)
	if err != nil {
		return ComposerCatalogPage{}, err
	}
	return composerCatalogPage(ctx, entries, query)
}

func (s *HubService) ComposerHomeEntry(ctx context.Context, workspace, entryID string) (ComposerEntryDetail, error) {
	if len(entryID) > 72 || entryID != "builtin:plan" && entryID != "builtin:review" && (!strings.HasPrefix(entryID, "skill:") || !skill.ValidName(strings.TrimPrefix(entryID, "skill:"))) {
		return ComposerEntryDetail{}, errors.New("unknown composer entry identity")
	}
	entries, err := composerHomeEntries(ctx, workspace, strings.HasPrefix(entryID, "skill:"))
	if err != nil {
		return ComposerEntryDetail{}, err
	}
	var value ComposerEntryDetail
	for _, entry := range entries {
		if entry.ID == entryID {
			value.Entry = entry
			break
		}
	}
	if value.Entry.ID == "" {
		return ComposerEntryDetail{}, errors.New("selected home composer entry is unavailable")
	}
	if value.Entry.Kind != ComposerSkillEntry {
		return value, ctx.Err()
	}
	canonical, err := canonicalGoalWorkspace(workspace)
	if err != nil {
		return ComposerEntryDetail{}, err
	}
	repository, err := skill.OpenWithin(canonical, ManagedSkillDirectory, false)
	if err != nil {
		return ComposerEntryDetail{}, err
	}
	defer repository.Close()
	resource, err := repository.Resource(ctx, value.Entry.Name, "SKILL.md")
	if err != nil {
		return ComposerEntryDetail{}, err
	}
	document, err := skill.Parse(resource.Bytes)
	if err != nil || document.Meta.Name != value.Entry.Name {
		return ComposerEntryDetail{}, errors.Join(errors.New("selected home Skill source changed"), err)
	}
	sum := sha256.Sum256(resource.Bytes)
	value.Document, value.SHA256 = &document, hex.EncodeToString(sum[:])
	return value, ctx.Err()
}

func (s *HubService) PrepareComposerHomeSelection(ctx context.Context, workspace string, request ComposerSelectionRequest) (ComposerPreparation, error) {
	if err := validateComposerSelectionArguments(request); err != nil {
		return ComposerPreparation{}, err
	}
	if !strings.HasPrefix(request.EntryID, "skill:") {
		return s.PrepareComposerSelection(ctx, "", request)
	}
	detail, err := s.ComposerHomeEntry(ctx, workspace, request.EntryID)
	if err != nil {
		return ComposerPreparation{}, err
	}
	if !detail.Entry.Available {
		return ComposerPreparation{}, errors.New(detail.Entry.UnavailableReason)
	}
	canonical, err := canonicalGoalWorkspace(workspace)
	if err != nil {
		return ComposerPreparation{}, err
	}
	entry := detail.Entry
	value := ComposerPreparation{EntryID: entry.ID, Kind: entry.Kind, Origin: entry.Origin, Scope: entry.Scope, Text: "请使用项目 Skill「" + entry.Name + "」处理下一次请求。", SkillRef: &ComposerSkillReference{Workspace: canonical, Name: entry.Name, RelativePath: entry.Source, SHA256: detail.SHA256, Policy: skill.Automatic}}
	if request := request.Arguments["request"]; request != "" {
		value.Arguments = map[string]string{"request": request}
		value.Text += "\n\n" + request
	}
	return value, ctx.Err()
}

func (s *HubService) revalidatePinnedComposerSkill(ctx context.Context, id string, prepared ComposerPreparation) error {
	ref := prepared.SkillRef
	if ref == nil || ref.Workspace == "" {
		return errors.New("pinned project Skill source is missing")
	}
	current, err := s.PrepareComposerHomeSelection(ctx, ref.Workspace, ComposerSelectionRequest{EntryID: prepared.EntryID, Arguments: prepared.Arguments})
	if err != nil {
		return err
	}
	if id != "" {
		detail, err := s.Store.Detail(ctx, id)
		if err != nil {
			return err
		}
		branch, err := ActiveBranch(detail)
		if err != nil {
			return err
		}
		if ref.Workspace != detail.Session.Workspace || ref.SessionID != id || ref.BranchID != branch.ID {
			return errors.New("pinned project Skill belongs to another workspace or branch")
		}
		current.SkillRef.SessionID, current.SkillRef.BranchID = id, branch.ID
	}
	if !reflect.DeepEqual(current, prepared) {
		return errors.New("pinned project Skill source changed; review and select it again")
	}
	return ctx.Err()
}

func (s *HubService) rebindComposerSkillsLocked(ctx context.Context, id string, selections []ComposerPreparation) ([]ComposerPreparation, error) {
	result := append([]ComposerPreparation(nil), selections...)
	for i, prepared := range selections {
		ref := prepared.SkillRef
		if ref == nil || ref.Workspace == "" || ref.SessionID != "" {
			continue
		}
		if err := s.revalidatePinnedComposerSkill(ctx, "", prepared); err != nil {
			return nil, err
		}
		detail, err := s.Store.Detail(ctx, id)
		if err != nil {
			return nil, err
		}
		branch, err := ActiveBranch(detail)
		if err != nil {
			return nil, err
		}
		if detail.Session.Workspace != ref.Workspace {
			return nil, errors.New("home Skill belongs to another selected workspace")
		}
		copy := *ref
		copy.SessionID, copy.BranchID = id, branch.ID
		result[i].SkillRef = &copy
	}
	return result, ctx.Err()
}

func (s *HubService) composerSkillInputs(ctx context.Context, detail hub.Detail, preparations []ComposerPreparation) ([]engine.InputSkill, error) {
	var result []engine.InputSkill
	for _, prepared := range preparations {
		if prepared.Kind != ComposerSkillEntry {
			continue
		}
		if err := s.RevalidateComposerPreparation(ctx, detail.Session.ID, prepared); err != nil {
			return nil, err
		}
		ref := prepared.SkillRef
		if ref == nil {
			return nil, errors.New("selected Skill source is missing")
		}
		result = append(result, engine.InputSkill{Name: ref.Name, Path: filepath.Join(detail.Session.Workspace, filepath.FromSlash(ref.RelativePath)), SHA256: ref.SHA256})
	}
	return result, ctx.Err()
}

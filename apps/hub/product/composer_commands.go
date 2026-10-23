// INPUT: Saved product routing/settings, confined managed Skills and explicit composer selections
// OUTPUT: Bounded command/Skill catalog, cold details and revalidated next-submit preparations
// POS: [L7 Hub product] Product draft intents and Skill references; selection never sends, enables or executes
package product

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/protocol/skill"
)

type ComposerEntryKind string

const (
	ComposerCommandEntry ComposerEntryKind = "command"
	ComposerSkillEntry   ComposerEntryKind = "skill"
)

type ComposerIntent string

const ComposerIntentPlan ComposerIntent = "plan"

type ComposerArgument struct {
	Name     string `json:"name"`
	Label    string `json:"label"`
	Required bool   `json:"required"`
	MaxBytes int    `json:"max_bytes"`
}

type ComposerCatalogEntry struct {
	ID                string             `json:"id"`
	Kind              ComposerEntryKind  `json:"kind"`
	Name              string             `json:"name"`
	Description       string             `json:"description"`
	Origin            string             `json:"origin"`
	Scope             string             `json:"scope"`
	Source            string             `json:"source,omitempty"`
	Available         bool               `json:"available"`
	UnavailableReason string             `json:"unavailable_reason,omitempty"`
	Arguments         []ComposerArgument `json:"arguments"`
}

type ComposerCatalogQuery struct {
	Kind   ComposerEntryKind `json:"kind,omitempty"`
	Search string            `json:"search,omitempty"`
	Offset int               `json:"offset,omitempty"`
	Limit  int               `json:"limit,omitempty"`
}

type ComposerCatalogPage struct {
	Entries []ComposerCatalogEntry `json:"entries"`
	Total   int                    `json:"total"`
	Offset  int                    `json:"offset"`
	Limit   int                    `json:"limit"`
}

type ComposerEntryDetail struct {
	Entry    ComposerCatalogEntry `json:"entry"`
	Document *skill.Document      `json:"document,omitempty"`
	SHA256   string               `json:"sha256,omitempty"`
}

type ComposerSelectionRequest struct {
	EntryID   string            `json:"entry_id"`
	Arguments map[string]string `json:"arguments,omitempty"`
}

// ComposerSkillReference pins the selected original document and its saved
// product branch. It requests the existing Skill capability; it grants no tools.
type ComposerSkillReference struct {
	SessionID    string       `json:"session_id"`
	BranchID     string       `json:"branch_id"`
	Name         string       `json:"name"`
	RelativePath string       `json:"relative_path"`
	SHA256       string       `json:"sha256"`
	Policy       skill.Policy `json:"policy"`
}

// ComposerPreparation belongs to a draft. Intent=plan still requires the
// product's real PlanMode operation before admission; preparation changes none.
type ComposerPreparation struct {
	EntryID   string                  `json:"entry_id"`
	Kind      ComposerEntryKind       `json:"kind"`
	Origin    string                  `json:"origin"`
	Scope     string                  `json:"scope"`
	Arguments map[string]string       `json:"arguments,omitempty"`
	Intent    ComposerIntent          `json:"intent,omitempty"`
	Text      string                  `json:"text"`
	SkillRef  *ComposerSkillReference `json:"skill_ref,omitempty"`
}

func composerArguments() []ComposerArgument {
	return []ComposerArgument{{Name: "request", Label: "补充任务或审查范围", MaxBytes: 8192}}
}

func composerBuiltins() []ComposerCatalogEntry {
	return []ComposerCatalogEntry{
		{ID: "builtin:plan", Kind: ComposerCommandEntry, Name: "plan", Description: "准备下一次提交的计划意图；选择后仍需显式发送", Origin: "alva.builtin", Scope: "application", Available: true, Arguments: composerArguments()},
		{ID: "builtin:review", Kind: ComposerCommandEntry, Name: "review", Description: "准备下一次提交的项目变更审查请求", Origin: "alva.builtin", Scope: "application", Available: true, Arguments: composerArguments()},
	}
}

func (s *HubService) composerEntries(ctx context.Context, id string, includeSkills bool) ([]ComposerCatalogEntry, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	entries := composerBuiltins()
	if id == "" {
		return entries, nil
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return nil, err
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return nil, err
	}
	busy := hub.BlocksInput(detail.Session.Status)
	if s.Runtime != nil {
		_, active := s.Runtime.ActiveExecution(id)
		busy = busy || active
	}
	for i := range entries {
		if busy {
			entries[i].Available = false
			entries[i].UnavailableReason = "finish the current Turn before preparing a command"
		} else if entries[i].ID == "builtin:plan" && branch.Target.EngineID != "alva" {
			entries[i].Available = false
			entries[i].UnavailableReason = "plan mode requires the Alva engine"
		}
	}
	if !includeSkills {
		return entries, ctx.Err()
	}
	settings, err := s.Store.Settings(ctx, id)
	if err != nil {
		return nil, err
	}
	repository, err := s.projectSkills(ctx, id, false, false)
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
		entry := ComposerCatalogEntry{ID: "skill:" + value.Meta.Name, Kind: ComposerSkillEntry, Name: value.Meta.Name, Description: value.Meta.Description, Origin: "alva.project-skill", Scope: "workspace", Source: ManagedSkillDirectory + "/" + value.Meta.Name + "/SKILL.md", Available: settings.SkillsEnabled && value.Enabled && branch.Target.EngineID == "alva" && !busy, Arguments: composerArguments()}
		switch {
		case busy:
			entry.UnavailableReason = "finish the current Turn before selecting a Skill"
		case branch.Target.EngineID != "alva":
			entry.UnavailableReason = "managed project Skills require the Alva engine"
		case !settings.SkillsEnabled || !value.Enabled:
			entry.UnavailableReason = "project Skills are disabled; enable them explicitly in project settings"
		}
		entries = append(entries, entry)
	}
	return entries, ctx.Err()
}

// ComposerCatalog is a cold metadata query. Empty sessionID exposes only the
// product's application commands; it never invents a project or scans HOME.
func (s *HubService) ComposerCatalog(ctx context.Context, id string, query ComposerCatalogQuery) (ComposerCatalogPage, error) {
	var result ComposerCatalogPage
	if query.Kind != "" && query.Kind != ComposerCommandEntry && query.Kind != ComposerSkillEntry || query.Offset < 0 || query.Offset > 1026 || query.Limit < 0 || query.Limit > 100 || len(query.Search) > 256 || !utf8.ValidString(query.Search) || strings.ContainsRune(query.Search, 0) {
		return result, errors.New("invalid bounded composer catalog query")
	}
	if query.Limit == 0 {
		query.Limit = 20
	}
	entries, err := s.composerEntries(ctx, id, query.Kind != ComposerCommandEntry)
	if err != nil {
		return result, err
	}
	words := strings.Fields(strings.ToLower(query.Search))
	matched := make([]ComposerCatalogEntry, 0, len(entries))
	for _, entry := range entries {
		if query.Kind != "" && entry.Kind != query.Kind {
			continue
		}
		text := strings.ToLower(entry.Name + " " + entry.Description + " " + entry.Origin + " " + entry.Source)
		matches := true
		for _, word := range words {
			matches = matches && strings.Contains(text, word)
		}
		if matches {
			matched = append(matched, entry)
		}
	}
	result = ComposerCatalogPage{Entries: []ComposerCatalogEntry{}, Total: len(matched), Offset: query.Offset, Limit: query.Limit}
	if query.Offset < len(matched) {
		end := min(query.Offset+query.Limit, len(matched))
		result.Entries = matched[query.Offset:end]
	}
	return result, ctx.Err()
}

func (s *HubService) ComposerEntry(ctx context.Context, id, entryID string) (ComposerEntryDetail, error) {
	var result ComposerEntryDetail
	if len(entryID) > 72 || entryID != "builtin:plan" && entryID != "builtin:review" && (!strings.HasPrefix(entryID, "skill:") || !skill.ValidName(strings.TrimPrefix(entryID, "skill:"))) {
		return result, errors.New("unknown composer entry identity")
	}
	entries, err := s.composerEntries(ctx, id, strings.HasPrefix(entryID, "skill:"))
	if err != nil {
		return result, err
	}
	for _, entry := range entries {
		if entry.ID == entryID {
			result.Entry = entry
			break
		}
	}
	if result.Entry.ID == "" {
		return result, fmt.Errorf("composer entry %q: %w", entryID, os.ErrNotExist)
	}
	if result.Entry.Kind == ComposerSkillEntry {
		repository, err := s.projectSkills(ctx, id, false, false)
		if err != nil {
			return ComposerEntryDetail{}, err
		}
		defer repository.Close()
		resource, err := repository.Resource(ctx, result.Entry.Name, "SKILL.md")
		if err != nil {
			return ComposerEntryDetail{}, err
		}
		document, err := skill.Parse(resource.Bytes)
		if err != nil {
			return ComposerEntryDetail{}, err
		}
		if document.Meta.Name != result.Entry.Name {
			return ComposerEntryDetail{}, errors.New("selected Skill identity changed")
		}
		sum := sha256.Sum256(resource.Bytes)
		result.Document, result.SHA256 = &document, hex.EncodeToString(sum[:])
	}
	return result, ctx.Err()
}

func (s *HubService) PrepareComposerSelection(ctx context.Context, id string, request ComposerSelectionRequest) (ComposerPreparation, error) {
	var result ComposerPreparation
	if len(request.Arguments) > 1 {
		return result, errors.New("composer selection accepts at most one request argument")
	}
	for name, value := range request.Arguments {
		if name != "request" || len(value) > 8192 || !utf8.ValidString(value) || strings.ContainsRune(value, 0) {
			return result, errors.New("composer selection accepts only a bounded UTF-8 request argument")
		}
	}
	detail, err := s.ComposerEntry(ctx, id, request.EntryID)
	if err != nil {
		return result, err
	}
	if !detail.Entry.Available {
		return result, errors.New(detail.Entry.UnavailableReason)
	}
	entry := detail.Entry
	result = ComposerPreparation{EntryID: entry.ID, Kind: entry.Kind, Origin: entry.Origin, Scope: entry.Scope}
	if value := request.Arguments["request"]; value != "" {
		result.Arguments = map[string]string{"request": value}
	}
	switch entry.ID {
	case "builtin:plan":
		result.Intent = ComposerIntentPlan
		result.Text = "请为下一项任务制定计划，列出步骤、风险和验证方式。"
	case "builtin:review":
		result.Text = "请审查当前项目的变更，指出可验证的问题、影响和建议。"
	default:
		product, err := s.Store.Detail(ctx, id)
		if err != nil {
			return ComposerPreparation{}, err
		}
		branch, err := ActiveBranch(product)
		if err != nil {
			return ComposerPreparation{}, err
		}
		result.SkillRef = &ComposerSkillReference{SessionID: id, BranchID: branch.ID, Name: entry.Name, RelativePath: entry.Source, SHA256: detail.SHA256, Policy: skill.Automatic}
		result.Text = "请使用项目 Skill「" + entry.Name + "」处理下一次请求。"
	}
	if value := result.Arguments["request"]; value != "" {
		result.Text += "\n\n" + value
	}
	return result, ctx.Err()
}

// RevalidateComposerPreparation must be called on the frozen Send selection.
// It rejects disabled, archived, changed, foreign-branch or altered references;
// it performs no submission, PlanMode change, Skill enablement or execution.
func (s *HubService) RevalidateComposerPreparation(ctx context.Context, id string, prepared ComposerPreparation) error {
	current, err := s.PrepareComposerSelection(ctx, id, ComposerSelectionRequest{EntryID: prepared.EntryID, Arguments: prepared.Arguments})
	if err != nil {
		return err
	}
	same := current.EntryID == prepared.EntryID && current.Kind == prepared.Kind && current.Origin == prepared.Origin && current.Scope == prepared.Scope && current.Intent == prepared.Intent && current.Text == prepared.Text && len(current.Arguments) == len(prepared.Arguments)
	for key, value := range current.Arguments {
		same = same && prepared.Arguments[key] == value
	}
	if current.SkillRef == nil || prepared.SkillRef == nil {
		same = same && current.SkillRef == nil && prepared.SkillRef == nil
	} else {
		same = same && *current.SkillRef == *prepared.SkillRef
	}
	if !same {
		return errors.New("composer preparation changed; review and select it again")
	}
	return ctx.Err()
}

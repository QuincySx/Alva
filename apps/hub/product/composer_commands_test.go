// INPUT: Real product stores, confined SKILL.md fixtures and cancelled composer requests
// OUTPUT: Cold bounded catalog, typed preparations and stale/disabled/foreign selection regressions
// POS: [L7 Hub product] Tests never execute a command, Skill script, external process or model request
package product

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/integration/protocol/skill"
)

func TestComposerCommandsPrepareDraftsWithoutExecution(t *testing.T) {
	s, requests := fixture(t)
	ctx := t.Context()
	page, err := s.ComposerCatalog(ctx, "", ComposerCatalogQuery{Limit: 1})
	if err != nil || page.Total != 2 || len(page.Entries) != 1 || page.Entries[0].ID != "builtin:plan" {
		t.Fatal(page, err)
	}
	page, err = s.ComposerCatalog(ctx, "", ComposerCatalogQuery{Search: "REVIEW", Offset: 0})
	if err != nil || page.Total != 1 || page.Entries[0].ID != "builtin:review" {
		t.Fatal("case-insensitive command search", page, err)
	}
	page, err = s.ComposerCatalog(ctx, "", ComposerCatalogQuery{Kind: ComposerSkillEntry})
	if err != nil || page.Total != 0 || len(page.Entries) != 0 {
		t.Fatal("homepage invented project Skills", page, err)
	}
	page, err = s.ComposerCatalog(ctx, "", ComposerCatalogQuery{Offset: 2})
	if err != nil || page.Total != 2 || len(page.Entries) != 0 {
		t.Fatal("empty page", page, err)
	}
	arguments := map[string]string{"request": "拆分导入流程并验证取消"}
	prepared, err := s.PrepareComposerSelection(ctx, "", ComposerSelectionRequest{EntryID: "builtin:plan", Arguments: arguments})
	if err != nil || prepared.Intent != ComposerIntentPlan || prepared.SkillRef != nil || prepared.Scope != "application" || !strings.Contains(prepared.Text, arguments["request"]) {
		t.Fatal(prepared, err)
	}
	arguments["request"] = "later edit"
	if prepared.Arguments["request"] == "later edit" {
		t.Fatal("preparation aliases caller's argument map")
	}
	product, err := s.Create(ctx, "bind homepage intent", "")
	if err != nil {
		t.Fatal(err)
	}
	before, err := s.Store.Settings(ctx, product.ID)
	if err != nil {
		t.Fatal(err)
	}
	if err = s.RevalidateComposerPreparation(ctx, product.ID, prepared); err != nil {
		t.Fatal("application intent cannot move from homepage to created project", err)
	}
	after, err := s.Store.Settings(ctx, product.ID)
	if err != nil || before.Version != after.Version || before.PlanMode != after.PlanMode {
		t.Fatal("preparing command changed project settings", err)
	}
	prepared.Text = "pretend to have executed"
	if err = s.RevalidateComposerPreparation(ctx, product.ID, prepared); err == nil {
		t.Fatal("altered preparation text was accepted")
	}
	if len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("catalog or preparation started an execution")
	}
}

func TestComposerProjectSkillSelectionPinsOriginalAndBranch(t *testing.T) {
	s, requests := fixture(t)
	ctx := t.Context()
	workspace := t.TempDir()
	product, err := s.Create(ctx, "composer skills", workspace)
	if err != nil {
		t.Fatal(err)
	}
	page, err := s.ComposerCatalog(ctx, product.ID, ComposerCatalogQuery{Kind: ComposerSkillEntry})
	if err != nil || page.Total != 0 {
		t.Fatal(page, err)
	}
	if _, err = os.Stat(filepath.Join(workspace, ".alva")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("cold catalog initialized project directory", err)
	}
	if err = s.InitializeSkills(ctx, product.ID); err != nil {
		t.Fatal(err)
	}
	raw := "---\nname: exact-review\ndescription: Carefully review test fixtures\ninvocation: explicit\nallowed-tools: [read_file]\nmetadata:\n  preserved: true\n---\nRead the selected changes; scripts remain data.\n"
	if err = s.InstallSkill(ctx, product.ID, raw); err != nil {
		t.Fatal(err)
	}
	page, err = s.ComposerCatalog(ctx, product.ID, ComposerCatalogQuery{Kind: ComposerSkillEntry, Search: "EXACT fixtures"})
	if err != nil || page.Total != 1 || len(page.Entries) != 1 || page.Entries[0].Available || page.Entries[0].UnavailableReason == "" {
		t.Fatal("disabled Skill was advertised as selectable", page, err)
	}
	entry, err := s.ComposerEntry(ctx, product.ID, "skill:exact-review")
	if err != nil || entry.Document == nil || entry.Document.Body != "Read the selected changes; scripts remain data." {
		t.Fatal("cold exact Skill details", entry, err)
	}
	sum := sha256.Sum256([]byte(raw))
	if entry.SHA256 != hex.EncodeToString(sum[:]) {
		t.Fatal("detail does not hash original bytes", entry.SHA256)
	}
	selection := ComposerSelectionRequest{EntryID: "skill:exact-review", Arguments: map[string]string{"request": "重点检查路径逃逸"}}
	if _, err = s.PrepareComposerSelection(ctx, product.ID, selection); err == nil {
		t.Fatal("disabled Skill selected")
	}
	settings, err := s.Store.Settings(ctx, product.ID)
	if err != nil || settings.SkillsEnabled {
		t.Fatal("selection silently enabled Skills", err)
	}
	if err = s.Store.SetSkillsEnabled(ctx, product.ID, true); err != nil {
		t.Fatal(err)
	}
	prepared, err := s.PrepareComposerSelection(ctx, product.ID, selection)
	if err != nil || prepared.Kind != ComposerSkillEntry || prepared.SkillRef == nil {
		t.Fatal(prepared, err)
	}
	ref := prepared.SkillRef
	if ref.SessionID != product.ID || ref.BranchID != product.ActiveBranchID || ref.Name != "exact-review" || ref.Policy != skill.Automatic || ref.SHA256 != entry.SHA256 || ref.RelativePath != ".alva/skills/exact-review/SKILL.md" {
		t.Fatal("selection lost typed identity or original fingerprint", ref)
	}
	if err = s.RevalidateComposerPreparation(ctx, product.ID, prepared); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(workspace, filepath.FromSlash(ref.RelativePath))
	if err = os.WriteFile(path, []byte(raw+"\nChanged after selection.\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err = s.RevalidateComposerPreparation(ctx, product.ID, prepared); err == nil {
		t.Fatal("changed original was accepted")
	}
	if err = os.WriteFile(path, []byte(raw), 0600); err != nil {
		t.Fatal(err)
	}
	if err = s.Store.SetSkillsEnabled(ctx, product.ID, false); err != nil {
		t.Fatal(err)
	}
	if err = s.RevalidateComposerPreparation(ctx, product.ID, prepared); err == nil {
		t.Fatal("disabled selection remained valid")
	}
	if err = s.Store.SetSkillsEnabled(ctx, product.ID, true); err != nil {
		t.Fatal(err)
	}
	target := s.DefaultTarget
	target.ModelID = "another-fixture-model"
	if _, err = s.Store.Switch(ctx, product.ID, target, "explicit branch switch"); err != nil {
		t.Fatal(err)
	}
	if err = s.RevalidateComposerPreparation(ctx, product.ID, prepared); err == nil {
		t.Fatal("parked branch selection was accepted")
	}
	other, err := s.Create(ctx, "other owner", workspace)
	if err != nil {
		t.Fatal(err)
	}
	if err = s.Store.SetSkillsEnabled(ctx, other.ID, true); err != nil {
		t.Fatal(err)
	}
	if err = s.RevalidateComposerPreparation(ctx, other.ID, prepared); err == nil {
		t.Fatal("foreign product selection was accepted")
	}
	if _, err = s.ArchiveSkill(ctx, product.ID, ref.Name); err != nil {
		t.Fatal(err)
	}
	if _, err = s.ComposerEntry(ctx, product.ID, prepared.EntryID); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("archived Skill remained in active catalog", err)
	}
	if len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("Skill selection started an execution")
	}
}

func TestComposerSelectionRejectsBusyAndUnsupportedScope(t *testing.T) {
	s, _ := fixture(t)
	ctx := t.Context()
	product, err := s.Create(ctx, "foreign engine", "")
	if err != nil {
		t.Fatal(err)
	}
	target := s.DefaultTarget
	target.EngineID = "external-fixture"
	if _, err = s.Store.Switch(ctx, product.ID, target, "explicit external engine"); err != nil {
		t.Fatal(err)
	}
	entry, err := s.ComposerEntry(ctx, product.ID, "builtin:plan")
	if err != nil || entry.Entry.Available {
		t.Fatal("foreign engine promised plan mode", entry, err)
	}
	if _, err = s.PrepareComposerSelection(ctx, product.ID, ComposerSelectionRequest{EntryID: "builtin:plan"}); err == nil {
		t.Fatal("unsupported plan intent selected")
	}
	if _, err = s.PrepareComposerSelection(ctx, product.ID, ComposerSelectionRequest{EntryID: "builtin:review"}); err != nil {
		t.Fatal("plain review draft unexpectedly required Alva", err)
	}
	if _, err = s.Store.BeginTurn(ctx, product.ID, "already claimed"); err != nil {
		t.Fatal(err)
	}
	if _, err = s.PrepareComposerSelection(ctx, product.ID, ComposerSelectionRequest{EntryID: "builtin:review"}); err == nil {
		t.Fatal("running product accepted selection")
	}
}

func TestComposerCatalogRejectsInvalidQueriesArgumentsAndCancellation(t *testing.T) {
	s, _ := fixture(t)
	for _, query := range []ComposerCatalogQuery{
		{Kind: "missing"}, {Offset: -1}, {Offset: 1027}, {Limit: -1}, {Limit: 101},
		{Search: strings.Repeat("x", 257)}, {Search: "\x00"}, {Search: string([]byte{0xff})},
	} {
		if _, err := s.ComposerCatalog(t.Context(), "", query); err == nil {
			t.Fatalf("invalid query accepted: %#v", query)
		}
	}
	for _, arguments := range []map[string]string{
		{"unknown": "value"}, {"request": strings.Repeat("x", 8193)}, {"request": "\x00"}, {"request": string([]byte{0xff})},
	} {
		if _, err := s.PrepareComposerSelection(t.Context(), "", ComposerSelectionRequest{EntryID: "builtin:review", Arguments: arguments}); err == nil {
			t.Fatalf("invalid arguments accepted: %#v", arguments)
		}
	}
	for _, identity := range []string{"", "builtin:shell", "skill:../outside", "skill:missing-skill"} {
		if _, err := s.PrepareComposerSelection(t.Context(), "", ComposerSelectionRequest{EntryID: identity}); err == nil {
			t.Fatalf("missing or invalid identity accepted: %q", identity)
		}
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err := s.ComposerCatalog(ctx, "", ComposerCatalogQuery{}); !errors.Is(err, context.Canceled) {
		t.Fatal("catalog ignored cancellation", err)
	}
	if _, err := s.ComposerEntry(ctx, "", "builtin:review"); !errors.Is(err, context.Canceled) {
		t.Fatal("detail ignored cancellation", err)
	}
	if _, err := s.PrepareComposerSelection(ctx, "", ComposerSelectionRequest{EntryID: "builtin:review"}); !errors.Is(err, context.Canceled) {
		t.Fatal("selection ignored cancellation", err)
	}
	if err := s.RevalidateComposerPreparation(ctx, "", ComposerPreparation{EntryID: "builtin:review"}); !errors.Is(err, context.Canceled) {
		t.Fatal("revalidation ignored cancellation", err)
	}
}

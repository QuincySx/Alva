// INPUT: The actual managed Skill repository and a frozen companion name/source/hash scope.
// OUTPUT: One restricted repository for Skill catalog, instructions and resource operations.
// POS: [L7 Hub assembly] Companion selections constrain real model-visible capabilities without changing the SDK.
package bootstrap

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"path/filepath"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/protocol/skill"
	"github.com/QuincySx/Alva/platform/storage"
)

type companionSkillRepository struct {
	base  skill.Repository
	scope map[string]string
}

// Source session/branch/workspace ownership is checked by product admission and
// assembly before supplying this actual repository. Every operation here also
// checks the selected original SKILL.md bytes, including the catalog path.
func filterCompanionSkills(repository skill.Repository, scope *hub.CompanionSkillScope) (skill.Repository, error) {
	if scope == nil {
		return repository, nil
	}
	if err := scope.Validate(); err != nil {
		return nil, err
	}
	if len(scope.Skills) > 0 && storage.Missing(repository) {
		return nil, errors.New("scoped companion Skills require their managed repository")
	}
	r := &companionSkillRepository{base: repository, scope: make(map[string]string, len(scope.Skills))}
	for _, ref := range scope.Skills {
		if !skill.ValidName(ref.Name) {
			return nil, errors.New("scoped companion Skill requires a valid managed name")
		}
		r.scope[ref.Name] = strings.ToLower(ref.SHA256)
	}
	return r, nil
}

// filterComposerSkills reuses the same hash-pinned repository for the two
// consumers: a companion's saved allowlist and this Turn's explicit selection.
// Applying both filters intersects authority; selection never expands a saved
// companion scope or enables the full project catalog.
func filterComposerSkills(repository skill.Repository, workspace string, inputs []engine.InputSkill, companion *hub.CompanionSkillScope) (skill.Repository, error) {
	base, err := filterCompanionSkills(repository, companion)
	if err != nil || len(inputs) == 0 {
		return base, err
	}
	if err := engine.ValidateInputSkills(inputs); err != nil {
		return nil, err
	}
	r := &companionSkillRepository{base: base, scope: make(map[string]string, len(inputs))}
	for _, input := range inputs {
		if !skill.ValidName(input.Name) || input.Path != filepath.Join(workspace, product.ManagedSkillDirectory, input.Name, "SKILL.md") {
			return nil, errors.New("selected Skill is outside the actual managed project repository")
		}
		r.scope[input.Name] = input.SHA256
	}
	return r, nil
}

func (r *companionSkillRepository) document(ctx context.Context, name string) (skill.Document, skill.Resource, error) {
	if err := ctx.Err(); err != nil {
		return skill.Document{}, skill.Resource{}, err
	}
	ref, ok := r.scope[name]
	if !ok {
		return skill.Document{}, skill.Resource{}, fmt.Errorf("Skill %q is outside the frozen companion scope", name)
	}
	original, err := r.base.Resource(ctx, name, "SKILL.md")
	if err = errors.Join(err, ctx.Err()); err != nil {
		return skill.Document{}, skill.Resource{}, err
	}
	sum := sha256.Sum256(original.Bytes)
	if original.Path != "SKILL.md" || hex.EncodeToString(sum[:]) != ref {
		return skill.Document{}, skill.Resource{}, fmt.Errorf("pinned companion Skill %q changed; select and review its source again", name)
	}
	document, err := skill.Parse(original.Bytes)
	if err != nil {
		return skill.Document{}, skill.Resource{}, err
	}
	if document.Meta.Name != name {
		return skill.Document{}, skill.Resource{}, errors.New("pinned companion Skill source identity changed")
	}
	return document, original, nil
}

func (r *companionSkillRepository) List(ctx context.Context) ([]skill.Skill, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if len(r.scope) == 0 {
		return []skill.Skill{}, nil
	}
	list, err := r.base.List(ctx)
	if err = errors.Join(err, ctx.Err()); err != nil {
		return nil, err
	}
	result := make([]skill.Skill, 0, len(r.scope))
	seen := make(map[string]bool, len(r.scope))
	for _, entry := range list {
		if _, allowed := r.scope[entry.Meta.Name]; !allowed {
			continue
		}
		if seen[entry.Meta.Name] || !entry.Enabled {
			return nil, errors.New("pinned companion Skill catalog identity changed")
		}
		document, _, err := r.document(ctx, entry.Meta.Name)
		if err != nil {
			return nil, err
		}
		entry.Meta = document.Meta
		result = append(result, entry)
		seen[entry.Meta.Name] = true
	}
	if len(result) != len(r.scope) {
		return nil, errors.New("pinned companion Skill is unavailable in its managed source")
	}
	return result, ctx.Err()
}

func (r *companionSkillRepository) Load(ctx context.Context, name string) (skill.Document, error) {
	document, _, err := r.document(ctx, name)
	return document, err
}

func (r *companionSkillRepository) Resources(ctx context.Context, name string) ([]string, error) {
	if _, _, err := r.document(ctx, name); err != nil {
		return nil, err
	}
	paths, err := r.base.Resources(ctx, name)
	if err = errors.Join(err, ctx.Err()); err != nil {
		return nil, err
	}
	if _, _, err = r.document(ctx, name); err != nil {
		return nil, err
	}
	return paths, nil
}

func (r *companionSkillRepository) Resource(ctx context.Context, name, relative string) (skill.Resource, error) {
	_, original, err := r.document(ctx, name)
	if err != nil {
		return skill.Resource{}, err
	}
	if relative == "SKILL.md" {
		return original, nil
	}
	resource, err := r.base.Resource(ctx, name, relative)
	if err = errors.Join(err, ctx.Err()); err != nil {
		return skill.Resource{}, err
	}
	if _, _, err = r.document(ctx, name); err != nil {
		return skill.Resource{}, err
	}
	return resource, nil
}

var _ skill.Repository = (*companionSkillRepository)(nil)

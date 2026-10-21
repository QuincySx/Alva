// INPUT: Frozen Snapshot, fixed session/reference backend and optional existing Skill repository.
// OUTPUT: Plugin and the existing progressive skill Tool, without a second agent runtime.
// POS: [L4 extension] Contributes selected rules and one merged Skill catalog through existing Registrar slots.
package compatibility

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"reflect"
	"sort"
	"strings"
	"sync"

	skillext "github.com/QuincySx/Alva/extension/skill"
	"github.com/QuincySx/Alva/integration/protocol/skill"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

type Plugin struct {
	snapshot *Snapshot
	skills   *skillext.Plugin
}

// Plugin reuses the ordinary Skill tool. If base is supplied, it is merged into
// that one catalog; callers must not also register a separate Skills plugin.
// The caller owns Snapshot.Close after the Agent's execution has joined.
func (s *Snapshot) Plugin(sessionID string, references agentcontext.ReferenceBackend, base skill.Repository) *Plugin {
	p := &Plugin{snapshot: s}
	if s == nil {
		return p
	}
	if len(s.skills) > 0 || base != nil {
		p.skills = skillext.New(&mergedRepository{snapshot: s, base: base}, sessionID, references)
	}
	return p
}
func (*Plugin) Name() string { return "compatibility" }
func (p *Plugin) Tools() []abi.Tool {
	if p.skills == nil {
		return nil
	}
	return p.skills.Tools()
}
func (p *Plugin) Register(ctx context.Context, r *core.Registrar) error {
	if p.snapshot == nil || p.snapshot.templates == nil || len(p.snapshot.receipt.SHA256) != 64 {
		return errors.New("compatibility snapshot required")
	}
	p.snapshot.mu.RLock()
	closed := p.snapshot.closed
	p.snapshot.mu.RUnlock()
	if closed {
		return errors.New("compatibility snapshot closed")
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if p.snapshot.prompt != "" {
		if err := r.SystemPrompt(p.snapshot.prompt); err != nil {
			return err
		}
	}
	type templateEntry struct {
		Name        string `json:"agent_type"`
		Description string `json:"description"`
	}
	var catalog []templateEntry
	for _, template := range p.snapshot.templates.List() {
		catalog = append(catalog, templateEntry{template.Name, template.Description})
	}
	if len(catalog) > 0 {
		data, err := json.Marshal(catalog)
		if err != nil {
			return err
		}
		if len(data) > 64<<10 {
			return errors.New("compatibility template catalog exceeds 64 KiB")
		}
		if err := r.SystemPrompt("Imported Agent template metadata (instruction bodies are not parent context). If the host installed the agent tool, select its exact agent_type to delegate. All tools still require actual direct-parent grants and independent permissions. Templates without tools request no host tools:\n" + string(data)); err != nil {
			return err
		}
	}
	if p.skills != nil {
		return p.skills.Register(ctx, r)
	}
	return nil
}

func copyValue[T any](value T) (T, error) {
	var copy T
	data, err := json.Marshal(value)
	if err == nil {
		err = json.Unmarshal(data, &copy)
	}
	return copy, err
}
func (s *Snapshot) List(ctx context.Context) ([]skill.Skill, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	if err := s.available(ctx); err != nil {
		return nil, err
	}
	var list []skill.Skill
	for _, value := range s.skills {
		list = append(list, value.value)
	}
	sort.Slice(list, func(i, j int) bool { return list[i].Meta.Name < list[j].Meta.Name })
	return copyValue(list)
}
func (s *Snapshot) available(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if s.closed {
		return errors.New("compatibility snapshot closed")
	}
	return nil
}
func (s *Snapshot) Load(ctx context.Context, name string) (skill.Document, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	if err := s.available(ctx); err != nil {
		return skill.Document{}, err
	}
	value, ok := s.skills[name]
	if !ok {
		return skill.Document{}, os.ErrNotExist
	}
	return copyValue(value.document)
}
func (s *Snapshot) Resources(ctx context.Context, name string) ([]string, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	if err := s.available(ctx); err != nil {
		return nil, err
	}
	value, ok := s.skills[name]
	if !ok {
		return nil, os.ErrNotExist
	}
	return value.repository.Resources(ctx, name)
}
func (s *Snapshot) Resource(ctx context.Context, name, relative string) (skill.Resource, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	if err := s.available(ctx); err != nil {
		return skill.Resource{}, err
	}
	value, ok := s.skills[name]
	if !ok {
		return skill.Resource{}, os.ErrNotExist
	}
	if strings.EqualFold(relative, "SKILL.md") {
		return skill.Resource{}, errors.New("use load for the frozen skill instructions")
	}
	return value.repository.Resource(ctx, name, relative)
}

type mergedRepository struct {
	snapshot *Snapshot
	base     skill.Repository
	mu       sync.Mutex
	ready    bool
	catalog  []skill.Skill
	owners   map[string]skill.Repository
	frozen   map[string]skill.Document
}

func (r *mergedRepository) initialize(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	r.snapshot.mu.RLock()
	available := r.snapshot.available(ctx)
	r.snapshot.mu.RUnlock()
	if available != nil {
		return available
	}
	if r.ready {
		return nil
	}
	list, err := r.snapshot.List(ctx)
	if err != nil {
		return err
	}
	entries := map[string]skill.Skill{}
	owners := map[string]skill.Repository{}
	frozen := map[string]skill.Document{}
	for _, entry := range list {
		entries[entry.Meta.Name] = entry
		owners[entry.Meta.Name] = r.snapshot
	}
	if r.base != nil {
		value := reflect.ValueOf(r.base)
		switch value.Kind() {
		case reflect.Pointer, reflect.Map, reflect.Slice, reflect.Interface, reflect.Func, reflect.Chan:
			if value.IsNil() {
				return errors.New("native skill repository is typed-nil")
			}
		}
		base, err := r.base.List(ctx)
		if err = errors.Join(err, ctx.Err()); err != nil {
			return err
		}
		if len(base) > 1024 {
			return errors.New("native skill catalog exceeds 1024")
		}
		budget := int64(resourceComparisonBytes)
		for _, entry := range base {
			name := entry.Meta.Name
			if !skill.ValidName(name) {
				return errors.New("invalid native skill identity")
			}
			if existing, exists := entries[name]; exists {
				if existing.Enabled != entry.Enabled {
					return conflict("skill availability", name, existing.Source, entry.Source)
				}
				left, ok := frozen[name]
				if !ok {
					left, err = owners[name].Load(ctx, name)
					if err = errors.Join(err, ctx.Err()); err != nil {
						return err
					}
				}
				right, err := r.base.Load(ctx, name)
				if err = errors.Join(err, ctx.Err()); err != nil {
					return err
				}
				if err := sameSkill(ctx, name, left, owners[name], existing.Source, right, r.base, entry.Source, &budget); err != nil {
					return err
				}
				frozen[name], err = copyValue(right)
				if err != nil {
					return err
				}
				entry.Meta = right.Meta
			}
			// Equal duplicates use the host's existing native repository. No tool or
			// permission sets are combined; non-native roots retain explicit first-source order.
			entries[name], owners[name] = entry, r.base
			if len(entries) > 1024 {
				return errors.New("combined skill catalog exceeds 1024")
			}
		}
	}
	list = nil
	for _, entry := range entries {
		list = append(list, entry)
	}
	sort.Slice(list, func(i, j int) bool { return list[i].Meta.Name < list[j].Meta.Name })
	r.catalog, err = copyValue(list)
	if err != nil {
		return err
	}
	r.owners, r.frozen, r.ready = owners, frozen, true
	return ctx.Err()
}
func (r *mergedRepository) List(ctx context.Context) ([]skill.Skill, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if err := r.initialize(ctx); err != nil {
		return nil, err
	}
	return copyValue(r.catalog)
}
func (r *mergedRepository) repository(ctx context.Context, name string) (skill.Repository, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if err := r.initialize(ctx); err != nil {
		return nil, err
	}
	repository := r.owners[name]
	if repository == nil {
		return nil, os.ErrNotExist
	}
	return repository, nil
}
func (r *mergedRepository) Load(ctx context.Context, name string) (skill.Document, error) {
	r.mu.Lock()
	if err := r.initialize(ctx); err != nil {
		r.mu.Unlock()
		return skill.Document{}, err
	}
	if document, found := r.frozen[name]; found {
		result, err := copyValue(document)
		r.mu.Unlock()
		return result, err
	}
	repository := r.owners[name]
	r.mu.Unlock()
	if repository == nil {
		return skill.Document{}, os.ErrNotExist
	}
	return repository.Load(ctx, name)
}
func (r *mergedRepository) Resources(ctx context.Context, name string) ([]string, error) {
	repository, err := r.repository(ctx, name)
	if err != nil {
		return nil, err
	}
	return repository.Resources(ctx, name)
}
func (r *mergedRepository) Resource(ctx context.Context, name, relative string) (skill.Resource, error) {
	repository, err := r.repository(ctx, name)
	if err != nil {
		return skill.Resource{}, err
	}
	return repository.Resource(ctx, name, relative)
}

// INPUT: Explicit workspace, Config, pinned Skill repositories and rule/agent files.
// OUTPUT: Snapshot, Open, OpenFile, Receipt, Fingerprint and owned Close.
// POS: [L4 extension] Freezes selected instructions per turn; resource bytes remain explicit on-demand reads.
package compatibility

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strings"
	"sync"

	"github.com/QuincySx/Alva/extension/subagent"
	"github.com/QuincySx/Alva/integration/protocol/skill"
)

type Source struct {
	Kind   string `json:"kind"`
	Path   string `json:"path"`
	SHA256 string `json:"sha256"`
}
type Receipt struct {
	SHA256  string   `json:"sha256"`
	Scope   string   `json:"scope"`
	Sources []Source `json:"sources"`
	Skills  []string `json:"skills"`
	Agents  []string `json:"agents"`
}
type frozenSkill struct {
	value      skill.Skill
	document   skill.Document
	repository *skill.FS
}
type Snapshot struct {
	mu        sync.RWMutex
	closed    bool
	files     []*skill.FS
	skills    map[string]frozenSkill
	templates *subagent.Templates
	prompt    string
	receipt   Receipt
}

// OpenFile reads only this explicit project-relative JSON file. Nothing is
// created, discovered in HOME, executed or installed by inspecting a snapshot.
func OpenFile(ctx context.Context, workspace, relative string) (*Snapshot, error) {
	if !filepath.IsAbs(workspace) {
		return nil, errors.New("absolute compatibility workspace required")
	}
	root, err := os.OpenRoot(workspace)
	if err != nil {
		return nil, err
	}
	defer root.Close()
	data, err := readFile(ctx, root, relative, MaxConfigBytes)
	if err != nil {
		return nil, err
	}
	config, err := ParseConfig(data)
	if err != nil {
		return nil, err
	}
	return open(ctx, root, config)
}

func Open(ctx context.Context, workspace string, config Config) (*Snapshot, error) {
	if !filepath.IsAbs(workspace) {
		return nil, errors.New("absolute compatibility workspace required")
	}
	root, err := os.OpenRoot(workspace)
	if err != nil {
		return nil, err
	}
	defer root.Close()
	return open(ctx, root, config)
}

func open(ctx context.Context, root *os.Root, config Config) (result *Snapshot, resultErr error) {
	if err := config.Validate(); err != nil {
		return nil, err
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	config = uniqueSources(config)
	s := &Snapshot{skills: map[string]frozenSkill{}}
	defer func() {
		if resultErr != nil {
			resultErr = errors.Join(resultErr, s.Close(context.WithoutCancel(ctx)))
		}
	}()
	scope := config.Scope
	if scope == "" {
		scope = "."
	}
	s.receipt.Scope = scope
	scopeRoot, err := directory(root, scope)
	if err != nil {
		return nil, err
	}
	if err := scopeRoot.Close(); err != nil {
		return nil, err
	}
	var prompt strings.Builder
	if len(config.Rules) > 0 {
		prompt.WriteString("Project rules for the explicitly selected scope below. Apply outer rules first; nearer directories and later listed formats take precedence on conflicts. User requests and host permissions remain authoritative. Rules for other directories are not loaded. @imports and include directives are literal text, never expanded.\n")
	}
	directories := []string{"."}
	if scope != "." {
		parts := strings.Split(scope, "/")
		for i := range parts {
			directories = append(directories, strings.Join(parts[:i+1], "/"))
		}
	}
	names := map[string]string{"agents": "AGENTS.md", "claude": "CLAUDE.md", "gemini": "GEMINI.md"}
	for _, directory := range directories {
		for _, format := range config.Rules {
			name := path.Join(directory, names[format])
			data, err := readFile(ctx, root, name, 64<<10)
			if errors.Is(err, os.ErrNotExist) {
				continue
			}
			if err != nil {
				return nil, fmt.Errorf("rule %s: %w", name, err)
			}
			if strings.HasPrefix(strings.ReplaceAll(string(data), "\r\n", "\n"), "---\n") {
				return nil, fmt.Errorf("rule %s: conditional frontmatter is unsupported; select its directory with scope", name)
			}
			s.receipt.Sources = append(s.receipt.Sources, source("rule:"+format, name, data))
			fmt.Fprintf(&prompt, "\n[Project rule source: %s; selected scope: %s]\n%s\n[End project rule source: %s]\n", name, scope, data, name)
			if prompt.Len() > 256<<10 {
				return nil, errors.New("project rules exceed 256 KiB prompt budget")
			}
		}
	}
	s.prompt = prompt.String()
	total := 0
	comparisonBudget := int64(resourceComparisonBytes)
	for _, selection := range config.Skills {
		// Open relative to the already pinned workspace, rather than resolving
		// its textual path again if the caller replaces the workspace pathname.
		pinned, err := directory(root, selection.Path)
		if err != nil {
			return nil, err
		}
		var repository *skill.FS
		// The protocol constructor takes ownership of a duplicate of this root.
		repository, err = skill.OpenPinned(pinned, selection.Format == "agent-skills")
		pinned.Close()
		if err != nil {
			return nil, err
		}
		s.files = append(s.files, repository)
		list, err := repository.List(ctx)
		if err != nil {
			return nil, err
		}
		for _, entry := range list {
			doc, err := repository.Load(ctx, entry.Meta.Name)
			if err != nil {
				return nil, err
			}
			data, err := json.Marshal(doc)
			if err != nil {
				return nil, err
			}
			total += len(data)
			if total > 32<<20 {
				return nil, errors.New("compatibility skill snapshot exceeds 1024 entries or 32 MiB")
			}
			entry.Meta = doc.Meta
			entry.Source = path.Join(selection.Path, entry.Meta.Name)
			if existing, exists := s.skills[entry.Meta.Name]; exists {
				if err := sameSkill(ctx, entry.Meta.Name, existing.document, existing.repository, existing.value.Source, doc, repository, entry.Source, &comparisonBudget); err != nil {
					return nil, err
				}
			} else {
				if len(s.skills) >= 1024 {
					return nil, errors.New("compatibility skill count exceeds 1024")
				}
				s.skills[entry.Meta.Name] = frozenSkill{entry, doc, repository}
				s.receipt.Skills = append(s.receipt.Skills, entry.Meta.Name)
			}
			s.receipt.Sources = append(s.receipt.Sources, source("skill:"+selection.Format, path.Join(entry.Source, "SKILL.md"), data))
		}
	}
	var templates []subagent.Template
	agentSources := map[string]string{}
	agentDefinitions := map[string][]byte{}
	for _, selection := range config.Agents {
		pinned, err := directory(root, selection)
		if err != nil {
			return nil, err
		}
		dir, err := pinned.Open(".")
		if err != nil {
			pinned.Close()
			return nil, err
		}
		entries, readErr := dir.ReadDir(1025)
		closeErr := dir.Close()
		if readErr != nil && !errors.Is(readErr, io.EOF) || closeErr != nil || len(entries) > 1024 {
			pinned.Close()
			return nil, errors.Join(errors.New("cannot read bounded agent directory"), readErr, closeErr)
		}
		sort.Slice(entries, func(i, j int) bool { return entries[i].Name() < entries[j].Name() })
		for _, entry := range entries {
			if strings.HasPrefix(entry.Name(), ".") || !strings.HasSuffix(entry.Name(), ".md") {
				continue
			}
			data, err := readFile(ctx, pinned, entry.Name(), 20<<10)
			if err != nil {
				pinned.Close()
				return nil, err
			}
			template, err := parseAgent(data)
			if err != nil {
				pinned.Close()
				return nil, fmt.Errorf("agent %s: %w", entry.Name(), err)
			}
			if template.Name != strings.TrimSuffix(entry.Name(), ".md") {
				pinned.Close()
				return nil, errors.New("agent filename and name must match")
			}
			origin := path.Join(selection, entry.Name())
			definition, err := templateDefinition(template)
			if err != nil {
				pinned.Close()
				return nil, err
			}
			if previous, exists := agentDefinitions[template.Name]; exists {
				if string(previous) != string(definition) {
					pinned.Close()
					return nil, conflict("agent template", template.Name, agentSources[template.Name], origin)
				}
			} else {
				templates = append(templates, template)
				if len(templates) > 64 {
					pinned.Close()
					return nil, errors.New("agent template limit is 64")
				}
				agentDefinitions[template.Name] = definition
				agentSources[template.Name] = origin
				s.receipt.Agents = append(s.receipt.Agents, template.Name)
			}
			s.receipt.Sources = append(s.receipt.Sources, source("agent:claude", origin, data))
		}
		if err := pinned.Close(); err != nil {
			return nil, err
		}
	}
	s.templates, err = subagent.NewTemplates(templates)
	if err != nil {
		return nil, err
	}
	sort.Strings(s.receipt.Skills)
	sort.Strings(s.receipt.Agents)
	data, err := json.Marshal(struct {
		Config  Config
		Sources []Source
	}{config, s.receipt.Sources})
	if err != nil {
		return nil, err
	}
	s.receipt.SHA256 = digest(append([]byte("alva.compatibility.v1\x00"), data...))
	receipt, err := json.Marshal(s.receipt)
	if err != nil {
		return nil, err
	}
	if len(receipt) > 1<<20 {
		return nil, errors.New("compatibility source receipt exceeds 1 MiB; select fewer sources")
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return s, nil
}

func digest(data []byte) string                    { sum := sha256.Sum256(data); return hex.EncodeToString(sum[:]) }
func source(kind, name string, data []byte) Source { return Source{kind, name, digest(data)} }
func (s *Snapshot) Fingerprint() string            { return s.receipt.SHA256 }
func (s *Snapshot) Receipt() Receipt {
	r := s.receipt
	r.Sources = append([]Source(nil), r.Sources...)
	r.Skills = append([]string(nil), r.Skills...)
	r.Agents = append([]string(nil), r.Agents...)
	return r
}
func (s *Snapshot) Templates() subagent.TemplateRegistry { return s.templates }
func (s *Snapshot) Close(context.Context) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return nil
	}
	var result error
	for _, repository := range s.files {
		result = errors.Join(result, repository.Close())
	}
	s.closed = true
	return result
}
func (s *Snapshot) CleanupComplete() bool { s.mu.RLock(); defer s.mu.RUnlock(); return s.closed }

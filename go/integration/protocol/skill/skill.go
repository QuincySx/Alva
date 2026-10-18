// INPUT: context, encoding/json, strings, unicode/utf8, YAML parser
// OUTPUT: Meta, Document, Skill, Repository, Parse, Summary, Injection
// POS: [integration / external protocol] Skill protocol values and progressive prompt construction without execution authority
package skill

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"unicode/utf8"

	"go.yaml.in/yaml/v3"
)

const MaxDocumentBytes = 1 << 20
const MaxSummaryBytes = 64 << 10

type Invocation string

const (
	Auto     Invocation = "auto"
	Explicit Invocation = "explicit"
)

type Policy string

const (
	Automatic Policy = "auto"
	Inline    Policy = "explicit"
	Strict    Policy = "strict"
)

type Meta struct {
	Name         string         `yaml:"name" json:"name"`
	Description  string         `yaml:"description" json:"description"`
	License      string         `yaml:"license,omitempty" json:"license,omitempty"`
	AllowedTools []string       `yaml:"allowed-tools,omitempty" json:"allowed_tools,omitempty"`
	Invocation   Invocation     `yaml:"invocation,omitempty" json:"invocation"`
	Metadata     map[string]any `yaml:"metadata,omitempty" json:"metadata,omitempty"`
}
type Document struct {
	Meta            Meta
	Body            string
	EstimatedTokens int
}
type Skill struct {
	Meta    Meta
	Source  string
	Enabled bool
}
type Resource struct {
	Path      string
	MediaType string
	Bytes     []byte
}
type Ref struct {
	Name   string
	Policy Policy
}
type Repository interface {
	List(context.Context) ([]Skill, error)
	Load(context.Context, string) (Document, error)
	Resources(context.Context, string) ([]string, error)
	Resource(context.Context, string, string) (Resource, error)
}

func ValidName(name string) bool {
	if len(name) == 0 || len(name) > 64 || name[0] == '-' || name[len(name)-1] == '-' {
		return false
	}
	for _, c := range name {
		if (c < 'a' || c > 'z') && (c < '0' || c > '9') && c != '-' {
			return false
		}
	}
	return true
}
func Parse(data []byte) (Document, error) {
	if len(data) > MaxDocumentBytes || !utf8.Valid(data) {
		return Document{}, errors.New("skill exceeds 1 MiB or is not UTF-8")
	}
	text := strings.ReplaceAll(string(data), "\r\n", "\n")
	if !strings.HasPrefix(text, "---\n") {
		return Document{}, errors.New("skill requires YAML frontmatter")
	}
	lines := strings.Split(text, "\n")
	end := -1
	for i := 1; i < len(lines); i++ {
		if lines[i] == "---" {
			end = i
			break
		}
	}
	if end < 0 {
		return Document{}, errors.New("unterminated skill frontmatter")
	}
	var meta Meta
	if err := yaml.Unmarshal([]byte(strings.Join(lines[1:end], "\n")), &meta); err != nil {
		return Document{}, fmt.Errorf("invalid skill frontmatter: %w", err)
	}
	if !ValidName(meta.Name) || len(meta.Description) == 0 || len(meta.Description) > 1024 || strings.ContainsAny(meta.Description, "<>") {
		return Document{}, errors.New("invalid skill name or description")
	}
	if meta.Invocation == "" {
		meta.Invocation = Auto
	}
	if meta.Invocation != Auto && meta.Invocation != Explicit {
		return Document{}, errors.New("invalid skill invocation")
	}
	if len(meta.AllowedTools) > 128 {
		return Document{}, errors.New("skill tool list exceeds limit")
	}
	for _, name := range meta.AllowedTools {
		if name == "" || len(name) > 128 || strings.ContainsAny(name, "\n\r\x00") {
			return Document{}, errors.New("invalid skill tool name")
		}
	}
	// Metadata must survive a pure-data boundary; YAML executable tags are not used.
	if _, err := json.Marshal(meta.Metadata); err != nil {
		return Document{}, errors.New("skill metadata is not JSON-compatible")
	}
	body := strings.TrimSpace(strings.Join(lines[end+1:], "\n"))
	return Document{Meta: meta, Body: body, EstimatedTokens: (len([]rune(body)) + 2) / 3}, nil
}
func Summary(skills []Skill) (string, error) {
	var out strings.Builder
	for _, s := range skills {
		if !s.Enabled || s.Meta.Invocation == Explicit {
			continue
		}
		if !ValidName(s.Meta.Name) {
			return "", errors.New("invalid skill catalog identity")
		}
		fmt.Fprintf(&out, "- %s: %s\n", s.Meta.Name, strings.ReplaceAll(s.Meta.Description, "\n", " "))
		if out.Len() > MaxSummaryBytes {
			return "", errors.New("skill metadata directory exceeds 64 KiB")
		}
	}
	if out.Len() == 0 {
		return "", nil
	}
	return "Available skills (use skill to load instructions by exact name):\n" + out.String(), nil
}
func Injection(ctx context.Context, repo Repository, refs []Ref) (string, error) {
	if len(refs) > 128 {
		return "", errors.New("too many selected skills")
	}
	list, err := repo.List(ctx)
	if err != nil {
		return "", err
	}
	index := make(map[string]Skill, len(list))
	for _, s := range list {
		index[s.Meta.Name] = s
	}
	var out strings.Builder
	for _, ref := range refs {
		s, exists := index[ref.Name]
		if !exists || !s.Enabled {
			return "", fmt.Errorf("selected skill %q is unavailable", ref.Name)
		}
		if ref.Policy == "" || ref.Policy == Automatic {
			// Exact selection may advertise an explicit-only skill without globally listing it.
			fmt.Fprintf(&out, "Skill %s: %s\n", s.Meta.Name, s.Meta.Description)
		} else if ref.Policy == Inline || ref.Policy == Strict {
			doc, loadErr := repo.Load(ctx, ref.Name)
			if loadErr != nil {
				return "", loadErr
			}
			fmt.Fprintf(&out, "\n## Skill %s\n%s\n", ref.Name, doc.Body)
			if ref.Policy == Strict && len(doc.Meta.AllowedTools) > 0 {
				fmt.Fprintf(&out, "Skill-declared tool restriction: %s. This declaration does not grant host permissions.\n", strings.Join(doc.Meta.AllowedTools, ", "))
			}
		} else {
			return "", errors.New("unknown skill injection policy")
		}
		if out.Len() > MaxDocumentBytes {
			return "", errors.New("selected skill injection exceeds 1 MiB")
		}
	}
	return out.String(), nil
}

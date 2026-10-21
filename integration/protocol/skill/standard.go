// INPUT: UTF-8 Agent Skills YAML frontmatter and Markdown, YAML parser.
// OUTPUT: ParseAgentSkills normalizes the explicitly selected standard into Document.
// POS: [integration / protocol] Format adapter only; tool declarations never grant execution authority.
package skill

import (
	"errors"
	"fmt"
	"strings"
	"unicode/utf8"

	"go.yaml.in/yaml/v3"
)

// ParseAgentSkills implements https://agentskills.io/specification. Vendor
// execution extensions are rejected, rather than silently enabling their policy.
func ParseAgentSkills(data []byte) (Document, error) {
	if len(data) > MaxDocumentBytes || !utf8.Valid(data) {
		return Document{}, errors.New("skill exceeds 1 MiB or is not UTF-8")
	}
	text := strings.ReplaceAll(string(data), "\r\n", "\n")
	if !strings.HasPrefix(text, "---\n") {
		return Document{}, errors.New("skill requires YAML frontmatter")
	}
	head, body, ok := strings.Cut(text[4:], "\n---\n")
	if !ok {
		// A closing delimiter at EOF is valid for an empty body.
		if !strings.HasSuffix(text[4:], "\n---") {
			return Document{}, errors.New("unterminated skill frontmatter")
		}
		head, body = strings.TrimSuffix(text[4:], "\n---"), ""
	}
	var raw struct {
		Name          string            `yaml:"name"`
		Description   string            `yaml:"description"`
		License       string            `yaml:"license"`
		Compatibility string            `yaml:"compatibility"`
		Metadata      map[string]string `yaml:"metadata"`
		AllowedTools  string            `yaml:"allowed-tools"`
	}
	var syntax yaml.Node
	if err := yaml.Unmarshal([]byte(head), &syntax); err != nil || len(syntax.Content) != 1 || syntax.Content[0].Kind != yaml.MappingNode {
		return Document{}, errors.New("Agent Skills frontmatter must be a mapping")
	}
	fields := syntax.Content[0].Content
	for i := 0; i < len(fields); i += 2 {
		value := fields[i+1]
		if fields[i].Value == "metadata" {
			if value.Kind != yaml.MappingNode {
				return Document{}, errors.New("Agent Skills metadata must map strings to strings")
			}
			for _, scalar := range value.Content {
				if scalar.Kind != yaml.ScalarNode || scalar.Tag != "!!str" {
					return Document{}, errors.New("Agent Skills metadata must map strings to strings")
				}
			}
		} else if value.Kind != yaml.ScalarNode || value.Tag != "!!str" {
			return Document{}, errors.New("Agent Skills frontmatter fields must be strings")
		}
	}
	decoder := yaml.NewDecoder(strings.NewReader(head))
	decoder.KnownFields(true)
	if err := decoder.Decode(&raw); err != nil {
		return Document{}, fmt.Errorf("invalid Agent Skills frontmatter: %w", err)
	}
	if !ValidName(raw.Name) || strings.Contains(raw.Name, "--") || strings.TrimSpace(raw.Description) == "" || utf8.RuneCountInString(raw.Description) > 1024 || utf8.RuneCountInString(raw.Compatibility) > 500 {
		return Document{}, errors.New("invalid Agent Skills name, description or compatibility")
	}
	meta := Meta{Name: raw.Name, Description: raw.Description, License: raw.License, Compatibility: raw.Compatibility, Invocation: Auto, AllowedTools: strings.Fields(raw.AllowedTools)}
	if len(meta.AllowedTools) > 128 {
		return Document{}, errors.New("skill tool list exceeds 128")
	}
	for _, tool := range meta.AllowedTools {
		if len(tool) > 128 || strings.ContainsRune(tool, 0) {
			return Document{}, errors.New("invalid skill tool declaration")
		}
	}
	if raw.Metadata != nil {
		meta.Metadata = make(map[string]any, len(raw.Metadata))
		for k, v := range raw.Metadata {
			meta.Metadata[k] = v
		}
	}
	body = strings.TrimSpace(body)
	return Document{Meta: meta, Body: body, EstimatedTokens: (utf8.RuneCountInString(body) + 2) / 3}, nil
}

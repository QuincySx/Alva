// INPUT: Bounded Claude-style agent Markdown definitions with YAML frontmatter.
// OUTPUT: Existing subagent.Template values with explicit tool-name normalization.
// POS: [L4 extension] Imports declarative templates without executing hooks, selecting credentials or granting tools.
package compatibility

import (
	"errors"
	"fmt"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/extension/subagent"
	"github.com/QuincySx/Alva/integration/protocol/skill"
	"go.yaml.in/yaml/v3"
)

func parseAgent(data []byte) (subagent.Template, error) {
	text := strings.ReplaceAll(string(data), "\r\n", "\n")
	if !strings.HasPrefix(text, "---\n") {
		return subagent.Template{}, errors.New("agent requires YAML frontmatter")
	}
	head, body, ok := strings.Cut(text[4:], "\n---\n")
	if !ok {
		return subagent.Template{}, errors.New("agent requires closing frontmatter and a prompt")
	}
	var raw struct {
		Name        string `yaml:"name"`
		Description string `yaml:"description"`
		Tools       string `yaml:"tools"`
		Model       string `yaml:"model"`
	}
	var syntax yaml.Node
	if err := yaml.Unmarshal([]byte(head), &syntax); err != nil || len(syntax.Content) != 1 || syntax.Content[0].Kind != yaml.MappingNode {
		return subagent.Template{}, errors.New("agent frontmatter must be a mapping")
	}
	for _, field := range syntax.Content[0].Content {
		if field.Kind != yaml.ScalarNode || field.Tag != "!!str" {
			return subagent.Template{}, errors.New("supported agent fields must be strings")
		}
	}
	decoder := yaml.NewDecoder(strings.NewReader(head))
	decoder.KnownFields(true)
	if err := decoder.Decode(&raw); err != nil {
		return subagent.Template{}, fmt.Errorf("unsupported or invalid agent frontmatter: %w", err)
	}
	if !skill.ValidName(raw.Name) || strings.Contains(raw.Name, "--") || strings.TrimSpace(raw.Description) == "" || strings.TrimSpace(body) == "" {
		return subagent.Template{}, errors.New("agent requires a valid name, description and prompt")
	}
	if raw.Model != "" && raw.Model != "inherit" {
		return subagent.Template{}, errors.New("compatibility agents currently require model: inherit; routes are selected by the host")
	}
	// These map names, not capabilities. NativeChildFactory still intersects
	// requested names with the actual direct-parent Tool instances and binders.
	aliases := map[string]string{"Read": "read_file", "Write": "write_file", "Edit": "edit_file", "Grep": "search_files"}
	var tools []string
	if raw.Tools != "" {
		for _, name := range strings.Split(raw.Tools, ",") {
			name = strings.TrimSpace(name)
			if target, found := aliases[name]; found {
				name = target
			}
			if name == "" || strings.ContainsAny(name, " ()\t\r\n") {
				return subagent.Template{}, errors.New("agent tools must be exact comma-separated names; patterns are unsupported")
			}
			tools = append(tools, name)
		}
	}
	template := subagent.Template{Name: raw.Name, Description: raw.Description, SystemPrompt: strings.TrimSpace(body), Tools: tools}
	if !utf8.ValidString(text) {
		return template, errors.New("agent is not UTF-8")
	}
	_, err := subagent.NewTemplates([]subagent.Template{template})
	return template, err
}

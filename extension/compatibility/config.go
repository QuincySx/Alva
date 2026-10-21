// INPUT: Explicit JSON format/source selections, confined project paths.
// OUTPUT: Config, SkillSource, ParseConfig and Validate.
// POS: [L4 extension] Opt-in convention adapters; no ambient discovery or host authorization.
package compatibility

import (
	"bytes"
	"encoding/json"
	"errors"
	"io/fs"
	"strings"

	"github.com/QuincySx/Alva/platform/system/strictjson"
)

const MaxConfigBytes = 32 << 10

type SkillSource struct {
	Path   string `json:"path"`
	Format string `json:"format"` // "alva" or "agent-skills"
}

type Config struct {
	Version int           `json:"version"`
	Scope   string        `json:"scope,omitempty"` // Existing directory, relative to workspace; default ".".
	Rules   []string      `json:"rules,omitempty"` // "agents", "claude", "gemini", in same-directory priority order.
	Skills  []SkillSource `json:"skills,omitempty"`
	Agents  []string      `json:"agents,omitempty"` // Explicit directories containing Claude-style .md definitions.
}

func ValidPath(value string) bool {
	return len(value) <= 1024 && fs.ValidPath(value) && !strings.ContainsAny(value, "\\\x00\r\n") && len(strings.Split(value, "/")) <= 16
}

func ParseConfig(data []byte) (Config, error) {
	var c Config
	if len(data) > MaxConfigBytes || strictjson.ValidateObject(data, &c) != nil {
		return c, errors.New("compatibility configuration must be one JSON object within 32 KiB")
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&c); err != nil {
		return c, errors.New("invalid compatibility configuration fields")
	}
	return c, c.Validate()
}

func (c Config) Validate() error {
	if c.Version != 1 || c.Scope != "" && !ValidPath(c.Scope) || len(c.Rules) > 3 || len(c.Skills) > 16 || len(c.Agents) > 8 || len(c.Rules)+len(c.Skills)+len(c.Agents) == 0 {
		return errors.New("version 1, a confined scope and bounded explicit compatibility sources required")
	}
	seen := map[string]bool{}
	for _, format := range c.Rules {
		if format != "agents" && format != "claude" && format != "gemini" || seen[format] {
			return errors.New("unique supported rule formats required")
		}
		seen[format] = true
	}
	for _, source := range c.Skills {
		if !ValidPath(source.Path) || source.Path == "." || source.Format != "alva" && source.Format != "agent-skills" {
			return errors.New("confined skill directories and explicit alva/agent-skills format required")
		}
	}
	for _, directory := range c.Agents {
		if !ValidPath(directory) || directory == "." {
			return errors.New("confined agent directories required")
		}
	}
	return nil
}

func uniqueSources(config Config) Config {
	skills := make([]SkillSource, 0, len(config.Skills))
	seen := map[SkillSource]bool{}
	for _, source := range config.Skills {
		if !seen[source] {
			seen[source] = true
			skills = append(skills, source)
		}
	}
	agents := make([]string, 0, len(config.Agents))
	directories := map[string]bool{}
	for _, directory := range config.Agents {
		if !directories[directory] {
			directories[directory] = true
			agents = append(agents, directory)
		}
	}
	config.Skills, config.Agents = skills, agents
	return config
}

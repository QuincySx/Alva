// INPUT: Bounded explicit next-turn product capability/process snapshots
// OUTPUT: Settings and capability approvals validated without protocol or process imports
// POS: [L7 Hub product] Private product configuration snapshots; not SDK policy or execution authority
package hub

import (
	"errors"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

type Settings struct {
	DurableWaits     bool            `json:"durable_waits"`
	Version          int             `json:"version"`
	SessionID        string          `json:"session_id"`
	MCP              []MCPServer     `json:"mcp"`
	AEP              []AEPBundle     `json:"aep,omitempty"`
	SkillsEnabled    bool            `json:"skills_enabled"`
	SubagentsEnabled bool            `json:"subagents_enabled"`
	PersistentMemory bool            `json:"persistent_memory"`
	MediaEnabled     bool            `json:"media_enabled"`
	MediaDecoder     MediaDecoder    `json:"media_decoder"`
	Browser          BrowserSettings `json:"browser"`
	Commands         CommandSettings `json:"commands"`
	Tools            ToolSettings    `json:"tools"`
	SkillArchives    []string        `json:"skill_archives,omitempty"`
}
type MCPServer struct {
	ID                 string            `json:"id"`
	Command            string            `json:"command"`
	Args               []string          `json:"args,omitempty"`
	Env                map[string]string `json:"env,omitempty"`
	Enabled            bool              `json:"enabled"`
	HostAccessApproved bool              `json:"host_access_approved"`
}

func (s Settings) Validate() error {
	if s.Version != 1 || s.SessionID == "" || len(s.MCP) > 32 || len(s.SkillArchives) > 128 {
		return errors.New("versioned session settings and at most 32 MCP servers required")
	}
	if err := ValidateAEP(s.AEP); err != nil {
		return err
	}
	if err := s.Commands.Validate(); err != nil {
		return err
	}
	if err := s.Tools.Validate(); err != nil {
		return err
	}
	if err := s.MediaDecoder.Validate(); err != nil {
		return err
	}
	if err := s.Browser.Validate(); err != nil {
		return err
	}
	for _, archive := range s.SkillArchives {
		if len(archive) == 0 || len(archive) > 256 || strings.ContainsAny(archive, "\x00\r\n") {
			return errors.New("invalid managed skill archive receipt")
		}
	}
	seen := map[string]bool{}
	total := 0
	for _, server := range s.MCP {
		if server.ID == "" || len(server.ID) > 128 || !utf8.ValidString(server.ID) || strings.ContainsAny(server.ID, "\x00\r\n") || seen[server.ID] || server.Command == "" || len(server.Command) > 4096 || strings.ContainsRune(server.Command, 0) || (!filepath.IsAbs(server.Command) && strings.ContainsAny(server.Command, "/\\")) || len(server.Args) > 128 || len(server.Env) > 64 {
			return errors.New("invalid or duplicated explicit MCP process snapshot")
		}
		seen[server.ID] = true
		if server.Enabled && !server.HostAccessApproved {
			return errors.New("enabled MCP requires explicit host access approval")
		}
		total += len(server.Command)
		for _, arg := range server.Args {
			if strings.ContainsRune(arg, 0) {
				return errors.New("invalid MCP argument")
			}
			total += len(arg)
		}
		for key, value := range server.Env {
			if key == "" || len(key) > 128 || strings.ContainsAny(key, "=\x00") || len(value) > 8192 || strings.ContainsRune(value, 0) {
				return errors.New("invalid explicit MCP environment")
			}
			total += len(key) + len(value)
		}
	}
	if total > 64<<10 {
		return errors.New("MCP settings exceed 64 KiB total snapshot budget")
	}
	return nil
}

// INPUT: Stored next-turn planning choice and explicit per-execution system prompt.
// OUTPUT: A narrowed native recipe and planning intent, without editing the saved capability approvals.
// POS: [L7 composition] Plan combines SDK read-only Guard enforcement with omission of process-capable optional integrations.
package bootstrap

import "github.com/QuincySx/Alva/apps/hub"

func planExecutionSettings(s hub.Settings) hub.Settings {
	if !s.PlanMode {
		return s
	}
	// In particular Hooks and AEP can execute during setup/lifecycle before a
	// model tool reaches the Guard. Omit process/network integrations entirely.
	// This is a value-only recipe projection: stored approvals survive exit.
	s.MCP, s.AEP = nil, nil
	s.Commands = hub.CommandSettings{}
	s.Browser.Enabled = false
	s.MediaEnabled, s.SubagentsEnabled = false, false
	s.Tools.Shell.Enabled, s.Tools.Web.Enabled = false, false
	return s
}

func planSystemPrompt(prompt string, enabled bool) string {
	if !enabled {
		return prompt
	}
	if prompt != "" {
		prompt += "\n\n"
	}
	return prompt + "Plan mode is enabled for this entire execution. Inspect relevant information with the available read tools and return a proposed plan as text. The native Guard denies file writes, execution, network and unknown effects without asking for approval; a tool name in the catalog does not grant those effects. Process/network integrations (Hooks, diagnostics, AEP, MCP, Shell, Web, Browser, media and SubAgents) are omitted from this recipe. Configured planning tools may inspect existing tasks, but their mutations and todo_write are denied too. Do not claim to have applied changes, run validation commands, saved plan notes, or exited plan mode. Only the user can change the next-Turn plan choice while no Turn or wait is active; disabling it restores Ask without authorizing any operation."
}

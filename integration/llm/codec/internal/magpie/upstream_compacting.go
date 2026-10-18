// INPUT: strings.
// OUTPUT: Private protocol helpers; no exported declarations.
// POS: Internal pinned upstream conversion slice; Alva owns admission and transport.
// Derived from yetone/magpie e085d6e9262786815c0f6c5ff65c5a585916cc9e, MIT, copyright 2026 yetone.
package magpie

import (
	strings "strings"
)

// compactLead is how far into the system prompt, or into a text part of the
// last user message, an agent's compaction prompt may begin. Each agent puts
// its prompt first or behind a short lead of its own (Claude Code's billing
// header and identity line; its "CRITICAL: Respond with TEXT ONLY" preamble,
// about 420 bytes, before the ask). Further in, it is something quoting the
// prompt: a memory or a note an agent puts in the system prompt behind its
// own, every turn (#1635, jueda12: a Minis memory holding Minis's compaction
// line sent every request of that conversation to the compaction rule's
// model), or a user writing about it.
const compactLead = 2048

// compactSystems open (or are) the system prompt of a compaction request.
var compactSystems = []string{

	"You are a helpful AI assistant tasked with summarizing conversations.",

	"You are a context summarization agent.",

	"You are a context summarization assistant.",

	"You are a specialized system component responsible for distilling chat history into a structured XML <state_snapshot>",

	"You are the component that summarizes a conversation when its context window is about to overflow.",

	"You are a context compaction engine. Your summary will REPLACE the original messages in the conversation context window.",

	"You are a helpful assistant that compacts conversation context.",
}

// compactAsk is said in a text part of the last user message of a
// compaction request: within compactLead of the part's start, or, for an
// agent that sends the conversation ahead of it in the same part, anywhere
// in a part that opens with that agent's own first line (opens).
type compactAsk struct {
	ask, opens string
}

// compactAsks are in the last user message of a compaction request.
var compactAsks = []compactAsk{

	{ask: "Your task is to create a detailed summary of the conversation so far"},

	{ask: "You are performing a CONTEXT CHECKPOINT COMPACTION."},

	{ask: "You are now given a task to compact this conversation context"},

	{ask: "END OF CONVERSATION TO COMPACT.", opens: "Compact this conversation into a context summary:"},
}

// leads reports whether p begins within compactLead bytes of text's start.
func leads(text, p string) bool {
	i := strings.Index(text, p)
	return i >= 0 && i <= compactLead
}

// compacting reports whether req is an agent compacting its conversation.
func compacting(req *Request) bool {
	if req == nil {
		return false
	}
	for _, p := range compactSystems {
		if leads(req.System, p) {
			return true
		}
	}
	for i := len(req.Messages) - 1; i >= 0; i-- {
		m := req.Messages[i]
		if m.Role != "user" {
			continue
		}
		for _, p := range m.Parts {
			if p.Kind != Text {
				continue
			}
			for _, a := range compactAsks {
				if a.opens == "" && leads(p.Text, a.ask) ||
					a.opens != "" && strings.HasPrefix(strings.TrimSpace(p.Text), a.opens) && strings.Contains(p.Text, a.ask) {
					return true
				}
			}
		}
		break
	}
	return false
}

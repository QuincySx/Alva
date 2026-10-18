// INPUT: context, encoding/json, errors, fmt, strings, time, kernel/abi, llm/wire
// OUTPUT: DefaultApply, Externalizer, SafeWindow
// POS: [L3 capability library] Applies context injections and real compression/externalization to detached provider-bound messages.
package agentcontext

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"strings"
	"time"
)

type Externalizer interface {
	Externalize(context.Context, string, string) (wire.Reference, error)
}
type DefaultApply struct {
	References   ReferenceBackend
	Externalizer Externalizer
	// A nil Summarizer uses the local extractive fallback. Bind a Store's
	// SummarizeEntries explicitly to use that Store's configured callback.
	Summarizer Summarizer
}

func (DefaultApply) ApplyInjections(ctx context.Context, injections []abi.Injection, prompt *[]string, messages *[]wire.Message) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if len(*prompt) == 0 {
		*prompt = []string{"", ""}
	} else if len(*prompt) == 1 {
		*prompt = append(*prompt, "")
	}
	for _, injection := range injections {
		stable := len(*prompt) - 2
		dynamic := len(*prompt) - 1
		switch injection.Kind {
		case abi.InjectSystem:
			if injection.Section == nil {
				return errors.New("system injection section missing")
			}
			(*prompt)[stable] += "\n\n" + injection.Section.Content
		case abi.InjectSkill:
			(*prompt)[stable] += fmt.Sprintf("\n\n<skill name=%q>\n%s\n</skill>", injection.SkillName, injection.Text)
		case abi.InjectRuntime:
			(*prompt)[dynamic] += "\n\n<runtime>\n" + injection.Text + "\n</runtime>"
		case abi.InjectMessage:
			if injection.Message == nil {
				return errors.New("message injection payload missing")
			}
			if err := injection.Message.Validate(); err != nil {
				return err
			}
			*messages = append(*messages, wire.CloneMessages([]wire.Message{*injection.Message})[0])
		case abi.InjectMemory:
			var facts []string
			for _, fact := range injection.Memory {
				facts = append(facts, "- "+fact.Text)
			}
			if len(facts) > 0 {
				(*prompt)[stable] += "\n\n<user_memory>\n" + strings.Join(facts, "\n") + "\n</user_memory>"
			}
		default:
			return errors.New("unknown context injection")
		}
	}
	return nil
}
func (apply DefaultApply) ApplyCompressions(ctx context.Context, actions []abi.CompressAction, messages *[]wire.Message, handle abi.ContextHandle, agentID string) error {
	for _, action := range actions {
		if err := ctx.Err(); err != nil {
			return err
		}
		switch action.Kind {
		case abi.CompressWindow:
			if action.KeepRecent < 0 {
				return errors.New("negative compression window")
			}
			cut := safeWindowCut(*messages, action.KeepRecent)
			keep := make([]bool, len(*messages))
			for i, message := range *messages {
				entry, exists := handle.ReadMessage(agentID, message.ID)
				if i >= cut || message.Role == wire.RoleSystem || (exists && (entry.Metadata.Priority == abi.Critical || entry.Metadata.Layer == abi.AlwaysPresent)) {
					keep[i] = true
				}
			}
			*messages = retainProtocolGroups(*messages, keep)
		case abi.CompressPriority:
			keep := make([]bool, len(*messages))
			for i, message := range *messages {
				entry, ok := handle.ReadMessage(agentID, message.ID)
				if !ok || entry.Metadata.Priority != action.Priority || entry.Metadata.Priority == abi.Critical {
					keep[i] = true
				}
			}
			*messages = retainProtocolGroups(*messages, keep)
		case abi.CompressToolResult:
			found := false
			for i, message := range *messages {
				if message.ID != action.MessageID {
					continue
				}
				for j, block := range message.Content {
					if block.ToolResult != nil {
						result := *block.ToolResult
						result.Content = []wire.ContentBlock{{Kind: wire.ContentText, Text: action.Summary}}
						message.Content[j].ToolResult = &result
						(*messages)[i] = message
						found = true
					}
				}
			}
			if !found {
				return errors.New("compression target tool result not found")
			}
		case abi.CompressSummary, abi.CompressExternalize:
			entries := make([]abi.ContextEntry, len(*messages))
			for i, message := range *messages {
				entries[i] = abi.ContextEntry{ID: message.ID, Message: message}
			}
			from, to, err := resolveRange(action.Range, entries)
			if err != nil {
				return err
			}
			if from == to {
				continue
			}
			from, to = safeRange(*messages, from, to)
			var replacement wire.Message
			if action.Kind == abi.CompressSummary {
				timeout, cancel := context.WithTimeout(ctx, 5*time.Second)
				var summary string
				var err error
				if apply.Summarizer != nil {
					summary, err = apply.Summarizer.Summarize(timeout, cloneEntries(entries[from:to]), append([]string(nil), action.Hints...))
				} else {
					summary, err = summarizeEntries(timeout, entries[from:to])
				}
				err = errors.Join(err, timeout.Err())
				cancel()
				if err != nil {
					return err
				}
				replacement = wire.TextMessage(wire.RoleUser, "<conversation_summary>\n"+summary+"\n</conversation_summary>")
			} else {
				encoded, err := json.Marshal((*messages)[from:to])
				if err != nil {
					return err
				}
				var reference wire.Reference
				if action.Path != "" {
					if apply.Externalizer == nil {
						return errors.New("authorized externalizer unavailable")
					}
					reference, err = apply.Externalizer.Externalize(ctx, action.Path, string(encoded))
				} else {
					if apply.References == nil {
						return errors.New("reference externalizer unavailable")
					}
					reference, err = apply.References.StoreText(ctx, string(encoded))
				}
				if err != nil {
					return err
				}
				if err := reference.Validate(); err != nil {
					return err
				}
				if err := ctx.Err(); err != nil {
					return fmt.Errorf("artifact retained at %s; context unchanged: %w", reference.URI, err)
				}
				reference.Processing += "; Context messages serialized as JSON; original facts retained; reference only, not raw model delivery"
				replacement = wire.Message{Role: wire.RoleUser, Content: []wire.ContentBlock{{Kind: wire.ContentReference, Reference: &reference}}}
			}
			replacement.ID = fmt.Sprintf("context/%s/%d/%d", agentID, from, to)
			next := wire.CloneMessages((*messages)[:from])
			next = append(next, replacement)
			next = append(next, wire.CloneMessages((*messages)[to:])...)
			*messages = next
		default:
			return errors.New("unknown context compression action")
		}
	}
	return nil
}

// Retention expands across whole call/result groups, including assistant messages
// with several calls. A retained critical entry can never lose its protocol pair.
func retainProtocolGroups(messages []wire.Message, keep []bool) []wire.Message {
	indices := make(map[string][]int)
	for i, message := range messages {
		for _, block := range message.Content {
			if block.ToolCall != nil {
				indices[block.ToolCall.ID] = append(indices[block.ToolCall.ID], i)
			}
			if block.ToolResult != nil {
				indices[block.ToolResult.CallID] = append(indices[block.ToolResult.CallID], i)
			}
		}
	}
	for changed := true; changed; {
		changed = false
		for _, group := range indices {
			retained := false
			for _, i := range group {
				retained = retained || keep[i]
			}
			if retained {
				for _, i := range group {
					if !keep[i] {
						keep[i] = true
						changed = true
					}
				}
			}
		}
	}
	var result []wire.Message
	for i, message := range messages {
		if keep[i] {
			result = append(result, message)
		}
	}
	return wire.CloneMessages(result)
}
func SafeWindow(messages []wire.Message, keep int) []wire.Message {
	return wire.CloneMessages(messages[safeWindowCut(messages, keep):])
}
func safeWindowCut(messages []wire.Message, keep int) int {
	if keep < 0 {
		keep = 0
	}
	cut := len(messages) - keep
	if cut < 0 {
		return 0
	}
	for {
		next := cut
		calls := make(map[string]int)
		for i, message := range messages {
			for _, block := range message.Content {
				if block.ToolCall != nil {
					calls[block.ToolCall.ID] = i
				}
				if i >= cut && block.ToolResult != nil {
					if parent, ok := calls[block.ToolResult.CallID]; ok && parent < next {
						next = parent
					}
				}
			}
		}
		if next == cut {
			return cut
		}
		cut = next
	}
}
func safeRange(messages []wire.Message, from, to int) (int, int) {
	for {
		before, after := from, to
		calls := make(map[string]int)
		for i, message := range messages {
			for _, block := range message.Content {
				if block.ToolCall != nil {
					calls[block.ToolCall.ID] = i
				}
				if block.ToolResult != nil {
					if parent, ok := calls[block.ToolResult.CallID]; ok {
						if i >= from && i < to && parent < from {
							from = parent
						}
						if parent >= from && parent < to && i >= to {
							to = i + 1
						}
					}
				}
			}
		}
		if before == from && after == to {
			return from, to
		}
	}
}
func repairToolPairs(messages []wire.Message) []wire.Message {
	calls := make(map[string]bool)
	results := make(map[string]bool)
	for _, message := range messages {
		for _, block := range message.Content {
			if block.ToolCall != nil {
				calls[block.ToolCall.ID] = true
			}
			if block.ToolResult != nil {
				results[block.ToolResult.CallID] = true
			}
		}
	}
	var kept []wire.Message
	for _, message := range messages {
		var content []wire.ContentBlock
		for _, block := range message.Content {
			if block.ToolCall != nil && !results[block.ToolCall.ID] {
				continue
			}
			if block.ToolResult != nil && !calls[block.ToolResult.CallID] {
				continue
			}
			content = append(content, block)
		}
		if len(content) > 0 {
			message.Content = content
			kept = append(kept, message)
		}
	}
	return kept
}

// INPUT: Request-local Magpie values.
// OUTPUT: Private protocol helpers; no exported declarations.
// POS: Internal pinned upstream conversion slice; Alva owns admission and transport.
// Derived from yetone/magpie e085d6e9262786815c0f6c5ff65c5a585916cc9e, MIT, copyright 2026 yetone.
package magpie

// continueText is the user turn asked after a conversation the client
// ended with the assistant's message, for a model that takes no prefill.
const continueText = "Continue."

// compactLastText is that user turn when the conversation is an agent
// compacting it with the ask in the system prompt (compacting): "Continue."
// there reads as going on with the task, and the summary isn't written.
const compactLastText = "Write the summary of the conversation above that the system prompt asks for."

// modelLastAsk is the user turn put after a conversation ending with the
// model's turn, for an upstream that takes none (buildCodeAssistSent).
func modelLastAsk(r *Request) string {
	if compacting(r) {
		return compactLastText
	}
	return continueText
}

// endsWithAssistant says whether r's conversation ends with the
// assistant's message rather than a call it made: what a prefill is.
func (r *Request) endsWithAssistant() bool {
	n := len(r.Messages)
	if n == 0 || r.Messages[n-1].Role != "assistant" {
		return false
	}
	for _, p := range r.Messages[n-1].Parts {
		if p.Kind == ToolCall {
			return false
		}
	}
	return true
}

// userLast is r with a user turn after its last, the assistant's,
// message.
func (r *Request) userLast() *Request {
	q := *r
	q.Messages = append(q.Messages[:len(q.Messages):len(q.Messages)], Message{Role: "user", Parts: []Part{{Kind: Text, Text: continueText}}})
	return &q
}

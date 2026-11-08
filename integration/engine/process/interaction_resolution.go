// INPUT: The engine interaction contract, a pending native question with the response this adapter relayed, and the run's redaction secrets
// OUTPUT: relayedAnswers (offered choices only; free-text and secret answers kept as answered without a body) and publishedResolution (per-value redaction and validation that degrade, never fail)
// POS: [integration / native external-engine adapter] Resolved interaction facts never carry what a person typed, matching the native journal's answer-body omission
package process

import "github.com/QuincySx/Alva/integration/engine"

// relayedAnswers keeps, for each answered question, only the offered options
// the person chose. A free-text answer, or any answer to a secret question,
// is recorded as answered with no body: like the native journal, which omits
// every human response body, product facts never hold typed answers, so no
// literal-matching redaction has to recognize them.
func relayedAnswers(request engine.UserInputRequest, response engine.UserInputResponse) map[string][]string {
	questions := make(map[string]engine.UserInputQuestion, len(request.Questions))
	for _, q := range request.Questions {
		questions[q.ID] = q
	}
	out := make(map[string][]string, len(response.Answers))
	for id, values := range response.Answers {
		q, known := questions[id]
		kept := []string{}
		offered := known && !q.IsSecret && len(values) > 0
		for _, value := range values {
			found := false
			for _, option := range q.Options {
				found = found || value == option.Label
			}
			offered = offered && found
		}
		if offered {
			kept = append(kept, values...)
		}
		out[id] = kept
	}
	return out
}

// publishedResolution is what a resolved event may carry. With labels, each
// kept option label passes the run's redaction one value at a time (never
// keyed by question ID, so a credential-like ID cannot reshape the map); a
// label the redaction would change is kept only as answered. Without labels
// every answer is answered-only. A redaction or contract failure degrades to
// no resolution, shown as handled, instead of an error that ends the run.
func publishedResolution(resolution *engine.InteractionResolution, kind string, secrets []string, labels bool) *engine.InteractionResolution {
	if resolution == nil {
		return nil
	}
	clean := engine.InteractionResolution{}
	if resolution.Allowed != nil {
		allowed := *resolution.Allowed
		clean.Allowed = &allowed
	}
	if resolution.Answers != nil {
		clean.Answers = make(map[string][]string, len(resolution.Answers))
		for id, values := range resolution.Answers {
			kept := []string{}
			for _, value := range values {
				if !labels {
					kept = nil
					break
				}
				redacted, err := redactCodexText(value, secrets)
				if err != nil {
					return nil
				}
				if redacted != value {
					kept = nil
					break
				}
				kept = append(kept, value)
			}
			if kept == nil {
				kept = []string{}
			}
			clean.Answers[id] = kept
		}
	}
	if clean.Validate(kind) != nil {
		return nil
	}
	return &clean
}

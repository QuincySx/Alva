// INPUT: Known native-environment credential values and Codex incremental/completed output
// OUTPUT: Bounded run-local streaming redaction and recoverable completed native journal frames
// POS: [integration / native external-engine adapter] Human secret replies retain their separate stricter output-omission policy
package process

import (
	"encoding/json"
	"errors"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/redaction"
)

const codexStreamBufferBudget = 256 << 10

type codexStreamKey struct {
	id   string
	kind engine.Kind
}

// Both native transports use the same bounded literal stream filter. Execution
// and interaction ownership remain with their independent protocol runs.
type nativeStream struct {
	streamTails    map[codexStreamKey]string
	streamBytes    int
	streamKeep     int
	streamPatterns []string
}

func codexStreamKeep(secrets []string) (int, error) {
	longest := 0
	for _, value := range secrets {
		encoded, _ := json.Marshal(value)
		size := max(len(value), len(encoded)-2)
		if size > 16<<10 {
			return 0, errors.New("Codex credential stream redaction exceeds the 16 KiB literal bound")
		}
		longest = max(longest, size)
	}
	return max(0, longest-1), nil
}

func codexStreamPatterns(secrets []string) []string {
	// These are the literal forms accepted by the shared redaction policy:
	// original values, their JSON-escaped representation and multiline parts.
	known := make(map[string]bool)
	for _, value := range secrets {
		if value != "" {
			known[value] = true
		}
		encoded, _ := json.Marshal(value)
		if len(encoded) > 2 {
			known[string(encoded[1:len(encoded)-1])] = true
		}
		for _, line := range strings.FieldsFunc(value, func(r rune) bool { return r == '\n' || r == '\r' }) {
			known[line] = true
		}
	}
	patterns := make([]string, 0, len(known))
	for value := range known {
		patterns = append(patterns, value)
	}
	return patterns
}

func redactCodexText(value string, secrets []string) (string, error) {
	data, _ := json.Marshal(value)
	data, err := redaction.RedactJSON(data, secrets...)
	if err != nil {
		return "", errors.New("could not redact native stream text")
	}
	var clean string
	if json.Unmarshal(data, &clean) != nil {
		return "", errors.New("could not decode redacted native stream text")
	}
	return clean, nil
}

func (r *nativeStream) filterDelta(event engine.Event, secrets []string) (string, error) {
	if r.streamTails == nil {
		var err error
		r.streamKeep, err = codexStreamKeep(secrets)
		if err != nil {
			return "", err
		}
		r.streamTails = make(map[codexStreamKey]string)
		r.streamPatterns = codexStreamPatterns(secrets)
	}
	key := codexStreamKey{event.ID, event.Kind}
	previous := r.streamTails[key]
	if len(event.Text) > 64<<10 || r.streamBytes+len(event.Text) > codexStreamBufferBudget || previous == "" && len(r.streamTails) >= 64 {
		return "", errors.New("Codex credential stream redaction exceeded its bounded buffers")
	}
	// Retain raw undecided bytes. Never put generated [REDACTED] markers back
	// through the policy: a short credential may occur inside that marker.
	raw := previous + event.Text
	cut := max(0, len(raw)-r.streamKeep)
	for {
		before := cut
		for _, pattern := range r.streamPatterns {
			from := max(0, cut-len(pattern)+1)
			if index := strings.Index(raw[from:], pattern); index >= 0 && from+index < cut {
				cut = from + index
			}
		}
		if len(raw)-cut > 2*r.streamKeep+utf8.UTFMax {
			return "", errors.New("Codex overlapping credential literals exceed the bounded stream tail")
		}
		if cut == before {
			break
		}
	}
	for cut > 0 && cut < len(raw) && !utf8.RuneStart(raw[cut]) {
		cut--
	}
	if len(raw)-cut > 2*r.streamKeep+utf8.UTFMax {
		return "", errors.New("Codex overlapping credential literals exceed the bounded stream tail")
	}
	clean, err := redactCodexText(raw[:cut], secrets)
	if err != nil {
		return "", err
	}
	if len(clean) > 64<<10 {
		return "", errors.New("Codex redacted stream fragment exceeds its event budget")
	}
	r.streamTails[key] = raw[cut:]
	r.streamBytes += len(raw[cut:]) - len(previous)
	return clean, nil
}

func (r *nativeStream) clearStreams() {
	r.streamTails, r.streamBytes, r.streamPatterns = nil, 0, nil
}

func (r *codexRun) flushStreams(itemID string, completedMessage bool) error {
	r.rpc.journal.mu.Lock()
	private := r.rpc.journal.privateOutput
	secrets := append([]string(nil), r.rpc.journal.secrets...)
	r.rpc.journal.mu.Unlock()
	keys := make([]codexStreamKey, 0, len(r.streamTails))
	for key := range r.streamTails {
		if itemID == "" || key.id == itemID {
			keys = append(keys, key)
		}
	}
	sort.Slice(keys, func(i, j int) bool {
		if keys[i].id != keys[j].id {
			return keys[i].id < keys[j].id
		}
		return keys[i].kind < keys[j].kind
	})
	for _, key := range keys {
		value := r.streamTails[key]
		delete(r.streamTails, key)
		r.streamBytes -= len(value)
		// A completed aggregate supersedes the withheld message tail. It also
		// safely covers a native stream that stopped partway through a literal.
		if private || completedMessage && key.kind == engine.MessageDelta || value == "" {
			continue
		}
		clean, err := redactCodexText(value, secrets)
		if err != nil {
			return err
		}
		if err := r.publish(engine.Event{Kind: key.kind, SessionID: r.session, ID: key.id, Text: clean}); err != nil {
			return err
		}
	}
	return nil
}

func (j *journal) writeCodexStdout(frame codexFrame, body string) error {
	j.mu.Lock()
	protected := j.nativeStreamSecrets
	j.mu.Unlock()
	complete := frame.Method == "" || len(frame.ID) != 0
	switch frame.Method {
	case "item/started", "item/completed", "turn/started", "turn/completed", "thread/started", "thread/tokenUsage/updated", "thread/status/changed", "serverRequest/resolved", "error":
		complete = true
	}
	// Deltas and opaque notifications may split one credential across frames.
	// Completed tool/message/result and request/reply bodies remain recoverable
	// through the shared redaction policy, including oversized tool references.
	return j.writeBody("stdout", body, protected && !complete)
}

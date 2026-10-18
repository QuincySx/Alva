// INPUT: errors, strings, unicode/utf8
// OUTPUT: PermissionRules, RuleDecision, RulesAllow, RulesDeny, RulesAsk
// POS: [L3 capability library] Immutable configured tool/input patterns with deny-before-ask-before-allow precedence.
package security

import (
	"errors"
	"strings"
	"unicode/utf8"
)

type RuleDecision string

const (
	RulesAllow RuleDecision = "allow"
	RulesDeny  RuleDecision = "deny"
	RulesAsk   RuleDecision = "ask"
)

type PermissionRules struct {
	Allow    []string `json:"allow,omitempty"`
	Deny     []string `json:"deny,omitempty"`
	Ask      []string `json:"ask,omitempty"`
	ReadOnly []string `json:"read_only,omitempty"`
}

func (rules PermissionRules) Validate() error {
	count, bytes := 0, 0
	for _, group := range [][]string{rules.Allow, rules.Deny, rules.Ask, rules.ReadOnly} {
		for _, pattern := range group {
			count++
			bytes += len(pattern)
			if pattern == "" || len(pattern) > 2048 || !utf8.ValidString(pattern) {
				return errors.New("invalid permission rule pattern")
			}
			if opening := strings.IndexByte(pattern, '('); opening >= 0 && (opening == 0 || !strings.HasSuffix(pattern, ")")) {
				return errors.New("invalid permission argument pattern")
			}
		}
	}
	if count > 1024 || bytes > 256<<10 {
		return errors.New("permission rules exceed configured bounds")
	}
	return nil
}
func cloneRules(rules PermissionRules) PermissionRules {
	rules.Allow = append([]string(nil), rules.Allow...)
	rules.Deny = append([]string(nil), rules.Deny...)
	rules.Ask = append([]string(nil), rules.Ask...)
	rules.ReadOnly = append([]string(nil), rules.ReadOnly...)
	return rules
}
func (rules PermissionRules) Check(tool, input string) RuleDecision {
	decision, _ := rules.Match(tool, input)
	return decision
}
func (rules PermissionRules) Match(tool, input string) (RuleDecision, bool) {
	for _, group := range []struct {
		patterns []string
		decision RuleDecision
	}{{rules.Deny, RulesDeny}, {rules.Ask, RulesAsk}, {rules.Allow, RulesAllow}} {
		for _, pattern := range group.patterns {
			if matchRule(pattern, tool, input) {
				return group.decision, true
			}
		}
	}
	return RulesAsk, false
}

// Metadata only: the manager does not turn unknown dynamic tools into read effects.
func (rules PermissionRules) IsReadOnly(tool string) bool {
	for _, pattern := range rules.ReadOnly {
		if !strings.Contains(pattern, "(") && glob(pattern, tool) {
			return true
		}
	}
	return false
}
func matchRule(pattern, tool, input string) bool {
	opening := strings.IndexByte(pattern, '(')
	if opening < 0 {
		return glob(pattern, tool)
	}
	if !strings.HasSuffix(pattern, ")") {
		return false
	}
	return glob(pattern[:opening], tool) && glob(pattern[opening+1:len(pattern)-1], input)
}

// Both stars span arbitrary bytes, as in the concrete Rust matcher. This is
// neither filesystem authorization nor shell safety classification.
func glob(pattern, text string) bool {
	parts := strings.Split(pattern, "*")
	if len(parts) == 1 {
		return pattern == text
	}
	remaining := text
	if parts[0] != "" {
		if !strings.HasPrefix(remaining, parts[0]) {
			return false
		}
		remaining = remaining[len(parts[0]):]
	}
	for _, part := range parts[1 : len(parts)-1] {
		if part == "" {
			continue
		}
		position := strings.Index(remaining, part)
		if position < 0 {
			return false
		}
		remaining = remaining[position+len(part):]
	}
	last := parts[len(parts)-1]
	return last == "" || strings.HasSuffix(remaining, last)
}

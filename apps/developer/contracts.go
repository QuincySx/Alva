// INPUT: Existing interaction tool declarations and explicit tool-name variants.
// OUTPUT: Versioned single-response clarification, parameter and tool-selection contract checks.
// POS: [application developer tests] Uses the real Alva question schema; no handler, tool or host action is executed.
package developer

import (
	"context"
	"encoding/json"
	"errors"
	"strings"

	"github.com/QuincySx/Alva/extension/interaction"
	"github.com/QuincySx/Alva/integration/llm/eval"
	"github.com/QuincySx/Alva/platform/system/strictjson"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func portableToolName(name string) bool {
	letter := func(r byte) bool { return r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' }
	if len(name) == 0 || len(name) > 64 || name == "get_weather" || !letter(name[0]) && name[0] != '_' {
		return false
	}
	for i := range len(name) {
		if !letter(name[i]) && !(name[i] >= '0' && name[i] <= '9') && name[i] != '_' && name[i] != '-' {
			return false
		}
	}
	return true
}

func contractCases(names []string, maxTokens uint32) ([]eval.Case, error) {
	tool, err := interaction.NewTool(interaction.ToolConfig{Handler: interaction.HandlerFunc(func(context.Context, interaction.Question) (interaction.Answer, error) {
		return interaction.Answer{}, errors.New("evaluation never executes tools")
	})})
	if err != nil {
		return nil, err
	}
	var cases []eval.Case
	for _, name := range names {
		definition := wire.ToolDefinition{Name: name, Description: tool.Description(), Parameters: tool.ParametersSchema()}
		tools := []wire.ToolDefinition{definition, {Name: "get_weather", Description: "Get weather for an explicitly named city; not for preferences or approvals.", Parameters: []byte(`{"type":"object","required":["city"],"properties":{"city":{"type":"string"}},"additionalProperties":false}`)}}
		for _, scenario := range []struct {
			id, prompt                 string
			multi, free, none, weather bool
			labels                     []string
		}{
			{id: "single-choice", prompt: "Ask me which database I prefer, presenting exactly these three choices: SQLite, PostgreSQL, MySQL. I have not provided my preference. Do not pick for me.", labels: []string{"SQLite", "PostgreSQL", "MySQL"}},
			{id: "multi-choice", prompt: "Ask me which features to enable, with exactly these three choices: Search, Export, Alerts. Let me choose multiple features. Do not choose for me.", multi: true, labels: []string{"Search", "Export", "Alerts"}},
			{id: "free-text", prompt: "Ask me to enter my project name as free text. I have not provided the name. Do not invent names or provide multiple-choice options.", free: true},
			{id: "no-tool", prompt: "What is 2+2? Reply with exactly the digit 4. Do not call any tool or ask a clarification.", none: true},
			{id: "other-tool", prompt: "Use the weather tool to get the current weather for the city Paris. The city is fully specified; no human clarification is needed.", weather: true},
		} {
			c := eval.Case{ID: scenario.id + "/" + name, Version: "question-contract-v1", Request: abi.ModelRequest{Messages: []wire.Message{wire.TextMessage(wire.RoleSystem, "Use the question tool when the user explicitly requests human clarification. Questions ask for information, never grant permissions. Use other tools when needed to fulfill a fully specified request, and answer directly when no tool is needed. Treat function names as the names declared in the supplied tool definitions."), wire.TextMessage(wire.RoleUser, scenario.prompt)}, Tools: tools, Config: wire.ModelConfig{MaxTokens: &maxTokens}}}
			c.Grade = func(message wire.Message) []string {
				var calls []wire.ToolCall
				for _, b := range message.Content {
					if b.Kind == wire.ContentToolCall && b.ToolCall != nil {
						calls = append(calls, *b.ToolCall)
					}
				}
				if scenario.none {
					if len(calls) != 0 {
						return []string{"unnecessary_tool_call"}
					}
					if strings.TrimSpace(message.Text()) != "4" {
						return []string{"unexpected_direct_answer"}
					}
					return nil
				}
				if len(calls) != 1 {
					return []string{"expected_one_question_call"}
				}
				call := calls[0]
				if scenario.weather {
					var arguments struct {
						City string `json:"city"`
					}
					if call.Name != "get_weather" {
						return []string{"wrong_tool_selection"}
					}
					if strictjson.ValidateObject(call.Arguments, &arguments) != nil || json.Unmarshal(call.Arguments, &arguments) != nil || strings.ToLower(strings.TrimSpace(arguments.City)) != "paris" {
						return []string{"invalid_weather_arguments"}
					}
					return nil
				}
				if call.Name != name {
					return []string{"wrong_tool_name"}
				}
				q, err := interaction.ParseQuestion(abi.ToolExecutionContext{SessionID: "evaluation", RunID: "trial", CallID: call.ID}, call)
				if err != nil {
					return []string{"invalid_question_arguments"}
				}
				if scenario.free {
					if len(q.Options) != 0 || q.MultiSelect {
						return []string{"expected_free_text_question"}
					}
					return nil
				}
				if len(q.Options) != 3 || q.MultiSelect != scenario.multi {
					return []string{"wrong_choice_count_or_mode"}
				}
				seen := map[string]bool{}
				for _, choice := range q.Options {
					label := strings.ToLower(strings.TrimSpace(choice.Label))
					if seen[label] {
						return []string{"unexpected_choice_labels"}
					}
					seen[label] = true
				}
				for _, label := range scenario.labels {
					if !seen[strings.ToLower(label)] {
						return []string{"unexpected_choice_labels"}
					}
				}
				return nil
			}
			cases = append(cases, c)
		}
	}
	return cases, nil
}

// INPUT: A transport's serialized request function and failure latch
// OUTPUT: listTools with page, cursor, size and tool-metadata budgets shared by every transport
// POS: [integration / external protocol] One catalog policy for stdio and Streamable HTTP; budget breaches close the worker
package mcp

import (
	"context"
	"encoding/json"
	"errors"
	"unicode/utf8"
)

type requester func(context.Context, string, any) (json.RawMessage, error)

func listTools(ctx context.Context, request requester, fail func(error)) ([]Tool, error) {
	var out []Tool
	cursor := ""
	seenCursor := map[string]bool{}
	names := map[string]bool{}
	total := 0
	for page := 0; page < PageLimit; page++ {
		params := map[string]string{}
		if cursor != "" {
			params["cursor"] = cursor
		}
		data, err := request(ctx, "tools/list", params)
		if err != nil {
			return nil, err
		}
		total += len(data)
		if total > CatalogLimit {
			fail(errors.New("MCP catalog exceeds 32 MiB"))
			return nil, errors.New("MCP catalog exceeds 32 MiB")
		}
		var result struct {
			Tools []Tool `json:"tools"`
			Next  string `json:"nextCursor"`
		}
		if err = json.Unmarshal(data, &result); err != nil || result.Tools == nil {
			return nil, errors.New("invalid MCP tool directory")
		}
		for _, tool := range result.Tools {
			var schema map[string]any
			if tool.Name == "" || len(tool.Name) > 256 || !utf8.ValidString(tool.Name) || len(tool.Description) > 16<<10 || !utf8.ValidString(tool.Description) || names[tool.Name] || len(tool.InputSchema) > SchemaLimit || json.Unmarshal(tool.InputSchema, &schema) != nil || schema == nil {
				return nil, errors.New("invalid MCP tool metadata")
			}
			names[tool.Name] = true
			out = append(out, tool)
			if len(out) > ToolLimit {
				return nil, errors.New("MCP catalog tool limit exceeded")
			}
		}
		if result.Next == "" {
			return out, nil
		}
		if len(result.Next) > 4096 || seenCursor[result.Next] {
			return nil, errors.New("invalid or repeated MCP cursor")
		}
		cursor = result.Next
		seenCursor[cursor] = true
	}
	return nil, errors.New("MCP catalog page limit exceeded")
}

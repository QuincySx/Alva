// INPUT: context, encoding/json, fmt, filepath, sort, strings, unicode, yaml/v3, kernel/abi, llm/wire.
// OUTPUT: Explicit structured Markdown memory extraction and recoverable index refresh.
// POS: [L4 extension] Reuses authorized atomic file Tools; file/index/SQLite stages are reported separately, never called one transaction.
package memoryplugin

import (
	"context"
	"encoding/json"
	"fmt"
	"net/url"
	"path/filepath"
	"sort"
	"strings"
	"unicode"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"go.yaml.in/yaml/v3"
)

type frontmatter struct {
	Name        string `yaml:"name"`
	Description string `yaml:"description"`
	Type        string `yaml:"type"`
}

func sanitized(name string) string {
	var builder strings.Builder
	for _, character := range name {
		if unicode.IsLetter(character) || unicode.IsDigit(character) || character == '_' || character == '-' {
			builder.WriteRune(unicode.ToLower(character))
		} else {
			builder.WriteByte('_')
		}
	}
	return builder.String()
}
func (p *Plugin) extract(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall, args arguments) (wire.ToolResult, error) {
	directory := p.config.MemoryDirectory
	if err := p.config.Workspace.MakeDirectory(ctx, directory, p.config.Guard); err != nil {
		return wire.ToolResult{}, err
	}
	fileName := args.Type + "_" + sanitized(args.Name) + "_" + digest(args.Name)[:8] + ".md"
	path := filepath.Join(directory, fileName)
	meta, err := yaml.Marshal(frontmatter{Name: args.Name, Description: args.Description, Type: args.Type})
	if err != nil {
		return wire.ToolResult{}, err
	}
	text := "---\n" + string(meta) + "---\n\n" + args.Content
	fileWrite, err := p.file(ctx, execution, "write_file", map[string]any{"path": path, "content": text})
	if err != nil {
		return wire.ToolResult{}, fmt.Errorf("memory file write failed or is uncertain; no automatic retry: %w", err)
	}
	_, source, err := p.read(ctx, execution, path, 128<<10)
	if err != nil {
		return wire.ToolResult{}, fmt.Errorf("memory file was written but source receipt unavailable: %w", err)
	}
	entries, err := p.list(ctx, execution, directory)
	if err != nil {
		return wire.ToolResult{}, fmt.Errorf("memory file was written; index not refreshed: %w", err)
	}
	var lines []string
	excluded, totalBytes := 0, 0
	for _, entry := range entries {
		if entry.Directory || entry.Symlink || entry.Name == "MEMORY.md" || !strings.HasSuffix(entry.Name, ".md") {
			continue
		}
		if len(lines)+excluded >= 64 {
			return wire.ToolResult{}, fmt.Errorf("memory file was written; index exceeds 64-note budget")
		}
		body, _, readErr := p.read(ctx, execution, filepath.Join(directory, entry.Name), 128<<10)
		if readErr != nil {
			return wire.ToolResult{}, fmt.Errorf("memory file was written; index source unavailable: %w", readErr)
		}
		totalBytes += len(body)
		if totalBytes > 4<<20 {
			return wire.ToolResult{}, fmt.Errorf("memory file was written; index source exceeds 4 MiB budget")
		}
		if !strings.HasPrefix(body, "---\n") {
			excluded++
			continue
		}
		end := strings.Index(body[4:], "\n---\n")
		if end < 0 || end > 2048 {
			excluded++
			continue
		}
		var header frontmatter
		if yaml.Unmarshal([]byte(body[4:4+end]), &header) != nil || header.Name == "" || len(header.Name) > 128 || len(header.Description) > 256 || !memoryType(header.Type) || strings.ContainsAny(header.Name+header.Description, "\r\n\x00") {
			excluded++
			continue
		}
		label := strings.NewReplacer("\\", "\\\\", "[", "\\[", "]", "\\]").Replace(header.Name)
		lines = append(lines, "- ["+label+"]("+url.PathEscape(entry.Name)+") — "+header.Description)
	}
	sort.Strings(lines)
	indexWrite, err := p.file(ctx, execution, "write_file", map[string]any{"path": filepath.Join(directory, "MEMORY.md"), "content": "# Memory Index\n\n" + strings.Join(lines, "\n") + "\n"})
	if err != nil {
		return wire.ToolResult{}, fmt.Errorf("memory file was written; index update failed or is uncertain: %w", err)
	}
	if err = p.config.Service.StoreEntry(ctx, filepath.Join(p.config.Cwd, path), text, args.Type); err != nil {
		return wire.ToolResult{}, fmt.Errorf("memory Markdown/index were written but SQLite indexing failed; no automatic retry: %w", err)
	}
	var fileReceipt, indexReceipt any
	if len(fileWrite.Content) > 0 {
		_ = json.Unmarshal([]byte(fileWrite.Content[0].Text), &fileReceipt)
	}
	if len(indexWrite.Content) > 0 {
		_ = json.Unmarshal([]byte(indexWrite.Content[0].Text), &indexReceipt)
	}
	return value(call, map[string]any{"path": path, "source": source, "file_write": fileReceipt, "index_write": indexReceipt, "indexed_notes": len(lines), "excluded_unrecognized_notes": excluded, "processing": "Explicit structured note, YAML frontmatter and Markdown index only. Prior file/index bytes are retained by backup references. File, index and memory database are separate commits; no automatic LLM extraction, implicit account access or media processing."})
}

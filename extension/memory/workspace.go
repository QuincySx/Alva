// INPUT: context, crypto/sha256, encoding/json, filepath, sort, strings, time, memory FileReplacer, builtin file tools.
// OUTPUT: Authorized explicit workspace MEMORY.md synchronization with truthful bounded report.
// POS: [L4 extension] Reuses pinned filesystem capabilities and same-session references rather than inventing another host file boundary.
package memoryplugin

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/QuincySx/Alva/sdk/agent/memory"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type directoryEntry struct {
	Name      string `json:"name"`
	Directory bool   `json:"directory"`
	Symlink   bool   `json:"symlink"`
}

func (p *Plugin) file(ctx context.Context, execution abi.ToolExecutionContext, name string, args any) (wire.ToolResult, error) {
	data, err := json.Marshal(args)
	if err != nil {
		return wire.ToolResult{}, err
	}
	tool := p.files[name]
	if tool == nil {
		return wire.ToolResult{}, errors.New("memory filesystem capability unavailable")
	}
	result, err := tool.Execute(ctx, execution, wire.ToolCall{ID: execution.CallID, Name: name, Arguments: data})
	if err != nil {
		return result, err
	}
	if result.IsError {
		return result, errors.New("memory file operation rejected; no complete result")
	}
	return result, nil
}
func (p *Plugin) list(ctx context.Context, execution abi.ToolExecutionContext, path string) ([]directoryEntry, error) {
	result, err := p.file(ctx, execution, "list_directory", map[string]any{"path": path})
	if err != nil {
		return nil, err
	}
	var page struct {
		Entries   []directoryEntry `json:"entries"`
		Truncated bool             `json:"truncated"`
	}
	if len(result.Content) != 1 || json.Unmarshal([]byte(result.Content[0].Text), &page) != nil {
		return nil, errors.New("invalid memory directory receipt")
	}
	if page.Truncated {
		return nil, errors.New("memory directory exceeds 256 entries; no exhaustive sync claimed")
	}
	sort.Slice(page.Entries, func(i, j int) bool { return page.Entries[i].Name < page.Entries[j].Name })
	return page.Entries, nil
}
func (p *Plugin) read(ctx context.Context, execution abi.ToolExecutionContext, path string, maxBytes int64) (string, wire.Reference, error) {
	result, err := p.file(ctx, execution, "read_file", map[string]any{"path": path})
	if err != nil {
		return "", wire.Reference{}, err
	}
	if len(result.Content) != 1 || result.Content[0].Reference == nil {
		return "", wire.Reference{}, errors.New("memory source did not yield a retained reference")
	}
	reference := *result.Content[0].Reference
	if reference.Bytes > maxBytes {
		return "", reference, errors.New("memory source exceeds indexing byte budget; raw source retained, not indexed")
	}
	var text strings.Builder
	for offset := int64(0); ; {
		page, err := p.config.References.ReadText(ctx, reference.URI, offset, 4096)
		if err != nil {
			return "", reference, err
		}
		if page.Offset != offset || page.NextOffset != offset+int64(len(page.Text)) || page.TotalBytes != reference.Bytes || page.NextOffset > reference.Bytes || page.Truncated && page.NextOffset == offset {
			return "", reference, errors.New("invalid memory source page")
		}
		text.WriteString(page.Text)
		if int64(text.Len()) > maxBytes {
			return "", reference, ErrSourceBudget
		}
		if !page.Truncated {
			break
		}
		offset = page.NextOffset
	}
	if int64(text.Len()) != reference.Bytes {
		return "", reference, errors.New("incomplete retained memory source")
	}
	return text.String(), reference, nil
}

var ErrSourceBudget = errors.New("workspace memory scan budget exceeded; no exhaustive success claimed")

type syncReport struct {
	FilesScanned, FilesAdded, FilesUpdated, FilesUnchanged, ChunksCreated int
	ScannedBytes                                                          int    `json:"scanned_bytes"`
	EmbeddingFallback                                                     bool   `json:"embedding_fallback"`
	Policy                                                                string `json:"policy"`
}

func digest(text string) string {
	hash := sha256.Sum256([]byte(text))
	return hex.EncodeToString(hash[:])
}
func (p *Plugin) sync(ctx context.Context, execution abi.ToolExecutionContext) (syncReport, error) {
	report := syncReport{Policy: "Explicit MEMORY.md only; no symlink/hidden/node_modules/target traversal. At most 2048 nodes, 64 files, 8 MiB; no auto scan, media processing or deleted-file purge."}
	replacer, ok := p.config.Service.Store().(memory.FileReplacer)
	if !ok {
		return report, errors.New("memory backend lacks atomic complete-file replacement")
	}
	nodes := 0
	var walk func(string, int) error
	walk = func(path string, depth int) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		entries, err := p.list(ctx, execution, path)
		if err != nil {
			return err
		}
		for _, entry := range entries {
			nodes++
			if nodes > 2048 {
				return ErrSourceBudget
			}
			if entry.Symlink {
				continue
			}
			if entry.Directory {
				if depth >= 10 || strings.HasPrefix(entry.Name, ".") || entry.Name == "node_modules" || entry.Name == "target" {
					continue
				}
				if err := walk(filepath.Join(path, entry.Name), depth+1); err != nil {
					return err
				}
				continue
			}
			if entry.Name != "MEMORY.md" {
				continue
			}
			report.FilesScanned++
			if report.FilesScanned > 64 {
				return ErrSourceBudget
			}
			name := filepath.Join(path, entry.Name)
			text, reference, err := p.read(ctx, execution, name, 1<<20)
			if err != nil {
				return err
			}
			report.ScannedBytes += len(text)
			if report.ScannedBytes > 8<<20 {
				return ErrSourceBudget
			}
			key := filepath.Join(p.config.Cwd, name)
			old, found, err := p.config.Service.Store().GetFile(ctx, key)
			if err != nil {
				return err
			}
			if found && old.Hash == reference.SHA256 {
				report.FilesUnchanged++
				continue
			}
			chunks := chunk(text, key)
			texts := make([]string, len(chunks))
			for index := range chunks {
				texts[index] = chunks[index].Text
			}
			vectors, embedErr := p.config.Service.Embedder().Embed(ctx, texts)
			if ctx.Err() != nil {
				return ctx.Err()
			}
			if embedErr != nil || len(vectors) != len(chunks) {
				report.EmbeddingFallback = true
				vectors = make([][]float32, len(chunks))
			}
			for index := range chunks {
				chunks[index].Embedding = vectors[index]
			}
			if err = replacer.ReplaceFile(ctx, memory.File{Path: key, Source: "workspace", Hash: reference.SHA256, MTime: time.Now().UTC().Format(time.RFC3339Nano), Size: reference.Bytes}, chunks); err != nil {
				return err
			}
			if found {
				report.FilesUpdated++
			} else {
				report.FilesAdded++
			}
			report.ChunksCreated += len(chunks)
		}
		return nil
	}
	err := walk(".", 0)
	return report, err
}
func chunk(text, path string) []memory.Chunk {
	if text == "" {
		return nil
	}
	lines := strings.Split(strings.TrimSuffix(text, "\n"), "\n")
	var chunks []memory.Chunk
	for start := 0; start < len(lines); start += 50 {
		end := start + 50
		if end > len(lines) {
			end = len(lines)
		}
		body := strings.Join(lines[start:end], "\n")
		chunks = append(chunks, memory.Chunk{Path: path, Source: "workspace", StartLine: int64(start + 1), EndLine: int64(end), Hash: digest(body), Text: body})
	}
	return chunks
}

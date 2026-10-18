// INPUT: bytes, context, encoding/json, errors, io, io/fs, os, path, sort, strings, unicode/utf8, agent/context, kernel/abi, llm/wire, sandbox/abi
// OUTPUT: FileTools
// POS: [platform / WASM host or guest] Real guest-side WASI regular-file tools with volatile reference-first reads and bounded discovery/search.
package guest

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"os"
	"path"
	"sort"
	"strings"
	"unicode/utf8"

	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	kernelabi "github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"github.com/QuincySx/Alva/sdk/sandbox/abi"
)

type fileTool struct {
	config abi.Context
	refs   agentcontext.ReferenceBackend
	kind   string
}

func FileTools(config abi.Context, refs agentcontext.ReferenceBackend) []kernelabi.Tool {
	names := []string{"read_file", "list_directory", "find_files", "search_files"}
	if config.Policy.AllowFileWrite {
		names = append(names, "write_file", "edit_file")
	}
	out := make([]kernelabi.Tool, 0, len(names))
	for _, kind := range names {
		out = append(out, fileTool{config: config, refs: refs, kind: kind})
	}
	return out
}
func (tool fileTool) Name() string { return tool.kind }
func (tool fileTool) Description() string {
	switch tool.kind {
	case "read_file":
		return "Read UTF-8 regular file (<=20MiB) into a bounded job-local volatile reference. Retrieve selected content with context_read/search; no automatic full delivery."
	case "write_file":
		return "Atomically write UTF-8 file inside explicit WASI grants; keep prior text in a job-local reference before overwrite."
	case "edit_file":
		return "Replace one unambiguous exact text match, backing up prior text before atomic write."
	case "list_directory":
		return "List <=256 immediate entries; no recursive read."
	case "find_files":
		return "Find <=100 relative paths by glob, inspect <=4096 entries; truncation explicit."
	default:
		return "Literal UTF-8 file search with <=256 files/1MiB scan, <=20 snippets; truncation explicit."
	}
}
func (tool fileTool) ParametersSchema() json.RawMessage {
	fields := `"path":{"type":"string"}`
	required := `"path"`
	switch tool.kind {
	case "write_file":
		fields += `,"content":{"type":"string"}`
		required += `,"content"`
	case "edit_file":
		fields += `,"old":{"type":"string"},"new":{"type":"string"}`
		required += `,"old","new"`
	case "find_files":
		fields += `,"glob":{"type":"string"}`
		required += `,"glob"`
	case "search_files":
		fields += `,"query":{"type":"string"}`
		required += `,"query"`
	}
	return json.RawMessage(`{"type":"object","properties":{` + fields + `},"required":[` + required + `],"additionalProperties":false}`)
}
func (tool fileTool) Scheduling(call wire.ToolCall) kernelabi.Scheduling {
	return kernelabi.Scheduling{ExecutionMode: kernelabi.SerialGlobal, Resources: []kernelabi.ResourceClaim{{Resource: "guest-files:" + tool.config.SessionID, Access: map[bool]kernelabi.Access{true: kernelabi.AccessRead, false: kernelabi.AccessWrite}[tool.IsReadOnly(call)]}}}
}
func (tool fileTool) DescribeEffect(call wire.ToolCall) kernelabi.Effect {
	kind := kernelabi.EffectRead
	if !tool.IsReadOnly(call) {
		kind = kernelabi.EffectWrite
	}
	return kernelabi.Effect{Kind: kind, Description: tool.Description()}
}
func (tool fileTool) IsReadOnly(wire.ToolCall) bool {
	return tool.kind != "write_file" && tool.kind != "edit_file"
}

type fileArgs struct {
	Path    string `json:"path"`
	Content string `json:"content"`
	Old     string `json:"old"`
	New     string `json:"new"`
	Query   string `json:"query"`
	Glob    string `json:"glob"`
}

func decodeArgs(data json.RawMessage, value any) error {
	return abi.Decode(data, abi.MaxModelRequestBytes, value)
}
func (tool fileTool) Execute(ctx context.Context, execution kernelabi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if !fileToolsEnabled() {
		return wire.ToolResult{}, errors.New("guest file tools require actual wasip1/wasm execution")
	}
	if execution.SessionID != tool.config.SessionID || tool.refs == nil {
		return wire.ToolResult{}, errors.New("guest file capability belongs to another session")
	}
	if err := ctx.Err(); err != nil {
		return wire.ToolResult{}, err
	}
	var args fileArgs
	if err := decodeArgs(call.Arguments, &args); err != nil {
		return fileFailure(call, "Invalid file arguments"), nil
	}
	name := args.Path
	if !strings.HasPrefix(name, "/") {
		name = path.Join(tool.config.Workspace, name)
	}
	name = path.Clean(name)
	if !abi.GuestPath(name) {
		return fileFailure(call, "Invalid guest path"), nil
	}
	var data any
	var err error
	switch tool.kind {
	case "read_file":
		var text string
		text, err = readText(ctx, name, 20<<20)
		if err == nil {
			var ref wire.Reference
			ref, err = tool.refs.StoreText(ctx, text)
			if err == nil {
				ref.Processing = "Raw UTF-8 file; not compressed or analyzed; volatile reference valid only during this guest job"
				return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentReference, Reference: &ref}}}, nil
			}
		}
	case "write_file", "edit_file":
		if !tool.config.Policy.AllowFileWrite {
			return fileFailure(call, "Guest policy denies writes"), nil
		}
		if !utf8.ValidString(args.Content) || len(args.Content) > 20<<20 {
			return fileFailure(call, "File content must be UTF-8 within 20MiB"), nil
		}
		var backup *wire.Reference
		var previous string
		previous, err = readText(ctx, name, 20<<20)
		if errors.Is(err, fs.ErrNotExist) && tool.kind == "write_file" {
			err = nil
		} else if err == nil {
			ref, e := tool.refs.StoreText(ctx, previous)
			err = e
			backup = &ref
		}
		if err == nil {
			content := args.Content
			if tool.kind == "edit_file" {
				if args.Old == "" || strings.Count(previous, args.Old) != 1 {
					err = errors.New("edit requires exactly one old text match")
				} else {
					content = strings.Replace(previous, args.Old, args.New, 1)
				}
			}
			if len(content) > 20<<20 || !utf8.ValidString(content) {
				err = errors.New("edited content exceeds UTF-8 budget")
			}
			if err == nil {
				err = atomicWrite(ctx, name, []byte(content))
				data = struct {
					Path   string          `json:"path"`
					Bytes  int             `json:"bytes"`
					Backup *wire.Reference `json:"volatile_backup,omitempty"`
				}{name, len(content), backup}
			}
		}
	case "list_directory":
		data, err = listDirectory(name)
	case "find_files":
		data, err = scanFiles(ctx, name, args.Glob, "", false)
	case "search_files":
		data, err = scanFiles(ctx, name, "", args.Query, true)
	default:
		return wire.ToolResult{}, errors.New("unknown guest file implementation")
	}
	if ctx.Err() != nil {
		return wire.ToolResult{}, ctx.Err()
	}
	if err != nil {
		return fileFailure(call, "WASI file operation failed: "+err.Error()), nil
	}
	encoded, err := abi.Encode(data, 4096)
	if err != nil {
		return fileFailure(call, "Result metadata exceeds 4KiB; narrow directory/query"), nil
	}
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(encoded)}}}, nil
}
func fileFailure(call wire.ToolCall, text string) wire.ToolResult {
	return wire.ToolResult{CallID: call.ID, IsError: true, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: text}}}
}
func readText(ctx context.Context, name string, limit int) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	f, err := os.Open(name)
	if err != nil {
		return "", err
	}
	defer f.Close()
	st, err := f.Stat()
	if err != nil {
		return "", err
	}
	if !st.Mode().IsRegular() || st.Size() > int64(limit) {
		return "", errors.New("file must be regular within byte budget")
	}
	data, err := io.ReadAll(io.LimitReader(f, int64(limit)+1))
	if err != nil {
		return "", err
	}
	if len(data) > limit || !utf8.Valid(data) {
		return "", errors.New("file exceeds UTF-8 byte budget")
	}
	return string(data), ctx.Err()
}
func atomicWrite(ctx context.Context, name string, data []byte) (err error) {
	file, err := os.CreateTemp(path.Dir(name), ".alva-wasm-*")
	if err != nil {
		return err
	}
	temp := file.Name()
	defer func() { _ = os.Remove(temp) }()
	if err = ctx.Err(); err != nil {
		file.Close()
		return err
	}
	_, err = file.Write(data)
	if err == nil {
		err = file.Sync()
	}
	err = errors.Join(err, file.Close())
	if err != nil {
		return err
	}
	if err = ctx.Err(); err != nil {
		return err
	}
	return os.Rename(temp, name)
}

type directoryEntry struct {
	Name      string `json:"name"`
	Directory bool   `json:"directory"`
	Bytes     int64  `json:"bytes"`
}

func listDirectory(name string) (any, error) {
	f, err := os.Open(name)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	entries, err := f.Readdir(257)
	if err != nil && !errors.Is(err, io.EOF) {
		return nil, err
	}
	truncated := len(entries) > 256
	if truncated {
		entries = entries[:256]
	}
	out := make([]directoryEntry, 0, len(entries))
	for _, entry := range entries {
		out = append(out, directoryEntry{entry.Name(), entry.IsDir(), entry.Size()})
	}
	sort.Slice(out, func(a, b int) bool { return out[a].Name < out[b].Name })
	return struct {
		Entries   []directoryEntry `json:"entries"`
		Truncated bool             `json:"truncated"`
	}{out, truncated}, nil
}

type fileMatch struct {
	Path    string `json:"path"`
	Offset  int    `json:"offset,omitempty"`
	Snippet string `json:"snippet,omitempty"`
}
type scanResult struct {
	Matches   []fileMatch `json:"matches"`
	Entries   int         `json:"entries_scanned"`
	Files     int         `json:"files_scanned"`
	Bytes     int         `json:"bytes_scanned"`
	Truncated bool        `json:"truncated"`
}

func scanFiles(ctx context.Context, root, glob, query string, search bool) (scanResult, error) {
	var result scanResult
	if search && (query == "" || len(query) > 1024) {
		return result, errors.New("bounded literal query required")
	}
	if !search {
		if glob == "" || len(glob) > 1024 {
			return result, errors.New("bounded glob required")
		}
		if _, err := path.Match(glob, ""); err != nil {
			return result, err
		}
	}
	err := fs.WalkDir(os.DirFS(root), ".", func(name string, entry fs.DirEntry, walkErr error) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		if walkErr != nil {
			return walkErr
		}
		result.Entries++
		if result.Entries > 4096 {
			result.Truncated = true
			return fs.SkipAll
		}
		if entry.IsDir() {
			return nil
		}
		if entry.Type()&fs.ModeSymlink != 0 {
			return nil
		}
		if !search {
			matched, _ := path.Match(glob, name)
			if matched {
				result.Matches = append(result.Matches, fileMatch{Path: name})
			}
			if len(result.Matches) >= 100 {
				result.Truncated = true
				return fs.SkipAll
			}
			return nil
		}
		if result.Files >= 256 || result.Bytes >= 1<<20 {
			result.Truncated = true
			return fs.SkipAll
		}
		result.Files++
		text, err := readText(ctx, path.Join(root, name), min(64<<10, (1<<20)-result.Bytes))
		if err != nil {
			result.Truncated = true
			return nil
		}
		result.Bytes += len(text)
		for offset := 0; offset < len(text); {
			found := strings.Index(text[offset:], query)
			if found < 0 {
				break
			}
			at := offset + found
			end := min(len(text), at+256)
			for end > at && !utf8.ValidString(text[at:end]) {
				end--
			}
			result.Matches = append(result.Matches, fileMatch{Path: name, Offset: at, Snippet: text[at:end]})
			if len(result.Matches) >= 20 {
				result.Truncated = true
				return fs.SkipAll
			}
			offset = at + len(query)
		}
		return nil
	})
	return result, err
}

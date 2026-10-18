// INPUT: os.Root, explicit security Guard, context ReferenceBackend, kernel Tool contracts, platform/system/httpjson.
// OUTPUT: Workspace, OpenWorkspace, OpenFileSystem, FileTools
// POS: [L4 extension] Native confined regular-file tools with reference-first reads, recoverable edits and bounded search
package builtin

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/system/httpjson"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const maxFileBytes = 20 << 20

type fileRoot struct {
	path   string
	access abi.Access
	root   *os.Root
}
type Workspace struct {
	mu    sync.Mutex
	cwd   string
	roots []fileRoot
}

func OpenWorkspace(directory string) (*Workspace, error) { return OpenFileSystem(directory, nil) }
func OpenFileSystem(directory string, extra []security.AuthorizedRoot) (*Workspace, error) {
	if !filepath.IsAbs(directory) || len(extra) > 16 {
		return nil, errors.New("absolute workspace and bounded explicit roots required")
	}
	w := &Workspace{cwd: filepath.Clean(directory)}
	configs := append([]security.AuthorizedRoot{{Path: w.cwd, Access: abi.AccessWrite}}, extra...)
	for _, config := range configs {
		if !filepath.IsAbs(config.Path) || (config.Access != abi.AccessRead && config.Access != abi.AccessWrite) {
			w.Close()
			return nil, errors.New("invalid filesystem grant")
		}
		p := filepath.Clean(config.Path)
		st, err := os.Lstat(p)
		if err != nil || !st.IsDir() || st.Mode()&os.ModeSymlink != 0 {
			w.Close()
			return nil, errors.New("filesystem root must be an existing real directory")
		}
		root, err := os.OpenRoot(p)
		if err != nil {
			w.Close()
			return nil, err
		}
		w.roots = append(w.roots, fileRoot{p, config.Access, root})
	}
	sort.Slice(w.roots, func(i, j int) bool { return len(w.roots[i].path) > len(w.roots[j].path) })
	return w, nil
}
func (w *Workspace) Close() error {
	var err error
	for _, r := range w.roots {
		err = errors.Join(err, r.root.Close())
	}
	return err
}
func (w *Workspace) locate(path string, access abi.Access) (fileRoot, string, error) {
	if !filepath.IsAbs(path) {
		path = filepath.Join(w.cwd, path)
	}
	path = filepath.Clean(path)
	for _, r := range w.roots {
		if access == abi.AccessWrite && r.access != abi.AccessWrite {
			continue
		}
		relative, err := filepath.Rel(r.path, path)
		if err == nil && filepath.IsLocal(relative) {
			return r, relative, nil
		}
	}
	return fileRoot{}, "", security.ErrPermissionDenied
}
func inspect(root *os.Root, relative string, write bool) (os.FileInfo, error) {
	parts := strings.Split(filepath.ToSlash(relative), "/")
	var last os.FileInfo
	for i := range parts {
		st, err := root.Lstat(filepath.FromSlash(strings.Join(parts[:i+1], "/")))
		if errors.Is(err, os.ErrNotExist) && write && i == len(parts)-1 {
			return nil, nil
		}
		if err != nil {
			return nil, err
		}
		if st.Mode()&os.ModeSymlink != 0 {
			return nil, errors.New("file tool symlinks are not allowed")
		}
		last = st
	}
	return last, nil
}
func (w *Workspace) Resolve(ctx context.Context, path string, access abi.Access) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	r, relative, err := w.locate(path, access)
	if err != nil {
		return "", err
	}
	if _, err = inspect(r.root, relative, access == abi.AccessWrite); err != nil {
		return "", err
	}
	return filepath.Join(r.path, relative), nil
}

// MakeDirectory is a native filesystem capability for explicitly authorized
// host recipes. It does not invent approval or recursively create parents.
// A model-facing caller must pass the normal SecurityPlugin before calling it.
func (w *Workspace) MakeDirectory(ctx context.Context, path string, guard *security.Guard) error {
	if guard == nil {
		return errors.New("filesystem directory mutation requires security guard")
	}
	resolved, err := guard.CheckPath(ctx, path, abi.AccessWrite)
	if err != nil {
		return err
	}
	w.mu.Lock()
	defer w.mu.Unlock()
	r, relative, err := w.locate(resolved, abi.AccessWrite)
	if err != nil {
		return err
	}
	st, err := inspect(r.root, relative, true)
	if err != nil {
		return err
	}
	if st != nil {
		if st.IsDir() {
			return nil
		}
		return errors.New("directory target already exists and is not a directory")
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	return r.root.Mkdir(relative, 0700)
}
func readFile(ctx context.Context, root *os.Root, relative string) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	before, err := inspect(root, relative, false)
	if err != nil {
		return nil, err
	}
	if before == nil || !before.Mode().IsRegular() || before.Size() > maxFileBytes {
		return nil, errors.New("file must be regular and at most 20 MiB")
	}
	f, err := root.Open(relative)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	actual, err := f.Stat()
	if err != nil || !os.SameFile(before, actual) {
		return nil, errors.New("file changed during open")
	}
	data, err := io.ReadAll(io.LimitReader(f, maxFileBytes+1))
	if err != nil {
		return nil, err
	}
	if len(data) > maxFileBytes {
		return nil, errors.New("file exceeds 20 MiB")
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return data, nil
}

type fileTool struct {
	kind      string
	workspace *Workspace
	guard     *security.Guard
	backend   agentcontext.ReferenceBackend
	sessionID string
}

func FileTools(workspace *Workspace, guard *security.Guard, backend agentcontext.ReferenceBackend, sessionID string) []abi.Tool {
	var out []abi.Tool
	for _, name := range []string{"read_file", "write_file", "edit_file", "list_directory", "search_files"} {
		out = append(out, fileTool{name, workspace, guard, backend, sessionID})
	}
	return out
}
func (t fileTool) Name() string { return t.kind }
func (t fileTool) Description() string {
	switch t.kind {
	case "read_file":
		return "Store a UTF-8 regular file (max 20 MiB) as a session reference; original text is not automatically fed to the model. Use context_read/search to inspect selected ranges."
	case "write_file":
		return "Atomically write UTF-8 content, retaining a session reference to prior text for recovery. Requires host write authorization."
	case "edit_file":
		return "Replace exact text in a UTF-8 file, retaining a session backup; ambiguous matches require all=true."
	case "list_directory":
		return "List up to 256 immediate regular-file/directory metadata entries without opening their contents."
	default:
		return "Literal bounded text search in workspace files; reports scanned bytes/files and truncation, never claims exhaustive results beyond budget."
	}
}
func (t fileTool) ParametersSchema() json.RawMessage {
	switch t.kind {
	case "read_file", "list_directory":
		return json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}`)
	case "write_file":
		return json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}`)
	case "edit_file":
		return json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"},"old":{"type":"string"},"new":{"type":"string"},"all":{"type":"boolean"}},"required":["path","old","new"],"additionalProperties":false}`)
	default:
		return json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"},"query":{"type":"string"}},"required":["path","query"],"additionalProperties":false}`)
	}
}

type fileArgs struct {
	Path    string `json:"path"`
	Content string `json:"content"`
	Old     string `json:"old"`
	New     string `json:"new"`
	All     bool   `json:"all"`
	Query   string `json:"query"`
}

func decodeFile(call wire.ToolCall, kind string) (fileArgs, error) {
	var args fileArgs
	if call.Name != kind {
		return args, errors.New("file tool identity mismatch")
	}
	if len(call.Arguments) > 2<<20 {
		return args, errors.New("file arguments exceed 2 MiB")
	}
	if err := httpjson.ValidateObject(call.Arguments, &args); err != nil {
		return args, err
	}
	decoder := json.NewDecoder(bytes.NewReader(call.Arguments))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&args); err != nil {
		return args, err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(call.Arguments, &fields); err != nil {
		return args, err
	}
	allowed := map[string]bool{"path": true}
	required := []string{"path"}
	switch kind {
	case "write_file":
		allowed["content"] = true
		required = append(required, "content")
	case "edit_file":
		allowed["old"] = true
		allowed["new"] = true
		allowed["all"] = true
		required = append(required, "old", "new")
	case "search_files":
		allowed["query"] = true
		required = append(required, "query")
	}
	for field := range fields {
		if !allowed[field] {
			return args, errors.New("unexpected file argument")
		}
		if bytes.Equal(bytes.TrimSpace(fields[field]), []byte("null")) {
			return args, errors.New("file arguments cannot be null")
		}
	}
	for _, field := range required {
		if _, exists := fields[field]; !exists {
			return args, errors.New("missing file argument")
		}
	}
	if args.Path == "" || len(args.Path) > 4096 || strings.ContainsRune(args.Path, 0) {
		return args, errors.New("invalid path")
	}
	return args, nil
}
func toolValue(call wire.ToolCall, value any) (wire.ToolResult, error) {
	data, err := json.Marshal(value)
	if err != nil {
		return wire.ToolResult{}, err
	}
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(data)}}}, nil
}
func toolProblem(call wire.ToolCall, message string) (wire.ToolResult, error) {
	return wire.ToolResult{CallID: call.ID, IsError: true, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: message}}}, nil
}
func (t fileTool) Execute(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if t.workspace == nil || t.guard == nil || t.backend == nil || t.sessionID == "" {
		return wire.ToolResult{}, errors.New("native file capability is unavailable")
	}
	args, err := decodeFile(call, t.kind)
	if err != nil {
		return toolProblem(call, "Invalid file tool arguments")
	}
	if execution.SessionID != t.sessionID || filepath.Clean(execution.Workspace) != t.workspace.cwd {
		return wire.ToolResult{}, errors.New("file tool session/workspace differs from active execution")
	}
	access := abi.AccessRead
	if !t.IsReadOnly(call) {
		access = abi.AccessWrite
	}
	absolute, err := t.guard.CheckPath(ctx, args.Path, access)
	if err != nil {
		return wire.ToolResult{}, err
	}
	root, relative, err := t.workspace.locate(absolute, access)
	if err != nil {
		return wire.ToolResult{}, err
	}
	t.workspace.mu.Lock()
	defer t.workspace.mu.Unlock()
	switch t.kind {
	case "list_directory":
		f, err := root.root.Open(relative)
		if err != nil {
			return toolProblem(call, "Directory is unavailable")
		}
		defer f.Close()
		entries, err := f.ReadDir(257)
		if err != nil && !errors.Is(err, io.EOF) {
			return wire.ToolResult{}, err
		}
		truncated := len(entries) > 256
		if truncated {
			entries = entries[:256]
		}
		type entry struct {
			Name      string `json:"name"`
			Directory bool   `json:"directory"`
			Symlink   bool   `json:"symlink"`
		}
		var out []entry
		for _, e := range entries {
			out = append(out, entry{e.Name(), e.IsDir(), e.Type()&os.ModeSymlink != 0})
		}
		return toolValue(call, map[string]any{"entries": out, "truncated": truncated, "contents_opened": false})
	case "search_files":
		return t.search(ctx, call, root, relative, args.Query)
	case "read_file":
		data, err := readFile(ctx, root.root, relative)
		if err != nil {
			return wire.ToolResult{}, err
		}
		if !utf8.Valid(data) {
			return toolProblem(call, "Binary file not delivered. Use explicit upload/media processing; this tool only stores UTF-8 text references.")
		}
		ref, err := t.backend.StoreText(ctx, string(data))
		if err != nil {
			return wire.ToolResult{}, err
		}
		return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentReference, Reference: &ref}}}, nil
	case "write_file", "edit_file":
		if len(args.Content) > 1<<20 || !utf8.ValidString(args.Content) {
			return toolProblem(call, "Write content must be UTF-8 and at most 1 MiB")
		}
		before, err := inspect(root.root, relative, true)
		if err != nil {
			return wire.ToolResult{}, err
		}
		var previous []byte
		var backup *wire.Reference
		if before != nil {
			previous, err = readFile(ctx, root.root, relative)
			if err != nil {
				return wire.ToolResult{}, err
			}
			if !utf8.Valid(previous) {
				return toolProblem(call, "Refusing to overwrite a binary file without a byte-preserving backup capability")
			}
			ref, err := t.backend.StoreText(ctx, string(previous))
			if err != nil {
				return wire.ToolResult{}, err
			}
			backup = &ref
		}
		content := args.Content
		if t.kind == "edit_file" {
			if before == nil || args.Old == "" {
				return toolProblem(call, "Edit requires an existing file and nonempty exact old text")
			}
			count := strings.Count(string(previous), args.Old)
			if count == 0 || (!args.All && count != 1) {
				return toolProblem(call, "Old text is missing or ambiguous; all=true is required for multiple matches")
			}
			n := 1
			if args.All {
				n = -1
			}
			content = strings.Replace(string(previous), args.Old, args.New, n)
			if len(content) > 1<<20 {
				return toolProblem(call, "Edited result exceeds 1 MiB")
			}
		}
		if err = atomicWrite(ctx, root.root, relative, []byte(content), before); err != nil {
			return wire.ToolResult{}, err
		}
		return toolValue(call, map[string]any{"path": args.Path, "written_bytes": len(content), "backup": backup, "atomic_replace": true})
	}
	return wire.ToolResult{}, errors.New("unknown native file tool")
}
func atomicWrite(ctx context.Context, root *os.Root, name string, data []byte, before os.FileInfo) error {
	var random [12]byte
	if _, err := rand.Read(random[:]); err != nil {
		return err
	}
	stage := filepath.Join(filepath.Dir(name), ".alva-write-"+hex.EncodeToString(random[:]))
	mode := os.FileMode(0600)
	if before != nil {
		mode = before.Mode().Perm()
	}
	f, err := root.OpenFile(stage, os.O_WRONLY|os.O_CREATE|os.O_EXCL, mode)
	if err != nil {
		return err
	}
	defer root.Remove(stage)
	_, writeErr := f.Write(data)
	syncErr := f.Sync()
	closeErr := f.Close()
	if err = errors.Join(writeErr, syncErr, closeErr); err != nil {
		return err
	}
	if err = ctx.Err(); err != nil {
		return err
	}
	current, err := root.Lstat(name)
	if before == nil {
		if !errors.Is(err, os.ErrNotExist) {
			return errors.New("file appeared before atomic replace")
		}
	} else if err != nil || !os.SameFile(before, current) || before.Size() != current.Size() || !before.ModTime().Equal(current.ModTime()) {
		return errors.New("file changed before atomic replace")
	}
	return root.Rename(stage, name)
}
func (t fileTool) search(ctx context.Context, call wire.ToolCall, root fileRoot, relative, query string) (wire.ToolResult, error) {
	if query == "" || len(query) > 1024 || !utf8.ValidString(query) {
		return toolProblem(call, "Search requires a nonempty UTF-8 literal query of at most 1024 bytes")
	}
	type match struct {
		Path   string `json:"path"`
		Offset int    `json:"offset"`
		Text   string `json:"text"`
	}
	var matches []match
	nodes, scanned, files := 0, 0, 0
	truncated := false
	stop := errors.New("search budget reached")
	err := fs.WalkDir(root.root.FS(), filepath.ToSlash(relative), func(path string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		nodes++
		if nodes > 10000 {
			truncated = true
			return stop
		}
		if entry.Type()&os.ModeSymlink != 0 {
			return nil
		}
		if entry.IsDir() {
			if entry.Name() == ".git" || entry.Name() == "node_modules" {
				truncated = true
				return fs.SkipDir
			}
			return nil
		}
		absolute := filepath.Join(root.path, filepath.FromSlash(path))
		if _, err := t.guard.CheckPath(ctx, absolute, abi.AccessRead); err != nil {
			return nil
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() || info.Size() > 1<<20 {
			truncated = true
			return nil
		}
		if scanned+int(info.Size()) > 16<<20 {
			truncated = true
			return stop
		}
		data, err := readFile(ctx, root.root, filepath.FromSlash(path))
		if err != nil {
			return err
		}
		scanned += len(data)
		files++
		if !utf8.Valid(data) {
			return nil
		}
		for offset := 0; offset < len(data); {
			index := bytes.Index(data[offset:], []byte(query))
			if index < 0 {
				break
			}
			index += offset
			end := index + 256
			if end > len(data) {
				end = len(data)
			}
			for end < len(data) && end > index && !utf8.RuneStart(data[end]) {
				end--
			}
			matches = append(matches, match{path, index, string(data[index:end])})
			if len(matches) >= 20 {
				truncated = true
				return stop
			}
			offset = index + len(query)
		}
		return nil
	})
	if err != nil && !errors.Is(err, stop) {
		return wire.ToolResult{}, err
	}
	return toolValue(call, map[string]any{"matches": matches, "truncated": truncated, "scanned_files": files, "scanned_bytes": scanned, "literal": true})
}
func (t fileTool) Scheduling(wire.ToolCall) abi.Scheduling {
	access := abi.AccessRead
	if t.kind == "write_file" || t.kind == "edit_file" {
		access = abi.AccessWrite
	}
	resource := "native-filesystem"
	if t.workspace != nil {
		resource += ":" + t.workspace.cwd
	}
	return abi.Scheduling{ConcurrentSafe: true, Resources: []abi.ResourceClaim{{Resource: resource, Access: access}}}
}
func (t fileTool) DescribeEffect(call wire.ToolCall) abi.Effect {
	access := abi.AccessRead
	kind := abi.EffectRead
	if !t.IsReadOnly(call) {
		access = abi.AccessWrite
		kind = abi.EffectWrite
	}
	args, err := decodeFile(call, t.kind)
	if err != nil {
		return abi.Effect{Kind: abi.EffectUnknown, Description: "Invalid native file arguments"}
	}
	return abi.Effect{Kind: kind, Description: t.Description(), Resources: []abi.ResourceClaim{{Resource: args.Path, Access: access}}}
}
func (t fileTool) IsReadOnly(wire.ToolCall) bool {
	return t.kind != "write_file" && t.kind != "edit_file"
}

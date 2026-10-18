// INPUT: Actual file Tool invocation, guarded workspace operations and explicit reference backend
// OUTPUT: Tool results preserving reference reads, backup-before-write and recoverable input errors
// POS: [L4 extension] Owns model-facing transformation and delivery policy without access to native root handles.
package files

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"unicode/utf8"

	workspacefs "github.com/QuincySx/Alva/platform/host/workspace"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type fileProblem string

func (problem fileProblem) Error() string { return string(problem) }

func (t fileTool) Execute(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if t.workspace == nil || t.guard == nil || t.backend == nil || t.sessionID == "" {
		return wire.ToolResult{}, errors.New("native file capability is unavailable")
	}
	args, err := decodeFile(call, t.kind)
	if err != nil {
		return toolProblem(call, "Invalid file tool arguments")
	}
	if execution.SessionID != t.sessionID || filepath.Clean(execution.Workspace) != t.workspace.Directory() {
		return wire.ToolResult{}, errors.New("file tool session/workspace differs from active execution")
	}
	access := abi.AccessRead
	if !t.IsReadOnly(call) {
		access = abi.AccessWrite
	}
	// Preserve authorization before model-facing input failures. The host also
	// validates its own guard/grant boundary when performing each operation.
	absolute, err := t.guard.CheckPath(ctx, args.Path, access)
	if err != nil {
		return wire.ToolResult{}, err
	}
	switch t.kind {
	case "list_directory":
		listing, err := t.workspace.ListDirectory(ctx, absolute, t.guard)
		if errors.Is(err, workspacefs.ErrDirectoryUnavailable) {
			return toolProblem(call, "Directory is unavailable")
		}
		if err != nil {
			return wire.ToolResult{}, err
		}
		return toolValue(call, map[string]any{"entries": listing.Entries, "truncated": listing.Truncated, "contents_opened": listing.ContentsOpened})
	case "search_files":
		if args.Query == "" || len(args.Query) > 1024 || !utf8.ValidString(args.Query) {
			return toolProblem(call, "Search requires a nonempty UTF-8 literal query of at most 1024 bytes")
		}
		result, err := t.workspace.Search(ctx, absolute, args.Query, t.guard)
		if err != nil {
			return wire.ToolResult{}, err
		}
		return toolValue(call, map[string]any{"matches": result.Matches, "truncated": result.Truncated, "scanned_files": result.ScannedFiles, "scanned_bytes": result.ScannedBytes, "literal": result.Literal})
	case "read_file":
		data, err := t.workspace.ReadFile(ctx, absolute, t.guard)
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
		var backup *wire.Reference
		written := 0
		err := t.workspace.Update(ctx, absolute, t.guard, func(previous []byte, exists bool) ([]byte, error) {
			if exists {
				if !utf8.Valid(previous) {
					return nil, fileProblem("Refusing to overwrite a binary file without a byte-preserving backup capability")
				}
				ref, err := t.backend.StoreText(ctx, string(previous))
				if err != nil {
					return nil, err
				}
				backup = &ref
			}
			content := args.Content
			if t.kind == "edit_file" {
				if !exists || args.Old == "" {
					return nil, fileProblem("Edit requires an existing file and nonempty exact old text")
				}
				count := strings.Count(string(previous), args.Old)
				if count == 0 || (!args.All && count != 1) {
					return nil, fileProblem("Old text is missing or ambiguous; all=true is required for multiple matches")
				}
				n := 1
				if args.All {
					n = -1
				}
				content = strings.Replace(string(previous), args.Old, args.New, n)
				if len(content) > 1<<20 {
					return nil, fileProblem("Edited result exceeds 1 MiB")
				}
			}
			written = len(content)
			return []byte(content), nil
		})
		var problem fileProblem
		if errors.As(err, &problem) {
			return toolProblem(call, string(problem))
		}
		if err != nil {
			return wire.ToolResult{}, err
		}
		return toolValue(call, map[string]any{"path": args.Path, "written_bytes": written, "backup": backup, "atomic_replace": true})
	}
	return wire.ToolResult{}, errors.New("unknown native file tool")
}

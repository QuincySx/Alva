// INPUT: context, crypto/sha256, encoding/hex/json, errors, path/filepath, strings, unicode/utf8, kernel/abi, llm/wire
// OUTPUT: Guard, NewGuard, AuthorizedRoot, PathResolver, SensitivePathFilter
// POS: [L3 capability library] Ordered sensitive-path, authorized-root and approval policy without pretending lexical checks are a sandbox.
package security

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type AuthorizedRoot struct {
	Path   string
	Access abi.Access
}
type PathResolver interface {
	Resolve(context.Context, string, abi.Access) (string, error)
}
type PathResolverFunc func(context.Context, string, abi.Access) (string, error)

func (fn PathResolverFunc) Resolve(ctx context.Context, path string, access abi.Access) (string, error) {
	if fn == nil {
		return "", errors.New("host path resolver unavailable")
	}
	return fn(ctx, path, access)
}

type SensitivePathFilter struct{}

func (SensitivePathFilter) IsSensitive(path string) bool {
	parts := strings.FieldsFunc(strings.ToLower(filepath.ToSlash(path)), func(r rune) bool { return r == '/' || r == '\\' })
	for _, part := range parts {
		switch part {
		case ".ssh", ".aws", ".gnupg", ".azure", ".kube", "keychains", "credentials", "id_rsa", "id_ed25519", ".npmrc", ".netrc", ".pypirc":
			return true
		}
		if part == ".env" || strings.HasPrefix(part, ".env.") {
			return true
		}
	}
	base := strings.ToLower(filepath.Base(path))
	for _, extension := range []string{".pem", ".key", ".p12", ".pfx", ".keystore"} {
		if strings.HasSuffix(base, extension) {
			return true
		}
	}
	return false
}

type Guard struct {
	workspace   string
	roots       []AuthorizedRoot
	resolver    PathResolver
	permissions *PermissionManager
	filter      SensitivePathFilter
}

func NewGuard(workspace string, roots []AuthorizedRoot, resolver PathResolver, permissions *PermissionManager) (*Guard, error) {
	if !filepath.IsAbs(workspace) {
		return nil, errors.New("security workspace must be absolute")
	}
	if resolver != nil && nilImplementation(resolver) {
		return nil, errors.New("typed-nil host path resolver")
	}
	workspace = filepath.Clean(workspace)
	owned := []AuthorizedRoot{{Path: workspace, Access: abi.AccessWrite}}
	for _, root := range roots {
		if !filepath.IsAbs(root.Path) || (root.Access != abi.AccessRead && root.Access != abi.AccessWrite) {
			return nil, errors.New("invalid authorized root")
		}
		root.Path = filepath.Clean(root.Path)
		owned = append(owned, root)
	}
	if permissions == nil {
		permissions = NewPermissionManager(Ask, nil)
	}
	return &Guard{workspace: workspace, roots: owned, resolver: resolver, permissions: permissions}, nil
}
func (guard *Guard) CheckPath(ctx context.Context, path string, access abi.Access) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	if path == "" || strings.ContainsRune(path, 0) || (access != abi.AccessRead && access != abi.AccessWrite) {
		return "", errors.New("invalid requested path/access")
	}
	if guard.filter.IsSensitive(path) {
		return "", ErrPermissionDenied
	}
	if !filepath.IsAbs(path) {
		path = filepath.Join(guard.workspace, path)
	}
	path = filepath.Clean(path)
	if guard.filter.IsSensitive(path) || !guard.contains(path, access) {
		return "", ErrPermissionDenied
	}
	if guard.resolver == nil {
		return "", errors.New("host path resolver unavailable; lexical checks are not a sandbox")
	}
	resolved, err := guard.resolver.Resolve(ctx, path, access)
	if canceled := ctx.Err(); canceled != nil {
		return "", errors.Join(err, canceled)
	}
	if err != nil {
		return "", err
	}
	if !filepath.IsAbs(resolved) {
		return "", errors.New("host path resolver returned relative path")
	}
	resolved = filepath.Clean(resolved)
	if guard.filter.IsSensitive(resolved) || !guard.contains(resolved, access) {
		return "", ErrPermissionDenied
	}
	return resolved, nil
}
func (guard *Guard) contains(path string, access abi.Access) bool {
	for _, root := range guard.roots {
		if access == abi.AccessWrite && root.Access != abi.AccessWrite {
			continue
		}
		relative, err := filepath.Rel(root.Path, path)
		if err == nil && relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator)) && !filepath.IsAbs(relative) {
			return true
		}
	}
	return false
}
func (guard *Guard) Authorize(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall, effect abi.Effect) error {
	if execution.SessionID == "" || execution.CallID != call.ID {
		return errors.New("security execution identity mismatch")
	}
	if len(call.Arguments) > 256<<10 || !utf8.Valid(call.Arguments) {
		return errors.New("approval arguments must be valid UTF-8 within 256 KiB")
	}
	if len(call.Arguments) > 0 && !wire.ValidJSON(call.Arguments) {
		return errors.New("approval arguments require non-normalizing valid JSON")
	}
	summary := string(call.Arguments)
	var fields map[string]json.RawMessage
	if json.Unmarshal(call.Arguments, &fields) == nil {
		for _, key := range []string{"command", "path", "file_path"} {
			var text string
			if raw := fields[key]; raw != nil && json.Unmarshal(raw, &text) == nil {
				summary = text
				break
			}
		}
	}
	argumentsHash := sha256.Sum256(call.Arguments)
	if effect.Kind != abi.EffectNetwork {
		for _, resource := range effect.Resources {
			if _, err := guard.CheckPath(ctx, resource.Resource, resource.Access); err != nil {
				return err
			}
		}
	}
	return guard.permissions.Authorize(ctx, ApprovalRequest{ID: execution.RunID + "/" + call.ID, SessionID: execution.SessionID, RunID: execution.RunID, CallID: call.ID, ToolName: call.Name, Effect: effect, InputSummary: summary, ArgumentsSHA256: hex.EncodeToString(argumentsHash[:])})
}

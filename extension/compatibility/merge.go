// INPUT: Same-name Skill/Agent definitions and bounded explicit resource repositories.
// OUTPUT: Canonical definition comparison, resource fingerprints and source-qualified conflicts.
// POS: [L4 extension] Coalesces identical definitions without merging permissions or hiding divergent resources.
package compatibility

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/extension/subagent"
	"github.com/QuincySx/Alva/integration/protocol/skill"
)

const resourceComparisonBytes = 64 << 20

func toolNames(names []string) []string {
	result := append([]string(nil), names...)
	sort.Strings(result)
	unique := result[:0]
	for _, name := range result {
		if len(unique) == 0 || unique[len(unique)-1] != name {
			unique = append(unique, name)
		}
	}
	return unique
}

func documentDefinition(document skill.Document) ([]byte, error) {
	if !skill.ValidName(document.Meta.Name) || len(document.Body) > skill.MaxDocumentBytes || !utf8.ValidString(document.Body) || len(document.Meta.AllowedTools) > 128 {
		return nil, errors.New("invalid bounded skill definition")
	}
	meta := document.Meta
	meta.AllowedTools = toolNames(meta.AllowedTools)
	if meta.Invocation == "" {
		meta.Invocation = skill.Auto
	}
	return json.Marshal(struct {
		Meta skill.Meta
		Body string
	}{meta, strings.TrimSpace(strings.ReplaceAll(document.Body, "\r\n", "\n"))})
}

func templateDefinition(template subagent.Template) ([]byte, error) {
	template.Tools = toolNames(template.Tools)
	template.SystemPrompt = strings.TrimSpace(strings.ReplaceAll(template.SystemPrompt, "\r\n", "\n"))
	return json.Marshal(template)
}

func conflict(kind, name, left, right string) error {
	if left == "" {
		left = "(unspecified repository source)"
	}
	if right == "" {
		right = "(unspecified repository source)"
	}
	return fmt.Errorf("conflicting %s %q between sources %q and %q; select one source or make the definitions/resources identical", kind, name, left, right)
}

// Resource content is hashed only when duplicate definitions need comparison.
// This is a bounded cold validation, not model delivery or script execution.
func resourceFingerprint(ctx context.Context, repository skill.Repository, name string, remaining *int64) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	paths, err := repository.Resources(ctx, name)
	if err = errors.Join(err, ctx.Err()); err != nil {
		return "", err
	}
	if len(paths) > 2048 {
		return "", errors.New("duplicate skill resource index exceeds 2048")
	}
	paths = append([]string(nil), paths...)
	sort.Strings(paths)
	hash := sha256.New()
	hash.Write([]byte("alva.compatibility.skill-resources.v1\x00"))
	for i, relative := range paths {
		if !fs.ValidPath(relative) || len(relative) > 4096 || strings.ContainsAny(relative, "\\\x00") || strings.Count(relative, "/") > 16 || i > 0 && paths[i-1] == relative {
			return "", errors.New("invalid duplicate skill resource index")
		}
		resource, err := repository.Resource(ctx, name, relative)
		if err = errors.Join(err, ctx.Err()); err != nil {
			return "", err
		}
		if resource.Path != relative || len(resource.Bytes) > skill.MaxDocumentBytes || len(resource.MediaType) > 256 || !utf8.ValidString(resource.MediaType) {
			return "", errors.New("invalid duplicate skill resource value")
		}
		if int64(len(resource.Bytes)) > *remaining {
			return "", errors.New("duplicate skill resource comparison exceeds 64 MiB; select one source")
		}
		*remaining -= int64(len(resource.Bytes))
		sum := sha256.Sum256(resource.Bytes)
		entry, _ := json.Marshal(struct{ Path, MediaType, SHA256 string }{relative, resource.MediaType, hex.EncodeToString(sum[:])})
		hash.Write(entry)
		hash.Write([]byte{0})
	}
	return hex.EncodeToString(hash.Sum(nil)), ctx.Err()
}

func sameSkill(ctx context.Context, name string, left skill.Document, leftRepo skill.Repository, leftSource string, right skill.Document, rightRepo skill.Repository, rightSource string, remaining *int64) error {
	if left.Meta.Name != name || right.Meta.Name != name {
		return conflict("skill identity", name, leftSource, rightSource)
	}
	a, err := documentDefinition(left)
	if err != nil {
		return err
	}
	b, err := documentDefinition(right)
	if err != nil {
		return err
	}
	if !bytes.Equal(a, b) {
		return conflict("skill definition", name, leftSource, rightSource)
	}
	aHash, err := resourceFingerprint(ctx, leftRepo, name, remaining)
	if err != nil {
		return fmt.Errorf("compare skill source %q: %w", leftSource, err)
	}
	bHash, err := resourceFingerprint(ctx, rightRepo, name, remaining)
	if err != nil {
		return fmt.Errorf("compare skill source %q: %w", rightSource, err)
	}
	if aHash != bHash {
		return conflict("skill resources", name, leftSource, rightSource)
	}
	return ctx.Err()
}

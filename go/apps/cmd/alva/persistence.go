// INPUT: context, crypto/rand, encoding/json, io, os, storage/session, agent/security, llm/wire.
// OUTPUT: Explicit native session setup and reference-only attachment persistence for the CLI.
// POS: [L6 application] CLI-owned durable assembly without importing Hub or legacy Rust configuration.
package main

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/platform/storage/session"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

const cliOwner = "alva-cli"

type attachmentFlags []string

func (a *attachmentFlags) String() string { return "explicit file paths" }
func (a *attachmentFlags) Set(value string) error {
	if value == "" || len(*a) >= 8 {
		return errors.New("one to eight explicit attachments required")
	}
	*a = append(*a, value)
	return nil
}

func openCLISession(ctx context.Context, repository *session.Repository, id string) (*session.Session, error) {
	if id != "" {
		return repository.OpenSession(ctx, cliOwner, id)
	}
	var value [16]byte
	if _, err := rand.Read(value[:]); err != nil {
		return nil, err
	}
	return repository.Create(ctx, cliOwner, hex.EncodeToString(value[:]), "")
}

func attachmentMessages(ctx context.Context, repository *session.Repository, log *session.Session, prompt string, paths []string) ([]wire.Message, error) {
	message := wire.TextMessage(wire.RoleUser, prompt)
	total := int64(0)
	for _, path := range paths {
		if (security.SensitivePathFilter{}).IsSensitive(path) {
			return nil, security.ErrPermissionDenied
		}
		resolved, resolveErr := filepath.EvalSymlinks(path)
		if resolveErr != nil {
			return nil, resolveErr
		}
		if (security.SensitivePathFilter{}).IsSensitive(resolved) {
			return nil, security.ErrPermissionDenied
		}
		info, err := os.Lstat(path)
		if err != nil {
			return nil, err
		}
		if !info.Mode().IsRegular() {
			return nil, errors.New("attachment must be a regular non-symlink file")
		}
		if info.Size() > session.MaxArtifactBytes || total+info.Size() > 40<<20 {
			return nil, errors.New("attachment limits are 20 MiB/file, 40 MiB/turn and eight files")
		}
		file, err := os.Open(path)
		if err != nil {
			return nil, err
		}
		opened, statErr := file.Stat()
		if statErr != nil || !os.SameFile(info, opened) {
			file.Close()
			return nil, errors.New("attachment changed while opening")
		}
		data, readErr := io.ReadAll(io.LimitReader(file, session.MaxArtifactBytes+1))
		closeErr := file.Close()
		if readErr != nil {
			return nil, readErr
		}
		if closeErr != nil {
			return nil, closeErr
		}
		if len(data) > session.MaxArtifactBytes || total+int64(len(data)) > 40<<20 {
			return nil, errors.New("attachment grew beyond byte limits")
		}
		total += int64(len(data))
		mediaType := http.DetectContentType(data)
		text := utf8.Valid(data) && (len(data) == 0 || strings.HasPrefix(mediaType, "text/") || mediaType == "application/json")
		stored, err := repository.PutArtifact(ctx, cliOwner, log.ID(), session.ArtifactInput{Kind: "upload", Name: filepath.Base(path), MediaType: mediaType, Processing: "Original uploaded bytes retained; no compression, image transformation, video frame extraction, transcription or media understanding performed; raw bytes not automatically delivered to model", Data: data, Text: text})
		if err != nil {
			return nil, err
		}
		reference := stored.Reference()
		message.Content = append(message.Content, wire.ContentBlock{Kind: wire.ContentReference, Reference: &reference})
	}
	return []wire.Message{message}, nil
}

func printSessions(ctx context.Context, r *session.Repository, output io.Writer) error {
	page, err := r.List(ctx, cliOwner, "", 100)
	if err != nil {
		return err
	}
	return json.NewEncoder(output).Encode(page)
}
func printHistory(ctx context.Context, r *session.Repository, id, query string, after, highWater uint64, output io.Writer) error {
	if id == "" {
		return errors.New("--history requires an existing --session")
	}
	if query != "" {
		page, err := r.Search(ctx, cliOwner, id, query, after, 20, highWater)
		if err != nil {
			return err
		}
		return json.NewEncoder(output).Encode(page)
	}
	page, err := r.Page(ctx, cliOwner, id, after, 20, highWater)
	if err != nil {
		return err
	}
	return json.NewEncoder(output).Encode(page)
}

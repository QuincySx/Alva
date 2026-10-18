// INPUT: Script capability arguments and the enclosing job's actual pinned guest grants
// OUTPUT: Bounded synchronous file/JSON/print/fetch operations without new path or network authority
// POS: [platform / WASM host] Script bindings reuse real job roots, write budget and fetch allowlist; errors expose no host paths
package host

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/sandbox/abi"
)

func (s *scriptState) resolve(raw string, write bool) (*mount, string, error) {
	if len(raw) == 0 || len(raw) > 4096 || !utf8.ValidString(raw) || strings.ContainsAny(raw, "\x00\\") {
		return nil, "", errors.New("invalid guest path")
	}
	name := raw
	if !path.IsAbs(name) {
		name = path.Join(s.job.request.Context.Workspace, name)
	} else {
		name = path.Clean(name)
	}
	for i := range s.job.mounts {
		m := &s.job.mounts[i]
		if !beneath(m.grant.Guest, name) {
			continue
		}
		if write && (m.grant.Access != ReadWrite || !s.job.request.Context.Policy.AllowFileWrite) {
			return nil, "", errors.New("job grant is read-only")
		}
		rel := strings.TrimPrefix(strings.TrimPrefix(name, m.grant.Guest), "/")
		if rel == "" {
			rel = "."
		}
		if len(strings.Split(rel, "/")) > 32 {
			return nil, "", errors.New("script path exceeds 32 components")
		}
		return m, filepath.FromSlash(rel), nil
	}
	return nil, "", errors.New("path is outside the job's existing guest grants")
}
func scriptIO(err error) error {
	if err == nil {
		return nil
	}
	return errors.New("file operation rejected or failed within the granted guest path")
}
func (s *scriptState) readFile(raw string) ([]byte, error) {
	m, rel, err := s.resolve(raw, false)
	if err != nil {
		return nil, err
	}
	f, err := nativeOpen(m.root, rel, os.O_RDONLY, 0, false)
	if err != nil {
		return nil, scriptIO(err)
	}
	defer f.Close()
	info, err := f.Stat()
	if err != nil || !info.Mode().IsRegular() || info.Size() > 1<<20 {
		return nil, errors.New("readFile requires a regular <=1MiB UTF-8 file")
	}
	data, err := io.ReadAll(io.LimitReader(f, 1<<20+1))
	if err != nil || len(data) > 1<<20 || !utf8.Valid(data) {
		return nil, errors.New("readFile exceeds 1MiB or is not valid UTF-8")
	}
	return data, nil
}
func (s *scriptState) writeFile(raw string, data []byte, appendMode bool) (int, error) {
	m, rel, err := s.resolve(raw, true)
	if err != nil {
		return 0, err
	}
	if rel == "." || len(data) > 1<<20 || !utf8.Valid(data) {
		return 0, errors.New("writeFile requires a non-root path and <=1MiB UTF-8 content")
	}
	if s.job.files.written.Add(int64(len(data))) > maxWriteBytes {
		return 0, errors.New("job file write byte budget exceeded")
	}
	if err = m.root.MkdirAll(filepath.Dir(rel), 0700); err != nil {
		return 0, scriptIO(err)
	}
	flags := os.O_WRONLY | os.O_CREATE
	if appendMode {
		flags |= os.O_APPEND
	} else {
		flags |= os.O_TRUNC
	}
	f, err := nativeOpen(m.root, rel, flags, 0600, false)
	if err != nil {
		return 0, scriptIO(err)
	}
	info, err := f.Stat()
	if err != nil || !info.Mode().IsRegular() || info.Size()+int64(len(data)) > maxFileBytes {
		f.Close()
		return 0, errors.New("script write requires a bounded regular file")
	}
	n, err := f.Write(data)
	err = errors.Join(err, f.Sync(), f.Close())
	return n, scriptIO(err)
}
func scriptMatch(pattern, name string) bool {
	parts, segments := strings.Split(pattern, "/"), strings.Split(name, "/")
	var match func(int, int) bool
	memo := map[[2]int]bool{}
	seen := map[[2]int]bool{}
	match = func(i, j int) bool {
		key := [2]int{i, j}
		if seen[key] {
			return memo[key]
		}
		seen[key] = true
		v := false
		if i == len(parts) {
			v = j == len(segments)
		} else if parts[i] == "**" {
			v = match(i+1, j) || j < len(segments) && match(i, j+1)
		} else if j < len(segments) {
			ok, err := path.Match(parts[i], segments[j])
			v = err == nil && ok && match(i+1, j+1)
		}
		memo[key] = v
		return v
	}
	return match(0, 0)
}
func (s *scriptState) glob(ctx context.Context, pattern string) ([]string, error) {
	if len(pattern) == 0 || len(pattern) > 256 || path.IsAbs(pattern) || strings.ContainsAny(pattern, "\\\x00") || len(strings.Split(pattern, "/")) > 32 {
		return nil, errors.New("glob requires a <=256-byte relative pattern without parent traversal")
	}
	for _, part := range strings.Split(pattern, "/") {
		if part == ".." {
			return nil, errors.New("glob cannot traverse parent components")
		}
		if part != "**" {
			if _, err := path.Match(part, ""); err != nil {
				return nil, errors.New("invalid glob pattern")
			}
		}
	}
	m, rel, err := s.resolve(".", false)
	if err != nil {
		return nil, err
	}
	root, err := m.root.OpenRoot(rel)
	if err != nil {
		return nil, scriptIO(err)
	}
	defer root.Close()
	out := []string{}
	nodes := 0
	err = fs.WalkDir(root.FS(), ".", func(name string, d fs.DirEntry, e error) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		if e != nil {
			return scriptIO(e)
		}
		nodes++
		if nodes > 2048 || len(strings.Split(name, "/")) > 32 {
			return errors.New("glob scan exceeds 2048 nodes/32 depth")
		}
		if !utf8.ValidString(name) || d.Type()&os.ModeSymlink != 0 {
			return errors.New("glob refuses symlinks and non-UTF-8 paths")
		}
		if name != "." && scriptMatch(pattern, name) {
			out = append(out, name)
		}
		return nil
	})
	sort.Strings(out)
	return out, err
}
func (s *scriptState) remove(ctx context.Context, raw string) error {
	m, rel, err := s.resolve(raw, true)
	if err != nil {
		return err
	}
	if rel == "." {
		return errors.New("cannot remove a grant root")
	}
	names := []string{}
	err = fs.WalkDir(m.root.FS(), filepath.ToSlash(rel), func(name string, d fs.DirEntry, e error) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		if e != nil {
			return scriptIO(e)
		}
		if len(names) >= 2048 || len(strings.Split(name, "/")) > 32 || d.Type()&os.ModeSymlink != 0 {
			return errors.New("remove exceeds bounded tree or encounters a symlink")
		}
		names = append(names, filepath.FromSlash(name))
		return nil
	})
	if err != nil {
		return err
	}
	for i := len(names) - 1; i >= 0; i-- {
		if err = ctx.Err(); err != nil {
			return err
		}
		if err = m.root.Remove(names[i]); err != nil {
			return scriptIO(err)
		}
	}
	return nil
}

func (s *scriptState) binding(ctx context.Context, op uint32, data []byte) (any, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	var args []json.RawMessage
	if json.Unmarshal(data, &args) != nil || len(args) > 16 {
		return nil, errors.New("binding requires bounded positional arguments")
	}
	text := func(i int) (string, error) {
		var value string
		if i >= len(args) || json.Unmarshal(args[i], &value) != nil {
			return "", errors.New("binding requires string arguments")
		}
		return value, nil
	}
	first, err := text(0)
	if err != nil {
		return nil, err
	}
	switch op {
	case 0:
		bytes, e := s.readFile(first)
		return string(bytes), e
	case 1, 2:
		second, e := text(1)
		if e != nil || len(args) != 2 {
			return nil, errors.New("writeFile/appendFile require path and text")
		}
		return s.writeFile(first, []byte(second), op == 2)
	case 3:
		m, rel, e := s.resolve(first, false)
		if e != nil {
			return nil, e
		}
		info, e := m.root.Stat(rel)
		if errors.Is(e, os.ErrNotExist) {
			return false, nil
		}
		if e != nil {
			return nil, scriptIO(e)
		}
		return info.IsDir() || info.Mode().IsRegular(), nil
	case 4:
		return nil, s.remove(ctx, first)
	case 5:
		second, e := text(1)
		if e != nil || len(args) != 2 {
			return nil, errors.New("rename requires source and destination")
		}
		a, from, e := s.resolve(first, true)
		if e != nil {
			return nil, e
		}
		b, to, e := s.resolve(second, true)
		if e != nil {
			return nil, e
		}
		if a != b || from == "." || to == "." {
			return nil, errors.New("rename requires two non-root paths in the same grant")
		}
		if e = a.root.MkdirAll(filepath.Dir(to), 0700); e != nil {
			return nil, scriptIO(e)
		}
		return nil, scriptIO(a.root.Rename(from, to))
	case 6:
		m, rel, e := s.resolve(first, true)
		if e != nil {
			return nil, e
		}
		return nil, scriptIO(m.root.MkdirAll(rel, 0700))
	case 7:
		m, rel, e := s.resolve(first, false)
		if e != nil {
			return nil, e
		}
		f, e := nativeOpen(m.root, rel, os.O_RDONLY, 0, false)
		if e != nil {
			return nil, scriptIO(e)
		}
		entries, e := f.ReadDir(2049)
		if errors.Is(e, io.EOF) {
			e = nil
		}
		e = errors.Join(e, f.Close())
		if e != nil || len(entries) > 2048 {
			return nil, errors.New("readDir failed or exceeds 2048 entries")
		}
		type entry struct {
			Name  string `json:"name"`
			IsDir bool   `json:"isDir"`
			Size  int64  `json:"size"`
		}
		out := []entry{}
		for _, d := range entries {
			info, e := d.Info()
			if e != nil || !utf8.ValidString(d.Name()) || !info.IsDir() && !info.Mode().IsRegular() {
				return nil, errors.New("readDir refuses unsupported entries")
			}
			out = append(out, entry{d.Name(), d.IsDir(), info.Size()})
		}
		sort.Slice(out, func(i, j int) bool { return out[i].Name < out[j].Name })
		return out, nil
	case 8:
		m, rel, e := s.resolve(first, false)
		if e != nil {
			return nil, e
		}
		info, e := m.root.Stat(rel)
		if e != nil {
			return nil, scriptIO(e)
		}
		if !info.IsDir() && !info.Mode().IsRegular() {
			return nil, errors.New("stat requires a regular file or directory")
		}
		return map[string]any{"isFile": info.Mode().IsRegular(), "isDir": info.IsDir(), "size": info.Size(), "readonly": m.grant.Access != ReadWrite || !s.job.request.Context.Policy.AllowFileWrite || info.Mode().Perm()&0222 == 0}, nil
	case 9:
		return s.glob(ctx, first)
	case 10:
		second, e := text(1)
		if e != nil || len(args) != 2 {
			return nil, errors.New("copyFile requires source and destination")
		}
		bytes, e := s.readFile(first)
		if e != nil {
			return nil, e
		}
		return s.writeFile(second, bytes, false)
	case 11:
		if len(args) != 1 {
			return nil, errors.New("print requires its normalized text line")
		}
		if len(s.output) >= 256 || len(first) > 64<<10-s.outputBytes {
			s.outputFailure = true
			return nil, errors.New("script output exceeds 256 lines/64KiB")
		}
		s.output = append(s.output, first)
		s.outputBytes += len(first)
		return nil, nil
	case 12:
		if len(args) != 4 {
			return nil, errors.New("fetch requires method, URL, headers and UTF-8 body")
		}
		url, e := text(1)
		if e != nil {
			return nil, e
		}
		body, e := text(3)
		if e != nil {
			return nil, e
		}
		var headers []abi.Header
		if json.Unmarshal(args[2], &headers) != nil {
			return nil, errors.New("invalid fetch header pairs")
		}
		request := abi.FetchRequest{Version: abi.Version, Method: first, URL: url, Headers: headers, Body: []byte(body)}
		if e = request.Validate(); e != nil {
			return nil, errors.New("fetch request exceeds host protocol limits")
		}
		s.job.outcome.FetchCalls++
		if s.job.outcome.FetchCalls > s.job.request.Limits.MaxFetchCalls {
			return nil, errors.New("job fetch call limit exceeded")
		}
		response, e := executeFetch(ctx, s.job.request.AllowedDomains, request)
		if e != nil {
			return nil, e
		}
		if !utf8.Valid(response.Body) {
			return nil, errors.New("fetch body is not UTF-8 text")
		}
		return map[string]any{"status": response.Status, "headers": response.Headers, "body": string(response.Body)}, nil
	}
	return nil, errors.New("unknown script capability")
}

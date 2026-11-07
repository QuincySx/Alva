// INPUT: Explicit captured file identities and bounded local paths
// OUTPUT: Streaming hash, size and file-identity revalidation shared by native transports
// POS: [integration / native external-engine adapter] Content verification grants no additional filesystem authority
package process

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"io"
	"os"

	"github.com/QuincySx/Alva/integration/engine"
)

// Check the admitted capture immediately before exposing paths to the native
// process. This is content verification, not an additional filesystem grant.
func verifyCapturedInputFiles(ctx context.Context, files []engine.InputFile) error {
	if len(files) == 0 {
		return ctx.Err()
	}
	buffer := make([]byte, 256<<10)
	for _, selected := range files {
		if err := verifyCapturedFile(ctx, selected.Path, selected.Bytes, selected.SHA256, buffer); err != nil {
			return err
		}
		if selected.TextPath == "" || selected.TextPath == selected.Path && selected.TextBytes == selected.Bytes && selected.TextSHA256 == selected.SHA256 {
			continue
		}
		if err := verifyCapturedFile(ctx, selected.TextPath, selected.TextBytes, selected.TextSHA256, buffer); err != nil {
			return err
		}
	}
	return ctx.Err()
}

type capturedFileReader struct {
	context context.Context
	reader  io.Reader
}

func (r capturedFileReader) Read(data []byte) (int, error) {
	if err := r.context.Err(); err != nil {
		return 0, err
	}
	return r.reader.Read(data)
}

func verifyCapturedFile(ctx context.Context, path string, size int64, expected string, buffer []byte) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || info.Size() != size {
		return errors.New("selected file capture is unavailable or changed since selection")
	}
	file, err := os.Open(path)
	if err != nil {
		return errors.New("selected file capture could not be opened")
	}
	defer file.Close()
	opened, err := file.Stat()
	if err != nil || !os.SameFile(info, opened) || opened.Size() != size {
		return errors.New("selected file capture changed while opening")
	}
	digest := sha256.New()
	read, err := io.CopyBuffer(digest, io.LimitReader(capturedFileReader{ctx, file}, size+1), buffer)
	if err != nil {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return errors.New("selected file capture could not be completely verified")
	}
	after, err := os.Lstat(path)
	if err != nil || !after.Mode().IsRegular() || !os.SameFile(opened, after) || read != size || after.Size() != size || hex.EncodeToString(digest.Sum(nil)) != expected {
		return errors.New("selected file capture changed since selection")
	}
	if err := file.Close(); err != nil {
		return errors.New("selected file capture could not be closed")
	}
	return ctx.Err()
}

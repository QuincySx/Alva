// INPUT: Actual CLI session storage, versioned extension records and bounded JSON values.
// OUTPUT: Private CLI snapshot load/CAS helpers and checked terminal-safe JSON output.
// POS: [L6 application] CLI settings retain the supplied facts authority without importing Hub or choosing storage.
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"unicode"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/redaction"
	"github.com/QuincySx/Alva/platform/system/strictjson"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

const cliNamespace = "alva.cli"

func loadCLIValue(ctx context.Context, repository storage.Repository, log storage.Session, kind string, out any) (uint64, error) {
	entry, found, err := repository.LatestRecord(ctx, cliOwner, log.ID(), cliNamespace, kind)
	if err != nil || !found {
		return 0, err
	}
	_, data, err := repository.ReadArtifactFull(ctx, cliOwner, log.ID(), entry.Value.URI, 512<<10)
	if err != nil {
		return 0, err
	}
	var record abi.SessionRecord
	if err = json.Unmarshal(data, &record); err != nil {
		return 0, err
	}
	if err = record.Validate(); err != nil {
		return 0, err
	}
	if record.ID != entry.ID || record.Sequence != entry.Sequence || record.Kind != abi.RecordExtension || record.Namespace != cliNamespace || record.Type != kind || record.SchemaVersion != 1 || entry.SchemaVersion != 1 {
		return 0, errors.New("CLI snapshot metadata, identity or schema changed")
	}
	if err = decodeCLIJSON(record.Payload, out); err != nil {
		return 0, err
	}
	return entry.Sequence, nil
}

func commitCLIValue(ctx context.Context, log storage.Session, kind string, revision uint64, value any) error {
	data, err := json.Marshal(value)
	if err != nil {
		return err
	}
	accepted, err := redaction.RedactJSON(data)
	if err != nil {
		return err
	}
	var original, redacted any
	if json.Unmarshal(data, &original) != nil || json.Unmarshal(accepted, &redacted) != nil {
		return errors.New("invalid CLI settings JSON")
	}
	left, _ := json.Marshal(original)
	right, _ := json.Marshal(redacted)
	if !bytes.Equal(left, right) {
		return errors.New("CLI settings contain data changed by journal redaction; credentials must remain in the explicit source file")
	}
	_, err = log.AppendExtensionSnapshot(ctx, cliNamespace, kind, revision, accepted)
	return err
}

func decodeCLIJSON(data []byte, out any) error {
	if len(data) == 0 || len(data) > 512<<10 {
		return errors.New("CLI JSON outside byte limits")
	}
	if err := strictjson.ValidateObject(data, out); err != nil {
		return err
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(out); err != nil {
		return err
	}
	if decoder.Decode(new(any)) != io.EOF {
		return errors.New("expected one CLI JSON value")
	}
	return nil
}

func readCLIJSONFile(ctx context.Context, path string, limit int64) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	before, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !before.Mode().IsRegular() || before.Size() > limit {
		return nil, errors.New("explicit JSON file must be a bounded regular non-symlink file")
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	actual, err := file.Stat()
	if err != nil || !os.SameFile(before, actual) {
		return nil, errors.New("explicit JSON file changed while opening")
	}
	data, err := io.ReadAll(io.LimitReader(file, limit+1))
	if err != nil {
		return nil, err
	}
	if len(data) > int(limit) {
		return nil, errors.New("explicit JSON file exceeded byte limit")
	}
	return data, ctx.Err()
}

func writeCLIJSON(output io.Writer, value any) error {
	data, err := json.Marshal(value)
	if err != nil {
		return err
	}
	if len(data) > 512<<10 {
		return errors.New("CLI JSON output exceeds 512 KiB")
	}
	var encoded bytes.Buffer
	for _, r := range string(data) {
		if unicode.IsControl(r) {
			fmt.Fprintf(&encoded, "\\u%04x", r)
		} else {
			encoded.WriteRune(r)
		}
	}
	encoded.WriteByte('\n')
	if encoded.Len() > 512<<10 {
		return errors.New("escaped CLI JSON output exceeds 512 KiB")
	}
	n, err := output.Write(encoded.Bytes())
	if err == nil && n != encoded.Len() {
		err = io.ErrShortWrite
	}
	return err
}

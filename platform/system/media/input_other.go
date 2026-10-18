// INPUT: Private temporary root, bounded selected bytes and owned process file grants
// OUTPUT: Exact unlinked read-only input snapshots and Unix decoder fd3 binding
// POS: [platform / OS primitive] Decoders receive one stored input handle, never its original source path.
//go:build !windows

package media

import (
	"bytes"
	"errors"
	"io"
	"os"

	"github.com/QuincySx/Alva/platform/system/process"
)

const decoderInputURL = "/dev/fd/3"
const decoderProtocols = "file,pipe"

func bindDecoderInput(config *process.Config, file *os.File) { config.ReadFiles = []*os.File{file} }
func inputSnapshot(directory, prefix string, data []byte) (*os.File, error) {
	if len(data) > MaxInput {
		return nil, errors.New("media input exceeds20MiB")
	}
	file, err := os.CreateTemp(directory, prefix)
	if err != nil {
		return nil, err
	}
	name := file.Name()
	_, err = io.Copy(file, bytes.NewReader(data))
	err = errors.Join(err, file.Sync(), file.Close())
	if err != nil {
		return nil, errors.Join(err, os.Remove(name))
	}
	read, err := os.Open(name)
	removeErr := os.Remove(name)
	if err != nil {
		return nil, errors.Join(err, removeErr)
	}
	if removeErr != nil {
		read.Close()
		return nil, removeErr
	}
	return read, nil
}

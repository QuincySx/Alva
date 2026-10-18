// INPUT: Explicit byte stream reader, maximum line bytes and a synchronous frame consumer
// OUTPUT: ReadLines, StreamLine and ErrLineLimit preserving delimiters/partial EOF without silent loss
// POS: [platform / OS primitive] Bounded native I/O framing only; protocol/storage/secret policy belongs to consumers
package process

import (
	"bufio"
	"errors"
	"io"
)

var ErrLineLimit = errors.New("native stream line exceeds explicit byte limit")

type StreamLine struct {
	Data                []byte
	Complete, Truncated bool
}

// ReadLines accumulates across arbitrary Read/UTF-8 boundaries. Data includes
// the original newline. At EOF a partial line is emitted once; over budget a
// bounded prefix is emitted as Truncated and the error requires owner shutdown.
// The callback must consume synchronously; no goroutine or persistence is hidden.
func ReadLines(source io.Reader, limit int, consume func(StreamLine) error) error {
	if source == nil || consume == nil || limit < 1 || limit > 16<<20 {
		return errors.New("bounded stream reader and consumer required")
	}
	reader := bufio.NewReaderSize(source, min(4096, limit))
	var line []byte
	for {
		part, err := reader.ReadSlice('\n')
		if len(part) > limit-len(line) {
			line = append(line, part[:limit-len(line)]...)
			return errors.Join(ErrLineLimit, consume(StreamLine{Data: line, Truncated: true}))
		}
		line = append(line, part...)
		if errors.Is(err, bufio.ErrBufferFull) {
			continue
		}
		if len(line) > 0 {
			if emitErr := consume(StreamLine{Data: line, Complete: err == nil}); emitErr != nil {
				return emitErr
			}
			line = nil
		}
		if errors.Is(err, io.EOF) {
			return nil
		}
		if err != nil {
			return err
		}
	}
}

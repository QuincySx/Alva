// INPUT: Exclusive borrowed pipe, cancellation context and OS-supported file deadlines
// OUTPUT: Bounded fallback pipe read with a joined cancellation callback or explicit unsupported error
// POS: [platform / OS primitive] Fail closed when a native platform cannot interrupt its inherited pipe.
//go:build !darwin && !linux && !windows

package secretinput

import (
	"context"
	"errors"
	"io"
	"os"
	"time"
)

func readNativePipe(ctx context.Context, file *os.File, maximum int) ([]byte, error) {
	if file.SetReadDeadline(time.Time{}) != nil {
		return nil, errors.New("secret input pipe cancellation is unsupported on this platform")
	}
	settled := make(chan struct{})
	stop := context.AfterFunc(ctx, func() { _ = file.SetReadDeadline(time.Now()); close(settled) })
	defer func() {
		if !stop() {
			<-settled
		}
		_ = file.SetReadDeadline(time.Time{})
	}()
	data, err := io.ReadAll(io.LimitReader(file, int64(maximum)+1))
	if ctx.Err() != nil {
		return nil, ctx.Err()
	}
	if err != nil {
		return nil, errors.New("secret input pipe read failed")
	}
	if len(data) > maximum {
		return nil, errors.New("secret input exceeds byte ceiling")
	}
	return data, nil
}

// INPUT: Explicit Processor, fully verified silent bounded MP4 derivative and one-frame consumer
// OUTPUT: Single-use Playback with bounded decoded RGBA streaming, cancellation and actual cleanup evidence
// POS: [platform / OS primitive] Native self-drawn playback only; no WebView, original-media input, audio, model delivery or durable audit claim
package media

import (
	"bytes"
	"context"
	"errors"
	"image"
	"io"
	"os"
	"strconv"
	"sync"
	"time"

	"github.com/QuincySx/Alva/platform/system/process"
)

type PlaybackInput struct {
	Data             []byte
	SHA256           string
	Width, Height    int
	DurationMillis   int64
	Codec, FrameRate string
}
type PlaybackFrame struct {
	Image *image.RGBA
	// PositionMillis is the nominal 5fps playback clock, not source-frame
	// provenance/PTS, a new derivative manifest or proof of semantic analysis.
	PositionMillis, DurationMillis int64
}
type Playback struct {
	processor       *Processor
	input           PlaybackInput
	mu              sync.Mutex
	started, closed bool
	cancel          context.CancelFunc
	child           *process.Process
	done            chan struct{}
}

// NewPlayback copies checked bytes but starts no process. The caller must have
// already authorized the exact artifact and decoder snapshot; raw host paths
// and uploaded originals are not accepted by the product playback surface.
func (p *Processor) NewPlayback(input PlaybackInput) (*Playback, error) {
	if p == nil || len(input.Data) == 0 || len(input.Data) > 8<<20 || hash(input.Data) != input.SHA256 || input.Width < 1 || input.Height < 1 || input.Width > 720 || input.Height > 720 || input.DurationMillis < 1 || input.DurationMillis > 60000 || input.Codec != "h264" || input.FrameRate != "5/1" {
		return nil, errors.New("playback requires a verified <=8MiB/60s/720px/5fps silent H.264 derivative")
	}
	p.mu.Lock()
	closed := p.closed
	p.mu.Unlock()
	if closed {
		return nil, errors.New("playback processor is closed; explicitly construct a fresh approved owner")
	}
	input.Data = bytes.Clone(input.Data)
	return &Playback{processor: p, input: input, done: make(chan struct{})}, nil
}

// Run is single-use and synchronous. A consumer must finish with a frame
// before returning; no frame queue, full-video buffer or detached decode loop
// is created. Pause/seek creates a NEW playback only after Close proves cleanup.
func (p *Playback) Run(ctx context.Context, seekMillis int64, emit func(context.Context, PlaybackFrame) error) (result error) {
	p.mu.Lock()
	if p.started || p.closed || emit == nil || seekMillis < 0 || seekMillis >= p.input.DurationMillis {
		p.mu.Unlock()
		return errors.New("unused playback, valid seek position and synchronous frame consumer required")
	}
	p.started = true
	release, err := p.processor.enter()
	if err != nil {
		p.mu.Unlock()
		close(p.done)
		return err
	}
	// The Processor owns decoder work, including the synchronous consumer.
	// Closing that owner must cancel a cooperative emit even when its caller
	// has not cancelled and Playback.Close was never invoked.
	owned, cancel := context.WithTimeout(p.processor.ownerContext(), 100*time.Second)
	stopCaller := context.AfterFunc(ctx, cancel)
	if ctx.Err() != nil {
		cancel()
	}
	p.cancel = cancel
	p.mu.Unlock()
	defer cancel()
	defer stopCaller()
	defer close(p.done)
	defer func() {
		cleanup, finish := context.WithTimeout(context.Background(), 5*time.Second)
		defer finish()
		result = errors.Join(result, p.processor.Close(cleanup))
	}()
	defer release()
	end, err := p.processor.begin(owned)
	if err != nil {
		return err
	}
	defer end()
	actual, err := p.processor.probe(owned, p.input.Data, "video/mp4")
	if err != nil {
		return err
	}
	if actual.Width != p.input.Width || actual.Height != p.input.Height || actual.DurationMillis != p.input.DurationMillis || actual.Codec != "h264" || actual.FrameRate != "5/1" || actual.AudioStreams != 0 {
		return errors.New("playback derivative disagrees with actual decoder metadata or contains audio")
	}
	read, err := playbackDescriptor(p.processor.config.TempDirectory, p.input.Data)
	if err != nil {
		return err
	}
	defer read.Close()
	args, _ := decoderInput("video/mp4")
	args = append([]string{"-nostdin", "-hide_banner", "-v", "error", "-threads", "1", "-filter_threads", "1"}, args...)
	args = append(args, "-ss", strconv.FormatFloat(float64(seekMillis)/1000, 'f', 3, 64), "-map", "0:v:0", "-an", "-sn", "-dn", "-vf", "fps=5,format=rgba", "-threads", "1", "-pix_fmt", "rgba", "-f", "rawvideo", "pipe:1")
	if err = owned.Err(); err != nil {
		return err
	}
	config := process.Config{Command: p.processor.config.FFmpeg, Args: args, Cwd: p.processor.config.TempDirectory}
	bindDecoderInput(&config, read)
	child, err := p.processor.startProcess(owned, config)
	if err != nil {
		return err
	}
	p.mu.Lock()
	p.child = child
	p.mu.Unlock()
	_ = child.Stdin.Close()
	stop := context.AfterFunc(owned, child.BeginClose)
	defer stop()
	stderr := make(chan error, 1)
	go func() {
		// Bounded decoder diagnostics are consumed/discarded, NOT persisted or
		// returned raw (input metadata may contain secrets). Product/UI must not
		// advertise this local preview as a durable processing receipt.
		body, e := io.ReadAll(io.LimitReader(child.Stderr, 128<<10+1))
		if len(body) > 128<<10 {
			e = errors.Join(e, errors.New("playback diagnostic budget exceeded"))
			child.BeginClose()
		}
		stderr <- e
	}()
	defer func() {
		cleanup, finish := context.WithTimeout(context.Background(), 5*time.Second)
		defer finish()
		result = errors.Join(result, child.Close(cleanup))
		p.processor.releaseChild(child)
		readErr := <-stderr
		if readErr != nil && !errors.Is(readErr, os.ErrClosed) {
			result = errors.Join(result, errors.New("playback diagnostic capture failed"))
		}
		if !child.CleanupComplete() {
			result = errors.Join(result, errors.New("playback physical cleanup incomplete; retain owner for explicit Close retry"))
		}
	}()
	start := time.Now()
	frames := 0
	for {
		pixels := make([]byte, p.input.Width*p.input.Height*4)
		n, e := io.ReadFull(child.Stdout, pixels)
		if e == io.EOF && n == 0 {
			if owned.Err() != nil {
				return owned.Err()
			}
			break
		}
		if e != nil {
			if owned.Err() != nil {
				return owned.Err()
			}
			return errors.New("playback ended with an incomplete RGBA frame")
		}
		if frames >= 301 {
			return errors.New("playback exceeds bounded 5fps frame count")
		}
		if wait := time.Until(start.Add(time.Duration(frames) * 200 * time.Millisecond)); wait > 0 {
			timer := time.NewTimer(wait)
			select {
			case <-timer.C:
			case <-owned.Done():
				timer.Stop()
				return owned.Err()
			}
		}
		if err = owned.Err(); err != nil {
			return err
		}
		frame := PlaybackFrame{Image: &image.RGBA{Pix: pixels, Stride: p.input.Width * 4, Rect: image.Rect(0, 0, p.input.Width, p.input.Height)}, PositionMillis: min(p.input.DurationMillis, seekMillis+int64(frames)*200), DurationMillis: p.input.DurationMillis}
		if err = emit(owned, frame); err != nil {
			return err
		}
		frames++
	}
	if frames == 0 {
		return errors.New("decoder completed without a playback frame")
	}
	if err = child.Wait(owned); err != nil {
		return err
	}
	code, exited := child.ExitCode()
	if !exited || code != 0 || !child.CleanupComplete() {
		return errors.New("playback decoder did not exit and reap successfully")
	}
	return nil
}

func playbackDescriptor(directory string, data []byte) (*os.File, error) {
	return inputSnapshot(directory, "alva-playback-", data)
}

func (p *Playback) Close(ctx context.Context) error {
	p.mu.Lock()
	p.closed = true
	started, cancel, child := p.started, p.cancel, p.child
	p.mu.Unlock()
	if cancel != nil {
		cancel()
	}
	var err error
	// probe also launches owned decoder processes; its registry may retain a
	// child after a cleanup failure even before Playback.child was assigned.
	err = errors.Join(err, p.processor.Close(ctx))
	if child != nil {
		err = errors.Join(err, child.Close(ctx))
	}
	if started {
		select {
		case <-p.done:
		case <-ctx.Done():
			return errors.Join(err, ctx.Err())
		}
	}
	p.mu.Lock()
	child = p.child
	p.mu.Unlock()
	if child != nil {
		err = errors.Join(err, child.Close(ctx))
	}
	return err
}
func (p *Playback) CleanupComplete() bool {
	p.mu.Lock()
	started, closed, child := p.started, p.closed, p.child
	p.mu.Unlock()
	if !started {
		return closed && p.processor.CleanupComplete()
	}
	select {
	case <-p.done:
	default:
		return false
	}
	return (child == nil || child.CleanupComplete()) && p.processor.CleanupComplete()
}

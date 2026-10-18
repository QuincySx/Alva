// INPUT: Explicit Play of verified canonical PCM derivative and caller-owned cancellation
// OUTPUT: One bounded MyGo audio voice, seek, output-error reporting and synchronous voice close
// POS: [L7 Hub product] Device playback is UI-only; no decoder discovery, autoplay, transcript or model receipt
//go:build desktop

package hubdesktop

import (
	"context"
	"encoding/binary"
	"errors"
	"sync"
	"time"

	nativemedia "github.com/QuincySx/Alva/platform/system/media"
	"github.com/egoist/mygo/audio"
)

type localPlayback interface {
	Close(context.Context) error
	CleanupComplete() bool
}

type audioVoice interface {
	Play()
	Playing() bool
	Seek(time.Duration)
	Position() time.Duration
	Close()
}

type audioPlayback struct {
	mu              sync.Mutex
	voice           audioVoice
	outputError     func() error
	duration        int64
	started, closed bool
	cancel          context.CancelFunc
	done            chan struct{}
}

// Construction stays cold, including tests. The bounded in-memory voice has
// no streaming decoder goroutine. MyGo opens the shared OS output only in Play.
func newAudioPlayback(data []byte, duration int64, makeVoice func([]float32) audioVoice, outputError func() error) (*audioPlayback, error) {
	actual, err := nativemedia.ValidateAudio(data)
	if err != nil || actual != duration {
		return nil, errors.New("audio playback requires verified sample-derived duration")
	}
	samples := make([]float32, (len(data)-44)/2)
	for i := range samples {
		samples[i] = float32(int16(binary.LittleEndian.Uint16(data[44+2*i:]))) / 32768
	}
	if makeVoice == nil {
		makeVoice = func(samples []float32) audioVoice {
			return audio.NewSound(nativemedia.AudioSampleRate, 1, samples).NewPlayer()
		}
	}
	if outputError == nil {
		outputError = audio.Err
	}
	return &audioPlayback{voice: makeVoice(samples), outputError: outputError, duration: actual, done: make(chan struct{})}, nil
}

func (p *audioPlayback) Run(ctx context.Context, seek int64, emit func(context.Context, int64, int64) error) error {
	p.mu.Lock()
	if p.started || p.closed || emit == nil || seek < 0 || seek >= p.duration {
		p.mu.Unlock()
		return errors.New("unused audio playback, valid seek and position observer required")
	}
	p.started = true
	owned, cancel := context.WithTimeout(ctx, 65*time.Second)
	p.cancel = cancel
	if owned.Err() == nil {
		p.voice.Seek(time.Duration(seek) * time.Millisecond)
		p.voice.Play()
	}
	p.mu.Unlock()
	defer cancel()
	defer close(p.done)
	defer func() { p.mu.Lock(); p.closed = true; p.voice.Close(); p.mu.Unlock() }()
	tick := time.NewTicker(50 * time.Millisecond)
	defer tick.Stop()
	for {
		if err := owned.Err(); err != nil {
			return err
		}
		// MyGo keeps a failed device's clock advancing silently; report that
		// failure instead of claiming audible success. Even nil is not proof
		// that the user heard anything or an acoustic/device playhead receipt.
		if p.outputError() != nil {
			return errors.New("local audio output unavailable; no audible playback confirmed")
		}
		position := min(p.duration, max(0, p.voice.Position().Milliseconds()))
		if err := emit(owned, position, p.duration); err != nil {
			return err
		}
		if !p.voice.Playing() {
			return nil
		}
		select {
		case <-owned.Done():
			return owned.Err()
		case <-tick.C:
		}
	}
}

// Close removes only this voice, synchronously with the shared mixer's lock.
// Already queued device samples may take a short hardware buffer to drain.
// The shared device is MyGo-owned and may remain open for its idle timeout;
// it is not a process/decoder this window may terminate for other windows.
func (p *audioPlayback) Close(ctx context.Context) error {
	p.mu.Lock()
	p.closed = true
	if p.cancel != nil {
		p.cancel()
	}
	p.voice.Close()
	started := p.started
	p.mu.Unlock()
	if started {
		select {
		case <-p.done:
		case <-ctx.Done():
			return ctx.Err()
		}
	}
	return nil
}
func (p *audioPlayback) CleanupComplete() bool {
	p.mu.Lock()
	defer p.mu.Unlock()
	if !p.closed {
		return false
	}
	if !p.started {
		return true
	}
	select {
	case <-p.done:
		return true
	default:
		return false
	}
}

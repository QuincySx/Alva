// INPUT: Controlled native recognition drivers and explicit lifecycle requests.
// OUTPUT: Finalization, late-result, physical cleanup and bounded-text regressions without device access.
// POS: [platform / OS primitive tests] Does not request microphone permission or claim acoustic recognition.
package speech

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"
)

func TestStopWaitsForActualRecognitionCleanup(t *testing.T) {
	finalizing, release := make(chan struct{}), make(chan struct{})
	c := newCapture(t.Context(), Options{}, func(ctx context.Context, _ Options, stop <-chan struct{}, emit func(State, string) error) error {
		if err := emit(Recording, "部分"); err != nil {
			return err
		}
		<-stop
		if err := emit(Transcribing, "最终文字"); err != nil {
			return err
		}
		close(finalizing)
		<-release
		return nil
	})
	c.Stop()
	c.Stop()
	<-finalizing
	select {
	case <-c.Done():
		t.Fatal("recording Stop falsely completed before native cleanup")
	default:
	}
	if v := c.Snapshot(); v.State != Transcribing || v.Text != "最终文字" || v.StartedAt.IsZero() {
		t.Fatal(v)
	}
	close(release)
	<-c.Done()
	if v := c.Snapshot(); v.State != Completed || v.Text != "最终文字" {
		t.Fatal(v)
	}
}

func TestCancelSuppressesLateTextAndRetainsCleanupOwner(t *testing.T) {
	entered, cancelled, release := make(chan struct{}), make(chan struct{}), make(chan struct{})
	c := newCapture(t.Context(), Options{}, func(ctx context.Context, _ Options, _ <-chan struct{}, emit func(State, string) error) error {
		_ = emit(Recording, "原始片段")
		close(entered)
		<-ctx.Done()
		close(cancelled)
		<-release
		if err := emit(Recording, "迟到文字"); !errors.Is(err, context.Canceled) {
			t.Error("late callback accepted after cancel", err)
		}
		return nil
	})
	<-entered
	deadline, cancel := context.WithTimeout(t.Context(), time.Millisecond)
	defer cancel()
	if err := c.Close(deadline); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("unfinished native owner was released", err)
	}
	<-cancelled
	select {
	case <-c.Done():
		t.Fatal("cancel did not wait for actual native return")
	default:
	}
	close(release)
	if err := c.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	if v := c.Snapshot(); v.State != Cancelled || v.Text != "" {
		t.Fatal("cancel exposed recorded text", v)
	}
}

func TestSpeechRejectsUnboundedOrInvalidText(t *testing.T) {
	for _, text := range []string{strings.Repeat("x", 32<<10+1), string([]byte{0xff})} {
		c := newCapture(t.Context(), Options{}, func(_ context.Context, _ Options, _ <-chan struct{}, emit func(State, string) error) error {
			return emit(Recording, text)
		})
		<-c.Done()
		if v := c.Snapshot(); v.State != Failed || v.Text != "" {
			t.Fatal("invalid text became a completed transcript", v)
		}
	}
}

func TestSpeechInvalidStartNeverTouchesDevices(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err := Start(ctx, Options{}); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	for _, options := range []Options{{Locale: "bad\x00locale"}, {MaximumDuration: 2 * time.Minute}, {MaximumDuration: time.Nanosecond}} {
		if _, err := Start(t.Context(), options); err == nil {
			t.Fatal("invalid request admitted", options)
		}
	}
}

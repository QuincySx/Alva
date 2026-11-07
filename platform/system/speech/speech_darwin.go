// INPUT: Explicit microphone intent, macOS Speech/AVFoundation and the pinned purego Objective-C bridge.
// OUTPUT: On-device-only recognition with owned audio taps, callbacks and native completion.
// POS: [platform / OS primitive] No browser, cloud fallback, audio file or model request; Start alone asks OS permissions.
//go:build darwin

package speech

import (
	"context"
	"errors"
	"runtime"
	"sync"
	"sync/atomic"
	"time"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"
)

var frameworkOnce sync.Once
var frameworkError error

func Supported() bool { return true }

func message(object objc.ID, method string, args ...any) objc.ID {
	return object.Send(objc.RegisterName(method), args...)
}

func nativeString(value string) objc.ID {
	return message(message(objc.ID(objc.GetClass("NSString")), "alloc"), "initWithUTF8String:", value)
}

func runNative(ctx context.Context, options Options, stop <-chan struct{}, emit func(State, string) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	frameworkOnce.Do(func() {
		for _, path := range []string{
			"/System/Library/Frameworks/Foundation.framework/Foundation",
			"/System/Library/Frameworks/AVFoundation.framework/AVFoundation",
			"/System/Library/Frameworks/Speech.framework/Speech",
		} {
			if _, err := purego.Dlopen(path, purego.RTLD_NOW|purego.RTLD_GLOBAL); err != nil {
				frameworkError = ErrUnsupported
				return
			}
		}
	})
	if frameworkError != nil {
		return frameworkError
	}
	pool := message(objc.ID(objc.GetClass("NSAutoreleasePool")), "new")
	defer message(pool, "drain")
	bundle := message(objc.ID(objc.GetClass("NSBundle")), "mainBundle")
	for _, key := range []string{"NSMicrophoneUsageDescription", "NSSpeechRecognitionUsageDescription"} {
		name := nativeString(key)
		declared := message(bundle, "objectForInfoDictionaryKey:", name)
		message(name, "release")
		if declared == 0 {
			return errors.New("当前应用未声明系统麦克风/语音权限，请安装完整 Alva.app")
		}
	}
	localeName := nativeString(options.Locale)
	locale := message(message(objc.ID(objc.GetClass("NSLocale")), "alloc"), "initWithLocaleIdentifier:", localeName)
	message(localeName, "release")
	defer message(locale, "release")
	recognizerClass := objc.ID(objc.GetClass("SFSpeechRecognizer"))
	recognizer := message(message(recognizerClass, "alloc"), "initWithLocale:", locale)
	if recognizer == 0 {
		return ErrLocalUnavailable
	}
	defer message(recognizer, "release")
	// Apple only honors requiresOnDeviceRecognition when this capability is true.
	// https://developer.apple.com/documentation/speech/sfspeechrecognizer/supportsondevicerecognition
	if !objc.Send[bool](recognizer, objc.RegisterName("supportsOnDeviceRecognition")) {
		return ErrLocalUnavailable
	}
	if err := speechPermission(ctx, recognizerClass); err != nil {
		return err
	}
	if err := microphonePermission(ctx); err != nil {
		return err
	}
	if err := ctx.Err(); err != nil {
		return err
	}

	request := message(objc.ID(objc.GetClass("SFSpeechAudioBufferRecognitionRequest")), "new")
	if request == 0 {
		return ErrLocalUnavailable
	}
	defer message(request, "release")
	message(request, "setRequiresOnDeviceRecognition:", true)
	message(request, "setShouldReportPartialResults:", true)
	message(request, "setTaskHint:", int64(2)) // Dictation.
	if !objc.Send[bool](request, objc.RegisterName("requiresOnDeviceRecognition")) {
		return ErrLocalUnavailable
	}

	engine := message(objc.ID(objc.GetClass("AVAudioEngine")), "new")
	if engine == 0 {
		return errors.New("无法初始化系统录音设备")
	}
	defer message(engine, "release")
	node := message(engine, "inputNode")
	format := message(node, "outputFormatForBus:", uintptr(0))
	if node == 0 || format == 0 || objc.Send[float64](format, objc.RegisterName("sampleRate")) <= 0 || objc.Send[uint32](format, objc.RegisterName("channelCount")) == 0 {
		return errors.New("未找到可用的麦克风输入设备")
	}
	var audioGate, resultGate sync.RWMutex
	var acceptingAudio = true
	callbacksOpen := true
	var finalizing atomic.Bool
	var textMu sync.Mutex
	text := ""
	currentText := func() string { textMu.Lock(); defer textMu.Unlock(); return text }
	finished := make(chan error, 1)
	complete := func(err error) {
		select {
		case finished <- err:
		default:
		}
	}
	appendBuffer := objc.RegisterName("appendAudioPCMBuffer:")
	tap := objc.NewBlock(func(_ objc.Block, buffer, _ objc.ID) {
		audioGate.RLock()
		defer audioGate.RUnlock()
		if acceptingAudio && ctx.Err() == nil {
			request.Send(appendBuffer, buffer)
		}
	})
	defer tap.Release()
	result := objc.NewBlock(func(_ objc.Block, value, problem objc.ID) {
		resultGate.RLock()
		defer resultGate.RUnlock()
		if !callbacksOpen {
			return
		}
		if value != 0 {
			transcript := message(message(value, "bestTranscription"), "formattedString")
			length := objc.Send[uint64](transcript, objc.RegisterName("lengthOfBytesUsingEncoding:"), uint64(4))
			if length > 32<<10 {
				complete(errors.New("语音文字超过输入上限，请分段录入"))
				return
			}
			updated := objc.Send[string](transcript, objc.RegisterName("UTF8String"))
			textMu.Lock()
			text = updated
			textMu.Unlock()
			state := Recording
			if finalizing.Load() {
				state = Transcribing
			}
			if err := emit(state, updated); err != nil {
				complete(err)
				return
			}
			if objc.Send[bool](value, objc.RegisterName("isFinal")) {
				complete(nil)
			}
		}
		if problem != 0 {
			complete(errors.New("系统本地语音识别未完成，请重试"))
		}
	})
	defer result.Release()
	task := message(recognizer, "recognitionTaskWithRequest:resultHandler:", request, result)
	if task == 0 {
		return ErrLocalUnavailable
	}
	message(task, "retain")
	defer message(task, "release")
	message(node, "installTapOnBus:bufferSize:format:block:", uintptr(0), uint32(1024), format, tap)
	tapInstalled := true
	stopAudio := func() {
		if !tapInstalled {
			return
		}
		message(engine, "stop")
		message(node, "removeTapOnBus:", uintptr(0))
		audioGate.Lock()
		acceptingAudio = false
		audioGate.Unlock()
		tapInstalled = false
		message(request, "endAudio")
	}
	defer stopAudio()
	message(engine, "prepare")
	var nativeError objc.ID
	started := objc.Send[bool](engine, objc.RegisterName("startAndReturnError:"), &nativeError)
	var outcome error
	if !started {
		outcome = errors.New("麦克风录音启动失败，请检查系统输入设备")
	}
	if outcome == nil {
		outcome = emit(Recording, "")
	}
	deadline := time.Now().Add(options.MaximumDuration)
	if outcome != nil {
		deadline = time.Now()
	}
	var finalDeadline time.Time
	shouldCancel := outcome != nil
	finalResultReceived := false
	cancelled := ctx.Done()
	tick := time.NewTicker(20 * time.Millisecond)
	defer tick.Stop()
	for {
		if !finalizing.Load() && (shouldCancel || ctx.Err() != nil || !time.Now().Before(deadline)) {
			finalizing.Store(true)
			stopAudio()
			_ = emit(Transcribing, currentText())
			if shouldCancel || ctx.Err() != nil {
				message(task, "cancel")
			} else {
				message(task, "finish")
			}
			finalDeadline = time.Now().Add(8 * time.Second)
		}
		if finalizing.Load() {
			if ctx.Err() != nil {
				message(task, "cancel")
			}
			if !finalDeadline.IsZero() && time.Now().After(finalDeadline) {
				message(task, "cancel")
				shouldCancel = true
				if outcome == nil {
					outcome = errors.New("本地转写超时，请重试")
				}
				finalDeadline = time.Time{}
			}
			if objc.Send[int64](task, objc.RegisterName("state")) == 4 && (finalResultReceived || shouldCancel || ctx.Err() != nil) {
				// Native completed is independent of the final text callback. Join
				// an in-flight callback before releasing our request and blocks.
				resultGate.Lock()
				callbacksOpen = false
				resultGate.Unlock()
				return outcome
			}
		}
		select {
		case err := <-finished:
			finalResultReceived = true
			if err != nil {
				outcome, shouldCancel = err, true
			}
			deadline = time.Now()
		case <-stop:
			deadline = time.Now()
			stop = nil
		case <-cancelled:
			shouldCancel = true
			cancelled = nil
		case <-tick.C:
		}
	}
}

// Permission callbacks retain the native owner until their actual invocation.
// Cancelling a pending system dialog suppresses capture after its response.
func speechPermission(ctx context.Context, class objc.ID) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	status := objc.Send[int64](class, objc.RegisterName("authorizationStatus"))
	if status == 0 {
		done := make(chan int64, 1)
		block := objc.NewBlock(func(_ objc.Block, value int64) { done <- value })
		if err := ctx.Err(); err != nil {
			block.Release()
			return err
		}
		message(class, "requestAuthorization:", block)
		status = <-done
		block.Release()
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if status != 3 {
		return ErrPermissionDenied
	}
	return nil
}

func microphonePermission(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	device := objc.ID(objc.GetClass("AVCaptureDevice"))
	media := nativeString("soun")
	defer message(media, "release")
	status := objc.Send[int64](device, objc.RegisterName("authorizationStatusForMediaType:"), media)
	if status == 0 {
		done := make(chan bool, 1)
		block := objc.NewBlock(func(_ objc.Block, allowed bool) { done <- allowed })
		if err := ctx.Err(); err != nil {
			block.Release()
			return err
		}
		message(device, "requestAccessForMediaType:completionHandler:", media, block)
		allowed := <-done
		block.Release()
		if allowed {
			status = 3
		}
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if status != 3 {
		return ErrPermissionDenied
	}
	return nil
}

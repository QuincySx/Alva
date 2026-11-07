// INPUT: macOS Foundation and the actual pinned Objective-C call/block bridge.
// OUTPUT: Native strings, a dispatched native block and typed closure validation without microphone permissions.
// POS: [platform / OS primitive tests] Cold bridge check only; does not claim recorded audio or speech accuracy.
//go:build darwin

package speech

import (
	"runtime"
	"sync/atomic"
	"testing"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"
)

func TestSpeechNativeFoundationAndBlockBridge(t *testing.T) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	if _, err := purego.Dlopen("/System/Library/Frameworks/Foundation.framework/Foundation", purego.RTLD_NOW|purego.RTLD_GLOBAL); err != nil {
		t.Fatal(err)
	}
	pool := message(objc.ID(objc.GetClass("NSAutoreleasePool")), "new")
	defer message(pool, "drain")
	want := "本地录入 · gyp 123"
	text := nativeString(want)
	defer message(text, "release")
	if got := objc.Send[string](text, objc.RegisterName("UTF8String")); got != want {
		t.Fatal("native UTF8 bridge changed text", got)
	}
	var status int64
	permission := objc.NewBlock(func(_ objc.Block, value int64) { status = value })
	defer permission.Release()
	permission.Invoke(int64(3))
	if status != 3 {
		t.Fatal("authorization block ABI mismatch")
	}
	var allowed bool
	microphone := objc.NewBlock(func(_ objc.Block, value bool) { allowed = value })
	defer microphone.Release()
	microphone.Invoke(true)
	if !allowed {
		t.Fatal("microphone callback ABI mismatch")
	}
	// Block.Invoke above checks typed closures, not the native invocation ABI.
	// Dispatch a real native block to check the C-to-Go trampoline separately.
	lib, err := purego.Dlopen("/usr/lib/system/libdispatch.dylib", purego.RTLD_NOW|purego.RTLD_GLOBAL)
	if err != nil {
		t.Fatal(err)
	}
	var queue func(int64, uintptr) uintptr
	var dispatch func(uintptr, objc.Block)
	purego.RegisterLibFunc(&queue, lib, "dispatch_get_global_queue")
	purego.RegisterLibFunc(&dispatch, lib, "dispatch_sync")
	var called atomic.Bool
	callback := objc.NewBlock(func(_ objc.Block) { called.Store(true) })
	defer callback.Release()
	dispatch(queue(0, 0), callback)
	if !called.Load() {
		t.Fatal("native block did not invoke its Go callback")
	}
}

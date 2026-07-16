// INPUT:  std::{alloc, fs, slice}, host import alva:host/llm::llm_complete
// OUTPUT: alloc(len) wasm export, /work/result.txt containing the blocking host response
// POS:    Minimal WASIp1 command guest proving the host-proxied LLM pointer/length ABI.

#[cfg(target_os = "wasi")]
use std::alloc::{self, Layout};
#[cfg(target_os = "wasi")]
use std::{fs, slice};

#[cfg(target_os = "wasi")]
#[link(wasm_import_module = "alva:host/llm")]
extern "C" {
    fn llm_complete(req_ptr: i32, req_len: i32) -> i64;
}

/// The layout every [`alloc`] result is created with: `len` bytes, align 1.
///
/// Alloc and dealloc must agree on the exact layout. This is why the guest
/// does not round-trip the host's buffer through `Vec`: `from_raw_parts`
/// demands the precise capacity the allocation was made with, while
/// `Vec::with_capacity(n)` only promises *at least* `n` — a mismatch would
/// free with the wrong layout, which is undefined behavior.
#[cfg(target_os = "wasi")]
fn response_layout(len: usize) -> Layout {
    Layout::from_size_align(len, 1).expect("response length fits a valid align-1 layout")
}

/// Allocates `len` bytes of guest linear memory for the host to fill.
///
/// Returns a pointer the host writes exactly `len` bytes into before
/// `llm_complete` returns. A zero-length request needs no allocation and
/// yields a null pointer, which the caller pairs with `resp_len == 0`.
#[cfg(target_os = "wasi")]
#[no_mangle]
pub extern "C" fn alloc(len: i32) -> i32 {
    let len = usize::try_from(len).expect("host requested a negative allocation");
    if len == 0 {
        return 0;
    }
    // SAFETY: `len` is non-zero, so the layout has non-zero size as
    // `alloc::alloc` requires.
    unsafe { alloc::alloc(response_layout(len)) as i32 }
}

#[cfg(target_os = "wasi")]
fn main() {
    let task = fs::read("/work/task.txt").expect("read /work/task.txt");
    let req_len = i32::try_from(task.len()).expect("task exceeds ptr/len ABI limit");
    let packed = unsafe { llm_complete(task.as_ptr() as i32, req_len) } as u64;
    let resp_ptr = (packed >> 32) as u32 as usize;
    let resp_len = (packed & u32::MAX as u64) as u32 as usize;

    // An empty completion is a legitimate outcome (a turn with no text), not a
    // reason to trap: it round-trips as an empty result file.
    let response = if resp_len == 0 {
        Vec::new()
    } else {
        assert!(
            resp_ptr != 0,
            "host returned a null pointer for a non-empty response"
        );
        // SAFETY: the host obtained this allocation from `alloc(resp_len)` and
        // initialized exactly `resp_len` bytes before returning. The guest
        // copies them out and frees with the same layout `alloc` used.
        let bytes = unsafe { slice::from_raw_parts(resp_ptr as *const u8, resp_len) }.to_vec();
        unsafe { alloc::dealloc(resp_ptr as *mut u8, response_layout(resp_len)) };
        bytes
    };

    fs::write("/work/result.txt", response).expect("write /work/result.txt");
}

#[cfg(not(target_os = "wasi"))]
fn main() {
    eprintln!("alva-worker-wasm is a wasm32-wasip1 guest binary");
}

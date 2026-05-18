//! macOS Sequoia (15+) Local Network Privacy warmup for Rust binaries.
//!
//! ## The problem
//!
//! Rust binaries using BSD sockets (std::net / tokio / hyper / reqwest)
//! CANNOT trigger the macOS Local Network Privacy prompt on Sequoia.
//! The prompt only fires for callers using Apple's higher-level network
//! APIs (Bonjour / Network.framework / NSURLSession). Without a prompt,
//! the binary never gets registered in TCC, so there's no entry the
//! user can toggle in System Settings → Privacy & Security → Local
//! Network, and every connect() to a private (RFC1918) IP fails with
//! EHOSTUNREACH.
//!
//! ## What this does
//!
//! Calls `DNSServiceBrowse` (the Bonjour C API), which is the canonical
//! macOS API for local-network discovery. The first time this runs from
//! a stable codesign identity:
//!
//!   1. macOS shows the "allow this app to access local network" prompt.
//!   2. The user clicks Allow.
//!   3. An entry appears under System Settings → Privacy & Security →
//!      Local Network.
//!   4. ALL subsequent network calls from binaries signed with the same
//!      identity — including plain BSD sockets — inherit the grant.
//!
//! ## Usage
//!
//! Must be run inside a GUI terminal (iTerm / Terminal.app) so the
//! prompt can actually appear. From scripts/run-eval.sh:
//!
//! ```bash
//! cargo run --example lnp_warmup
//! ```

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr;
use std::time::{Duration, Instant};

// --- Bonjour C API bindings (subset of <dns_sd.h>) ---
//
// dns_sd lives in libSystem on macOS, so no extra linker flag is needed.

#[allow(non_camel_case_types)]
type DNSServiceRef = *mut c_void;
#[allow(non_camel_case_types)]
type DNSServiceFlags = u32;
#[allow(non_camel_case_types)]
type DNSServiceErrorType = i32;

#[allow(non_snake_case)]
type DNSServiceBrowseReply = extern "C" fn(
    sdRef: DNSServiceRef,
    flags: DNSServiceFlags,
    interfaceIndex: u32,
    errorCode: DNSServiceErrorType,
    serviceName: *const c_char,
    regtype: *const c_char,
    replyDomain: *const c_char,
    context: *mut c_void,
);

extern "C" {
    fn DNSServiceBrowse(
        sd_ref: *mut DNSServiceRef,
        flags: DNSServiceFlags,
        interface_index: u32,
        regtype: *const c_char,
        domain: *const c_char,
        call_back: DNSServiceBrowseReply,
        context: *mut c_void,
    ) -> DNSServiceErrorType;

    fn DNSServiceRefSockFD(sd_ref: DNSServiceRef) -> c_int;
    fn DNSServiceProcessResult(sd_ref: DNSServiceRef) -> DNSServiceErrorType;
    fn DNSServiceRefDeallocate(sd_ref: DNSServiceRef);
}

// --- poll(2) for waiting on the dns-sd file descriptor ---

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

const POLLIN: i16 = 0x0001;

extern "C" {
    fn poll(fds: *mut PollFd, nfds: u32, timeout_ms: c_int) -> c_int;
}

// --- Browse reply callback (function pointer, no obj-c block needed) ---

extern "C" fn browse_reply(
    _sd_ref: DNSServiceRef,
    _flags: DNSServiceFlags,
    _interface_index: u32,
    error_code: DNSServiceErrorType,
    service_name: *const c_char,
    regtype: *const c_char,
    reply_domain: *const c_char,
    _context: *mut c_void,
) {
    if error_code != 0 {
        eprintln!("  [browse_reply] error code = {error_code}");
        return;
    }
    let cstr = |p: *const c_char| -> String {
        if p.is_null() {
            "(null)".into()
        } else {
            unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() }
        }
    };
    eprintln!(
        "  [discovered] {} . {} . {}",
        cstr(service_name),
        cstr(regtype),
        cstr(reply_domain)
    );
}

fn main() {
    eprintln!("==> macOS Local Network Privacy warmup");
    eprintln!();
    eprintln!("This binary calls Bonjour's DNSServiceBrowse to provoke the");
    eprintln!("macOS Local Network prompt. If a system dialog appears asking");
    eprintln!("you to allow Local Network access — CLICK ALLOW.");
    eprintln!();
    eprintln!("Once granted, subsequent runs (and all other binaries signed");
    eprintln!("with the same codesign identity) will inherit the permission,");
    eprintln!("including plain BSD socket calls in std::net / tokio / hyper.");
    eprintln!();

    // Browse for the standard "service discovery" type — broad, harmless,
    // commonly used and guaranteed to trigger LNP gating.
    let regtype = CString::new("_services._dns-sd._udp").unwrap();
    let domain = CString::new("local.").unwrap();

    let mut sd_ref: DNSServiceRef = ptr::null_mut();
    let err = unsafe {
        DNSServiceBrowse(
            &mut sd_ref,
            0, // flags
            0, // any interface
            regtype.as_ptr(),
            domain.as_ptr(),
            browse_reply,
            ptr::null_mut(),
        )
    };
    if err != 0 {
        eprintln!(
            "✗ DNSServiceBrowse failed with code {err}. \n\
             A nonzero return BEFORE the prompt fires usually means LNP \n\
             pre-denied — try System Settings → Privacy & Security → Local \n\
             Network and look for any entry related to this binary."
        );
        std::process::exit(1);
    }
    eprintln!("✓ DNSServiceBrowse started (prompt should appear if not already granted).");
    eprintln!("  Waiting up to 60 s for results (each line below = a discovered service).");
    eprintln!();

    let fd = unsafe { DNSServiceRefSockFD(sd_ref) };
    if fd < 0 {
        eprintln!("✗ DNSServiceRefSockFD returned invalid fd");
        std::process::exit(1);
    }

    let mut results_seen = 0u32;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(60) {
        let mut pfd = PollFd {
            fd,
            events: POLLIN,
            revents: 0,
        };
        let r = unsafe { poll(&mut pfd, 1, 1000) };
        if r > 0 {
            let perr = unsafe { DNSServiceProcessResult(sd_ref) };
            if perr != 0 {
                eprintln!("  [process_result] error {perr}");
                break;
            }
            results_seen += 1;
        }
    }

    unsafe { DNSServiceRefDeallocate(sd_ref) };

    eprintln!();
    if results_seen > 0 {
        eprintln!(
            "✓ Discovered {} reply event(s). Bonjour path OK.",
            results_seen
        );
        eprintln!();
        eprintln!("==> Now triggering BSD-socket connect to register binary in TCC.");
        eprintln!("    The first connect attempt provokes the prompt but is expected");
        eprintln!("    to fail-fast with EHOSTUNREACH. The PROMPT shows up shortly");
        eprintln!("    AFTER. We'll then sleep 60s — keep an eye on your screen,");
        eprintln!("    click ALLOW on the macOS prompt when it appears, then we");
        eprintln!("    retry the connect to verify the grant landed.");
        eprintln!();

        let target = bsd_connect_target();
        if let Some(addr) = target {
            eprintln!("==> attempt 1: connect to {addr} (expected to fail and queue the prompt)");
            match std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
                Ok(_) => {
                    eprintln!(
                        "✓ Already granted! No prompt needed — your codesign identity\n\
                         had Local Network access previously."
                    );
                    return;
                }
                Err(e) => eprintln!(
                    "  ✗ failed as expected: {e} (raw_os_error={:?})\n\
                     \n\
                     Now WAITING 60 seconds for the macOS prompt to surface.\n\
                     ⚠️  Look at your screen and click ALLOW when the dialog appears.",
                    e.raw_os_error()
                ),
            }

            // Long sleep with countdown so the user knows the process is still alive
            // and the prompt has a real chance to land + be clicked.
            for remaining in (1..=60).rev() {
                if remaining % 10 == 0 || remaining <= 5 {
                    eprintln!("  …{remaining} s remaining (click ALLOW on the prompt)");
                }
                std::thread::sleep(Duration::from_secs(1));
            }

            eprintln!();
            eprintln!("==> attempt 2: retry connect to {addr}");
            match std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
                Ok(s) => eprintln!(
                    "✓ CONNECTED. Local Network grant is live for this codesign\n\
                     identity. All future binaries signed with alva-eval-signing\n\
                     (or whatever EVAL_SIGN_IDENTITY is set to) will inherit it.\n\
                     local={:?} peer={:?}",
                    s.local_addr().ok(),
                    s.peer_addr().ok()
                ),
                Err(e) => eprintln!(
                    "✗ STILL failing: {e} (raw_os_error={:?})\n\
                     \n\
                     Diagnose:\n\
                       1. Did you actually click Allow on the prompt? (Sometimes\n\
                          the prompt is hidden under other windows.)\n\
                       2. Check System Settings → Privacy & Security → Local\n\
                          Network — is there an entry for the binary / its\n\
                          codesign identity, toggled ON?\n\
                       3. If no entry appeared, the prompt may not have fired\n\
                          at all (running over SSH, no GUI session). Re-run\n\
                          inside iTerm/Terminal.app.",
                    e.raw_os_error()
                ),
            }
        }
    } else {
        eprintln!(
            "⚠ No discovery replies in 60 s. Either:\n\
             - No Bonjour services are advertising on this network (uncommon\n\
               on a normal LAN), or\n\
             - The prompt was dismissed / no prompt fired (running over SSH\n\
               or no codesign identity).\n\
             \n\
             Check System Settings → Privacy & Security → Local Network."
        );
    }
}

fn bsd_connect_target() -> Option<std::net::SocketAddr> {
    let raw = std::env::var("EVAL_BASE_URL")
        .ok()
        .as_deref()
        .and_then(|u| {
            u.trim_start_matches("http://")
                .trim_start_matches("https://")
                .split('/')
                .next()
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "10.10.1.100:10443".into());
    raw.parse().ok()
}

//! Minimal TCP connect probe. Defaults to a battery of targets covering
//! public internet (DNS resolvable), public internet (IP literal),
//! local-network (RFC1918), and loopback — distinguishes "this whole
//! process can't outbound" from "only this network can't be reached".
//!
//! Override with explicit target(s) as args:
//!     cargo run --example raw_tcp_test -- 10.10.1.100:10443 8.8.8.8:53

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

fn main() {
    let user_targets: Vec<String> = std::env::args().skip(1).collect();
    let targets: Vec<&str> = if user_targets.is_empty() {
        vec![
            "10.10.1.100:10443",  // user's local LLM endpoint
            "8.8.8.8:53",         // Google DNS — public IP literal
            "1.1.1.1:443",        // Cloudflare — public IP literal
            "google.com:443",     // public, requires DNS
            "127.0.0.1:22",       // loopback (likely refused, but reachable)
        ]
    } else {
        user_targets.iter().map(|s| s.as_str()).collect()
    };

    println!("pid = {}", std::process::id());
    println!("exe = {:?}", std::env::current_exe().ok());
    println!();

    for target in targets {
        print!("{:<25} → ", target);
        match target.to_socket_addrs() {
            Ok(mut iter) => match iter.next() {
                Some(addr) => match TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
                    Ok(s) => println!(
                        "✓ connected (local={:?})",
                        s.local_addr().ok().map(|a| a.to_string())
                    ),
                    Err(e) => println!(
                        "✗ {e} (raw_os_error={:?})",
                        e.raw_os_error()
                    ),
                },
                None => println!("✗ resolved to 0 addresses"),
            },
            Err(e) => println!("✗ resolve failed: {e}"),
        }
    }
}

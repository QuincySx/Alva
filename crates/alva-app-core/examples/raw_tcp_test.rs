//! Minimal TCP connect probe. Run via `cargo run --example raw_tcp_test`.
//! Compares behavior under `cargo run` vs `cargo test`-spawned binaries.

use std::net::TcpStream;
use std::time::Duration;

fn main() {
    let target = std::env::args().nth(1).unwrap_or_else(|| "10.10.1.100:10443".into());
    println!("target = {target}");
    println!("pid = {}", std::process::id());
    println!("exe = {:?}", std::env::current_exe().ok());

    match TcpStream::connect_timeout(
        &target.parse().expect("parse SocketAddr"),
        Duration::from_secs(5),
    ) {
        Ok(s) => println!(
            "✓ connected, local={:?} peer={:?}",
            s.local_addr().ok(),
            s.peer_addr().ok()
        ),
        Err(e) => println!(
            "✗ failed: {e} (raw_os_error = {:?})",
            e.raw_os_error()
        ),
    }
}

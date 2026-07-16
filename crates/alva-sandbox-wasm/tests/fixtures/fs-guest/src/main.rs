// INPUT:  std::{env, fs, hint::black_box, process}
// OUTPUT: WASIp1 command fixture stdout/stderr plus granted-directory filesystem effects
// POS:    Exercises filesystem confinement, process exit, and host-enforced time/memory limits.

use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    let outside_absolute_path = args.first().expect("runner passes outside path");
    let command = args.get(1).map(String::as_str).unwrap_or_default();

    if command == "spin" {
        loop {
            std::hint::black_box(1_u64.wrapping_add(1));
        }
    }
    if command == "grow-memory" {
        let mut chunks = Vec::new();
        loop {
            chunks.push(vec![0xA5_u8; 1024 * 1024]);
            std::hint::black_box(&chunks);
        }
    }

    fs::write("/work/new.txt", "created-in-sandbox").expect("create file");

    let existing = fs::read_to_string("/work/existing.txt").expect("read existing file");
    println!("READ existing.txt: {}", existing.trim());

    fs::write(
        "/work/existing.txt",
        format!("{}+modified", existing.trim()),
    )
    .expect("overwrite existing file");

    let mut names: Vec<_> = fs::read_dir("/work")
        .expect("list work directory")
        .map(|entry| {
            entry
                .expect("read directory entry")
                .file_name()
                .into_string()
                .expect("fixture uses UTF-8 names")
        })
        .collect();
    names.sort();
    println!("LIST /work: {names:?}");

    fs::write("/work/tmp-delete-me.txt", "temporary").expect("create temporary file");
    fs::remove_file("/work/tmp-delete-me.txt").expect("delete temporary file");
    println!("DELETE ok");

    fs::create_dir("/work/subdir").expect("create subdirectory");
    fs::rename("/work/new.txt", "/work/subdir/renamed.txt").expect("rename into subdirectory");
    println!("MKDIR+RENAME ok");
    println!("ARGS: {command}");

    match fs::read_to_string(outside_absolute_path) {
        Ok(_) => println!("ESCAPE-1 !!! read host file"),
        Err(error) => println!("ESCAPE-1 blocked: {:?}", error.kind()),
    }

    match fs::read_to_string("/work/../outside/secret.txt") {
        Ok(_) => println!("ESCAPE-2 !!! dotdot escaped"),
        Err(error) => println!("ESCAPE-2 blocked: {:?}", error.kind()),
    }

    if command == "exit-7" {
        eprintln!("fixture requested exit 7");
        std::process::exit(7);
    }
}

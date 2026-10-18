// INPUT: ownership pattern from dirty tool_batch.rs:282–285
// OUTPUT: expected rustc E0382 under --cfg test
// POS: minimal language-level witness, not a full production build
struct Scheduling { resource_keys: Vec<String> }
fn scheduling_mode_name(_: &Scheduling) -> &'static str { "parallel" }
fn main() {
    let first_scheduling = Scheduling { resource_keys: vec![] };
    let mut claimed = first_scheduling.resource_keys;
    #[cfg(test)]
    eprintln!("DBG-WAVE first candidate ok, mode={:?}", scheduling_mode_name(&first_scheduling));
    claimed.clear();
}

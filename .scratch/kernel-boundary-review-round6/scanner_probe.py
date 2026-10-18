# INPUT: Repository public-surface scanner and synthetic Rust contracts
# OUTPUT: Before/after snapshot comparisons
# POS: Independent round-six API gate review probe
"""Read-only before/after probes for R56; unchanged output means missed ABI change."""
import runpy
s = runpy.run_path('scripts/public-surface.py')
def snapshot(source):
    records = []
    s['scan_scope'](s['strip_comments_and_strings'](source), '', True, records)
    return s['render'](records)
cases = [
    ('attributed field', 'pub struct Config { #[cfg(unix)] pub retries: u32 }', 'pub struct Config { #[cfg(unix)] pub retries: u64 }'),
    ('variant payload', 'pub enum E { Data(u32) }', 'pub enum E { Data(String) }'),
    ('assoc bounds', 'pub trait T { type Item: Send; }', 'pub trait T { type Item: Sync; }'),
    ('attributed variant', 'pub enum E { #[cfg(unix)] One, Two }', 'pub enum E { #[cfg(unix)] Changed, Two }'),
    ('arrow then variant', 'pub enum E { Handler(fn() -> u32), Tail }', 'pub enum E { Handler(fn() -> u32), Changed }'),
    ('stacked item attributes', '#[derive(Debug)]\n#[repr(u8)]\npub enum E { One }', '#[derive(Debug)]\n#[repr(u8)]\npub enum E { Two }'),
]
for name, before, after in cases:
    a, b = snapshot(before), snapshot(after)
    print(f'{name}: {"MISSED" if a == b else "DETECTED"}; before={a!r}; after={b!r}')

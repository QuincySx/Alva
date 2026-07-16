// INPUT:  alva_sandbox_abi escalation DTOs/limits, crate::{Grant,SandboxStoreData}, serde_json, std::{io,path}, wasmtime
// OUTPUT: register_escalation_proxy, translate_guest_cwd
// POS:    Versioned escalation memory bridge plus fail-closed guest-to-host grant path translation.

use crate::{Grant, SandboxStoreData};
use alva_sandbox_abi::{
    EscalationProxyRequest, EscalationProxyResult, ESCALATION_PROXY_ABI_VERSION,
    MAX_ESCALATION_PROXY_REQUEST_BYTES, MAX_ESCALATION_PROXY_RESPONSE_BYTES,
};
use std::io;
use std::path::{Component, Path, PathBuf};
use wasmtime::{Caller, Extern, Linker};

struct BoundedJsonBuffer {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}

impl BoundedJsonBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            exceeded: false,
        }
    }
}

impl io::Write for BoundedJsonBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > self.limit {
            self.exceeded = true;
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                "escalation proxy JSON exceeds byte limit",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Register the blocking `alva:host/escalation::execute` import.
///
/// This layer owns only ABI mechanics. The callback owns cwd translation,
/// permission policy, execution and audit persistence.
pub fn register_escalation_proxy<F>(
    linker: &mut Linker<SandboxStoreData>,
    execute: F,
) -> Result<(), wasmtime::Error>
where
    F: Fn(EscalationProxyRequest) -> Result<EscalationProxyResult, String> + Send + Sync + 'static,
{
    linker.func_wrap(
        "alva:host/escalation",
        "execute",
        move |mut caller: Caller<'_, SandboxStoreData>, req_ptr: i32, req_len: i32| {
            let request = read_request(&mut caller, req_ptr, req_len)?;
            let result = execute(request).map_err(wasmtime::Error::msg)?;
            if !result.has_supported_version() {
                return Err(wasmtime::Error::msg(format!(
                    "escalation callback returned version {}; host supports {}",
                    result.version, ESCALATION_PROXY_ABI_VERSION
                )));
            }
            write_result(&mut caller, &result)
        },
    )?;
    Ok(())
}

fn read_request(
    caller: &mut Caller<'_, SandboxStoreData>,
    req_ptr: i32,
    req_len: i32,
) -> Result<EscalationProxyRequest, wasmtime::Error> {
    let req_start = usize::try_from(req_ptr)
        .map_err(|_| wasmtime::Error::msg("negative escalation request pointer"))?;
    let req_len = usize::try_from(req_len)
        .map_err(|_| wasmtime::Error::msg("negative escalation request length"))?;
    if req_len > MAX_ESCALATION_PROXY_REQUEST_BYTES {
        return Err(wasmtime::Error::msg(format!(
            "escalation request is {req_len} bytes; limit is {MAX_ESCALATION_PROXY_REQUEST_BYTES} bytes"
        )));
    }
    let req_end = req_start
        .checked_add(req_len)
        .ok_or_else(|| wasmtime::Error::msg("escalation request range overflow"))?;
    let memory = caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| wasmtime::Error::msg("guest did not export memory"))?;
    let encoded = memory
        .data(&*caller)
        .get(req_start..req_end)
        .ok_or_else(|| wasmtime::Error::msg("escalation request is outside guest memory"))?;
    let request: EscalationProxyRequest = serde_json::from_slice(encoded)
        .map_err(|error| wasmtime::Error::msg(format!("decode escalation request: {error}")))?;
    if !request.has_supported_version() {
        return Err(wasmtime::Error::msg(format!(
            "unsupported escalation request version {}; host supports {}",
            request.version, ESCALATION_PROXY_ABI_VERSION
        )));
    }
    Ok(request)
}

fn write_result(
    caller: &mut Caller<'_, SandboxStoreData>,
    result: &EscalationProxyResult,
) -> Result<i64, wasmtime::Error> {
    let mut encoded = BoundedJsonBuffer::new(MAX_ESCALATION_PROXY_RESPONSE_BYTES);
    if let Err(error) = serde_json::to_writer(&mut encoded, result) {
        return Err(wasmtime::Error::msg(if encoded.exceeded {
            format!(
                "escalation response exceeds the {MAX_ESCALATION_PROXY_RESPONSE_BYTES}-byte JSON limit"
            )
        } else {
            format!("encode escalation response: {error}")
        }));
    }
    let response = encoded.bytes;
    let resp_len = i32::try_from(response.len())
        .map_err(|_| wasmtime::Error::msg("escalation response exceeds ptr/len ABI limit"))?;
    let memory = caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| wasmtime::Error::msg("guest did not export memory"))?;
    let alloc = caller
        .get_export("alloc")
        .and_then(Extern::into_func)
        .ok_or_else(|| wasmtime::Error::msg("guest did not export alloc"))?
        .typed::<i32, i32>(&caller)?;
    let resp_ptr = alloc.call(&mut *caller, resp_len)?;
    let resp_start = usize::try_from(resp_ptr)
        .map_err(|_| wasmtime::Error::msg("guest alloc returned a negative pointer"))?;
    memory.write(&mut *caller, resp_start, &response)?;
    let packed = (u64::from(resp_ptr as u32) << 32) | u64::from(resp_len as u32);
    Ok(packed as i64)
}

/// Translate an absolute guest cwd through the current run's grant table.
///
/// Both the guest path and the resolved host path are checked. Canonicalizing
/// the final cwd rejects symlinks inside a grant that point outside its host
/// root, while longest-prefix matching handles nested guest mounts.
pub fn translate_guest_cwd(grants: &[Grant], guest_cwd: &str) -> Result<PathBuf, String> {
    let guest_cwd = normalize_absolute_guest_path(guest_cwd)?;
    let mut candidates = grants
        .iter()
        .filter_map(|grant| {
            let guest_root = normalize_absolute_guest_path(&grant.guest).ok()?;
            guest_cwd.strip_prefix(&guest_root).ok().map(|relative| {
                (
                    guest_root.components().count(),
                    grant,
                    relative.to_path_buf(),
                )
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(depth, _, _)| std::cmp::Reverse(*depth));
    let (_, grant, relative) = candidates.first().ok_or_else(|| {
        format!(
            "guest cwd {:?} is outside this job's granted guest paths",
            guest_cwd.display()
        )
    })?;

    let host_root = grant.host.canonicalize().map_err(|error| {
        format!(
            "cannot resolve host grant {} for guest path {:?}: {error}",
            grant.host.display(),
            grant.guest
        )
    })?;
    let host_cwd = host_root.join(relative).canonicalize().map_err(|error| {
        format!(
            "cannot resolve guest cwd {:?} inside grant {:?}: {error}",
            guest_cwd.display(),
            grant.guest
        )
    })?;
    if !host_cwd.starts_with(&host_root) {
        return Err(format!(
            "guest cwd {:?} resolves outside host grant {:?}",
            guest_cwd.display(),
            grant.guest
        ));
    }
    if !host_cwd.is_dir() {
        return Err(format!(
            "guest cwd {:?} does not resolve to a directory",
            guest_cwd.display()
        ));
    }
    Ok(host_cwd)
}

fn normalize_absolute_guest_path(raw: &str) -> Result<PathBuf, String> {
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err(format!("guest cwd {raw:?} must be absolute"));
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => normalized.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(format!("guest cwd {raw:?} escapes above guest root"));
                }
            }
            Component::Normal(part) => normalized.push(part),
            Component::Prefix(_) => {
                return Err(format!("guest cwd {raw:?} uses a non-WASI path prefix"))
            }
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_cwd_translates_only_through_grants() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let nested = workspace.join("crate-a");
        std::fs::create_dir_all(&nested).unwrap();
        let grants = vec![Grant::read_write(&workspace, "/workspace")];

        assert_eq!(
            translate_guest_cwd(&grants, "/workspace/crate-a").unwrap(),
            nested.canonicalize().unwrap()
        );
        assert!(translate_guest_cwd(&grants, "/Users/host/project").is_err());
        assert!(translate_guest_cwd(&grants, "/workspace/../outside").is_err());
        assert!(translate_guest_cwd(&grants, "../workspace").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn guest_cwd_rejects_symlink_escape_from_host_grant() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, workspace.join("escape")).unwrap();
        let grants = vec![Grant::read_write(&workspace, "/workspace")];

        let error = translate_guest_cwd(&grants, "/workspace/escape").unwrap_err();
        assert!(error.contains("outside host grant"), "{error}");
    }

    #[test]
    fn nested_guest_mount_uses_the_most_specific_grant() {
        let root = tempfile::tempdir().unwrap();
        let outer = root.path().join("outer");
        let inner = root.path().join("inner");
        std::fs::create_dir_all(outer.join("nested")).unwrap();
        std::fs::create_dir_all(&inner).unwrap();
        let grants = vec![
            Grant::read_write(&outer, "/workspace"),
            Grant::read_write(&inner, "/workspace/nested"),
        ];

        assert_eq!(
            translate_guest_cwd(&grants, "/workspace/nested").unwrap(),
            inner.canonicalize().unwrap()
        );
    }
}

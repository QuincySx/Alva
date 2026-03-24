# alva-agent-security

> Security subsystem: path filtering, authorized roots, HITL permission management, and macOS sandbox profiles.

---

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Crate Root | src/lib.rs | Declares security modules and re-exports the public API |
| SecurityGuard | src/guard.rs | Unified security gate composing sensitive-path filtering, authorized-root checking, and HITL permission management |
| PermissionManager | src/permission.rs | Session-level HITL permission manager with cached always-allow/deny decisions and async approval flow |
| SensitivePathFilter | src/sensitive_paths.rs | Filters access to sensitive paths (secrets, certificates, private config) using dirs, extensions, filenames, and regex patterns |
| AuthorizedRoots | src/authorized_roots.rs | Manages the set of authorized directories the agent can access, defaulting to workspace root |
| SandboxConfig | src/sandbox.rs | Configures macOS Seatbelt sandbox profiles for restricting shell command execution |

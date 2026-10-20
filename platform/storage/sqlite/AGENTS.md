# 横向支撑 · sqlite
> Native SQLite opening and process ownership without session authorization policy.

## 地位
Private database plumbing for explicit storage implementations. Other components use system/privatefs for private directories without inheriting SQL.
## 逻辑
`LockDatabase` acquires an exclusive process lock before a writable owner opens a database; the caller closes both independently. `Open` returns one configured `database/sql.DB` and never imports historical data. Explicit host connection initializers can register optional built-in extensions such as FTS5; safety PRAGMAs and initializers run for every actual connection/reconnection, not only the first.
## 约束
Existing final symlinks, non-regular/hardlinked files, foreign ownership or broad access are rejected without changing existing permissions. Unix uses 0700/0600 and flock; Windows uses creation-time protected owner-only DACLs, handle checks and nonblocking LockFileEx. Files created by SQLite inherit the private directory ACE. Only basename database names are accepted. WAL is not used; foreign keys and FULL sync are enabled. Windows runtime acceptance remains separate from cross-compilation.
## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Database | sqlite.go | Safe path setup and configured native SQLite connection |
| Unix ownership | private_unix.go | Exclusive advisory process lock; UID/link checks are system/privatefs primitives |
| Windows ownership | private_windows.go | Actual LockFileEx/UnlockFileEx; private path/file checks are system/privatefs |
| Windows regressions | private_windows_test.go | 真实跨进程排他/释放、SQLite持久重开、底层文件DACL检查 |
| Other platforms | private_other.go | Explicit unsupported security implementation |
| Regression | sqlite_test.go | Private file, symlink, lock and foreign/wide permission rejection |

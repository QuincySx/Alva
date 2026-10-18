# L7 · Managed Worker worktrees
> Explicit approved Git operations in new product-private targets, never a Rust compatibility bridge.

## 地位

Concrete L7 native capability; Hub core consumes only pure product facts. No SDK/kernel import or global Git/account discovery.

## 逻辑

Git executable is an explicit host-approved immutable configuration snapshot. Fixed commands clear host environment and disable system/global config, hooks, fsmonitor, credential helpers, external diffs and network protocols. Owned process capture is bounded and cancellation joins actual readers/reap; incomplete physical cleanup retains the exact child for Close retry.
Whole operations serialize their large snapshots, have at most eight owned admissions and a two-minute owner/caller deadline. Every command has thirty seconds and 64KiB stderr/explicit stdout limits. Trees have at most4096 regular files,32 path components,20MiB/file and128MiB raw total. Private old/new evidence is64MiB/snapshot; patches16MiB; retained evidence at most32/256MiB. Reaching a cap fails visibly; nothing is silently deleted. Explicit evidence archive/recovery UI remains unimplemented. Removal has a separate cold whole-tree/index/HEAD review, exact product revision plus independent deletion confirmation, registered-path/branch/common-repository validation and actual Git removal/postcondition. It works after the Lead HEAD advances, retains branch/history/evidence, and refuses stale bytes/index or unmanaged targets; failures remain unknown, never replayed.

## 约束

No shell, user worktree reset, automatic commit/push, user profile/config migration or credential access. New private worktrees use no-checkout and direct blob copying so repository clean/smudge filters never execute. Unsupported symlinks/submodules/oversized trees fail explicitly. Creation is not adoption or proof of verification; product/native UI integration is separate acceptance. Preview saves private hash-verified binary patches and original/proposed literal bytes without `git add`/`status`. Apply does not use Git apply (which can invoke repository filters): it prepares all bytes, rechecks explicit paths, applies literal replacements/deletions and verifies final SHA/mode. It requires durable adopting status and exclusive leases on both workspaces; HEAD/index/source must still equal the saved base. Failed application is unknown/inspect-only, never replay/reset/rollback; private original copies remain for human recovery. Hash/recheck is optimistic, not an OS atomic hash-CAS, multi-file atomic transaction or hostile same-UID sandbox.
Actual parent descriptors are pinned component-by-component with before/after inode checks; final file IO is basename-only on those handles. Git process cwd separately has canonical path and before/after startup identity checks: Darwin cannot chdir into /dev/fd directory handles, so native process-level atomic fchdir is NOT provided or claimed. Constructors start no Git. Product/native UI acceptance uses the same Manager; existing native history cannot be retargeted, dirty/shared-head changes fail rather than merge, and the first version supports one explicit adoption per isolated Worker.
Existing Lead regular-file permissions are captured internally and bound into the shared evidence hash. Preserve read/write and unchanged execute bits; an explicit Git executable transition only adds owner execute, or clears all execute bits. New files remain0600/0700; the Worker's private rw permissions are never copied to broaden access. Stage at0600, write, then FD.Chmod and Sync so umask cannot silently strip pre-existing group read. Recheck permissions before replacement and verify them afterward; special privilege bits are rejected. Full ACL/xattrs are not preserved or claimed.
Windows parity is unfinished: the constructor checks Unix executable bits, and snapshot/apply evidence binds Unix file modes. Merely admitting git.exe would not migrate those semantics; Windows mode/ACL policy and native acceptance remain separate work.

## 业务域清单

| 文件 | 职责 |
|---|---|
| git.go | Explicit approved Git executor, bounded process/reader ownership and terminal Close |
| create.go | Repository-root validation, no-checkout managed worktree and bounded direct blob population |
| remove.go / remove_test.go | Explicit reviewed managed-only removal, revalidated whole-tree/index/HEAD, real Git exit and retained branch/evidence; stale/foreign/cancel refusal |
| create_test.go | Actual temporary Git repository, unadopted edits stay isolated, hooks/smudge trap and explicit target/mode refusal |
| snapshot.go | Bounded literal tree/index/file snapshots, symlink refusal and deterministic SHA/mode evidence |
| evidence.go | Private immutable binary patches, explicit small raw pages and checked no-commit adoption |
| evidence_test.go | Actual text/binary/mode/deletion adoption and stale/dirty/symlink/filter/tamper refusal |
| pinning_test.go | Real renamed-parent/internal-symlink descriptor proof and late-owned-child Close retry, never false cleanup |
| budget.go | Bounded private evidence count/bytes admission without deleting recovery originals |
| budget_test.go | Metadata-only retention-cap/no-delete and serial admission owner-cancel/Close retry regressions |
| permissions_test.go | Actual0640/0750 text/binary preservation, owner-only exec/new transitions, mode/content stale refusal and special privilege rejection |

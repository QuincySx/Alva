// INPUT:  ResourceClaims declared by mutating WorkerTasks
// OUTPUT: the conflict algebra deciding which tasks may run concurrently
// POS:    Product-level workspace coordination — applies identically to the
//         in-product Alva worker and to external engine workers, because the
//         decision happens before any engine starts.

use alva_kernel_abi::normalize_path_string;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimMode {
    /// Concurrent readers are fine; a writer excludes everyone.
    Read,
    Write,
}

impl ClaimMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClaimMode::Read => "read",
            ClaimMode::Write => "write",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResourceClaim {
    /// A workspace-relative path (file or directory subtree).
    Path { path: String, mode: ClaimMode },
    /// A workspace-wide facility such as the build directory, the package
    /// lockfile install, or the test runner.
    Facility { name: String },
    /// A tool that must not run twice concurrently anywhere in the
    /// workspace. Modelled after the kernel tool lock's exclusive keys;
    /// this is product-level scheduling, not that lock.
    ExclusiveTool { tool: String },
}

impl ResourceClaim {
    /// Reject claims that would silently protect nothing: empty names, and
    /// paths that are absolute or escape the workspace. An advisory system
    /// must never accept a meaningless declaration quietly.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            ResourceClaim::Path { path, .. } => {
                let trimmed = path.trim();
                if trimmed.is_empty() {
                    return Err("a path claim needs a path".to_string());
                }
                if trimmed.starts_with('/') || trimmed.contains('\\') {
                    return Err(format!(
                        "claim '{trimmed}' must be a workspace-relative path with / separators"
                    ));
                }
                let normalized = normalize_path_string(trimmed);
                if normalized == "." || normalized.starts_with("..") {
                    return Err(format!("claim '{trimmed}' escapes the workspace"));
                }
                Ok(())
            }
            ResourceClaim::Facility { name } => (!name.trim().is_empty())
                .then_some(())
                .ok_or_else(|| "a facility claim needs a name".to_string()),
            ResourceClaim::ExclusiveTool { tool } => (!tool.trim().is_empty())
                .then_some(())
                .ok_or_else(|| "an exclusive tool claim needs a tool".to_string()),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            ResourceClaim::Path { path, mode } => match mode {
                ClaimMode::Read => format!("read {path}"),
                ClaimMode::Write => format!("write {path}"),
            },
            ResourceClaim::Facility { name } => format!("facility {name}"),
            ResourceClaim::ExclusiveTool { tool } => format!("tool {tool}"),
        }
    }
}

/// Parse a comma-separated claim list like `src/lib.rs, read:docs,
/// facility:target-dir, tool:git`, matching the React `parseClaims`.
pub fn parse_claims(input: &str) -> Vec<ResourceClaim> {
    input
        .split(',')
        .map(|entry| entry.trim())
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            if let Some(name) = entry.strip_prefix("facility:") {
                ResourceClaim::Facility {
                    name: name.to_string(),
                }
            } else if let Some(tool) = entry.strip_prefix("tool:") {
                ResourceClaim::ExclusiveTool {
                    tool: tool.to_string(),
                }
            } else if let Some(path) = entry.strip_prefix("read:") {
                ResourceClaim::Path {
                    path: path.to_string(),
                    mode: ClaimMode::Read,
                }
            } else {
                ResourceClaim::Path {
                    path: entry.to_string(),
                    mode: ClaimMode::Write,
                }
            }
        })
        .collect()
}

/// Do two individual claims conflict?
pub fn claims_conflict(left: &ResourceClaim, right: &ResourceClaim) -> bool {
    match (left, right) {
        (
            ResourceClaim::Path {
                path: left_path,
                mode: left_mode,
            },
            ResourceClaim::Path {
                path: right_path,
                mode: right_mode,
            },
        ) => {
            if *left_mode == ClaimMode::Read && *right_mode == ClaimMode::Read {
                return false;
            }
            paths_overlap(left_path, right_path)
        }
        (ResourceClaim::Facility { name: left }, ResourceClaim::Facility { name: right }) => {
            left == right
        }
        (
            ResourceClaim::ExclusiveTool { tool: left },
            ResourceClaim::ExclusiveTool { tool: right },
        ) => left == right,
        // Different kinds never contend: a facility is not a path.
        _ => false,
    }
}

/// Every claim in `candidate` that conflicts with something already held.
pub fn conflicting_claims(
    candidate: &[ResourceClaim],
    held: &[ResourceClaim],
) -> Vec<ResourceClaim> {
    let mut conflicts: Vec<ResourceClaim> = Vec::new();
    for wanted in candidate {
        if held.iter().any(|holder| claims_conflict(wanted, holder)) && !conflicts.contains(wanted)
        {
            conflicts.push(wanted.clone());
        }
    }
    conflicts
}

/// A path claim covers its whole subtree, so `src` and `src/main.rs`
/// contend while `src/a.rs` and `src/b.rs` do not. Normalization is the
/// kernel's (collapses `//`, resolves `.`/`..`, keeps absolute distinct),
/// so `src//lib.rs`, `./src/lib.rs`, and `a/../src/lib.rs` are one resource.
fn paths_overlap(left: &str, right: &str) -> bool {
    let left = normalize(left);
    let right = normalize(right);
    left == right
        || left.strip_prefix(&right).is_some_and(starts_at_segment)
        || right.strip_prefix(&left).is_some_and(starts_at_segment)
}

fn normalize(path: &str) -> String {
    normalize_path_string(path.trim().trim_end_matches('/'))
}

fn starts_at_segment(rest: &str) -> bool {
    rest.starts_with('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &str) -> ResourceClaim {
        ResourceClaim::Path {
            path: path.into(),
            mode: ClaimMode::Write,
        }
    }

    fn read(path: &str) -> ResourceClaim {
        ResourceClaim::Path {
            path: path.into(),
            mode: ClaimMode::Read,
        }
    }

    #[test]
    fn parse_claims_splits_mixed_entries() {
        let claims = parse_claims("src/lib.rs, read:docs, facility:target-dir, tool:git");
        assert_eq!(claims.len(), 4);
        assert!(matches!(
            &claims[0],
            ResourceClaim::Path {
                mode: ClaimMode::Write,
                ..
            }
        ));
        assert!(matches!(
            &claims[1],
            ResourceClaim::Path { mode: ClaimMode::Read, path } if path == "docs"
        ));
        assert!(matches!(
            &claims[2],
            ResourceClaim::Facility { name } if name == "target-dir"
        ));
        assert!(matches!(
            &claims[3],
            ResourceClaim::ExclusiveTool { tool } if tool == "git"
        ));
    }

    #[test]
    fn parse_claims_ignores_empty_entries_and_whitespace() {
        let claims = parse_claims(" , src/a.rs,  ,read:x,");
        assert_eq!(claims.len(), 2);
        assert_eq!(claims[0].describe(), "write src/a.rs");
        assert_eq!(claims[1].describe(), "read x");
    }

    #[test]
    fn disjoint_paths_and_shared_reads_run_concurrently() {
        assert!(!claims_conflict(&write("src/a.rs"), &write("src/b.rs")));
        assert!(!claims_conflict(&read("src/a.rs"), &read("src/a.rs")));
        assert!(!claims_conflict(&read("src"), &read("src/deep/file.rs")));
        // Prefix similarity is not containment.
        assert!(!claims_conflict(&write("src2/a.rs"), &write("src/a.rs")));
        assert!(!claims_conflict(&write("srcfoo"), &write("src")));
    }

    #[test]
    fn writers_exclude_overlapping_paths_in_both_directions() {
        assert!(claims_conflict(&write("src/a.rs"), &write("src/a.rs")));
        assert!(claims_conflict(&write("src"), &read("src/a.rs")));
        assert!(claims_conflict(&read("src/a.rs"), &write("src")));
        assert!(claims_conflict(&write("src/a.rs"), &read("src/a.rs")));
        // Trailing slashes and ./ prefixes are the same resource.
        assert!(claims_conflict(&write("src/"), &write("./src")));
    }

    #[test]
    fn facilities_and_exclusive_tools_are_all_or_nothing() {
        let build = ResourceClaim::Facility {
            name: "target-dir".into(),
        };
        let tests = ResourceClaim::Facility {
            name: "test-runner".into(),
        };
        assert!(claims_conflict(&build, &build.clone()));
        assert!(!claims_conflict(&build, &tests));

        let git = ResourceClaim::ExclusiveTool { tool: "git".into() };
        assert!(claims_conflict(&git, &git.clone()));
        assert!(!claims_conflict(&git, &build));
        // A facility named like a path never contends with that path.
        assert!(!claims_conflict(&build, &write("target-dir")));
    }

    #[test]
    fn conflicting_claims_reports_every_blocked_resource_once() {
        let candidate = vec![
            write("src/lib.rs"),
            read("docs/readme.md"),
            ResourceClaim::Facility {
                name: "target-dir".into(),
            },
            write("src/lib.rs"),
        ];
        let held = vec![
            read("src"),
            ResourceClaim::Facility {
                name: "target-dir".into(),
            },
        ];
        let conflicts = conflicting_claims(&candidate, &held);
        assert_eq!(
            conflicts,
            vec![
                write("src/lib.rs"),
                ResourceClaim::Facility {
                    name: "target-dir".into()
                }
            ]
        );
        // Nothing held → nothing blocked.
        assert!(conflicting_claims(&candidate, &[]).is_empty());
    }

    #[test]
    fn messy_but_equivalent_paths_are_one_resource() {
        // Every pair below denotes the same file: a false negative here
        // would let two writers run against it concurrently.
        for (left, right) in [
            ("src//lib.rs", "src/lib.rs"),
            ("./src/lib.rs", "src/lib.rs"),
            ("a/../src/lib.rs", "src/lib.rs"),
            ("src/./lib.rs", "src/lib.rs"),
            ("src/sub/../lib.rs", "src/lib.rs"),
        ] {
            assert!(
                claims_conflict(&write(left), &write(right)),
                "{left:?} and {right:?} should be the same resource"
            );
        }
        // Names containing a dot are not mangled into each other.
        assert!(!claims_conflict(&write("x./y"), &write("xy")));
        assert!(!claims_conflict(&write("v1./cfg"), &write("v1cfg")));
    }

    #[test]
    fn meaningless_or_escaping_claims_are_rejected_loudly() {
        for claim in [
            write(""),
            write("   "),
            write("/"),
            write("."),
            write("/etc/passwd"),
            write("../outside"),
            write("src\\lib.rs"),
            ResourceClaim::Facility { name: "".into() },
            ResourceClaim::ExclusiveTool { tool: " ".into() },
        ] {
            assert!(claim.validate().is_err(), "{claim:?} must be rejected");
        }
        assert!(write("src/lib.rs").validate().is_ok());
        assert!(read("docs").validate().is_ok());
        assert!(ResourceClaim::Facility {
            name: "target-dir".into()
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn claim_json_shape_is_pinned_for_the_typescript_bridge() {
        assert_eq!(
            serde_json::to_value(write("src/a.rs")).unwrap(),
            serde_json::json!({"kind": "path", "path": "src/a.rs", "mode": "write"})
        );
        assert_eq!(
            serde_json::to_value(ResourceClaim::Facility {
                name: "target-dir".into()
            })
            .unwrap(),
            serde_json::json!({"kind": "facility", "name": "target-dir"})
        );
        assert_eq!(
            serde_json::to_value(ResourceClaim::ExclusiveTool { tool: "git".into() }).unwrap(),
            serde_json::json!({"kind": "exclusive_tool", "tool": "git"})
        );
    }
}

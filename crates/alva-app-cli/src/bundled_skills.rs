// INPUT:  std::{fs, path, sync}, include_dir, dirs, alva_protocol_skill
// OUTPUT: ensure_extracted, wasm_environment_skill_injection, BUNDLED_SKILLS_VERSION
// POS:    Host-side bundled skill extraction plus explicit wasm-env parsing/injection.

//! Built-in skill bundling.
//!
//! Skills shipped with the App live at the workspace root under
//! `assets/skills/` — pure static markdown, not a Cargo crate, because
//! that's what they are. Both `alva-app-cli` and `alva-app-tauri` embed
//! the same tree via `include_dir!` reaching up two levels from their
//! respective `CARGO_MANIFEST_DIR`.
//!
//! On first launch (or when the crate version changes) the tree is
//! extracted to `<cache_dir>/alva/bundled-skills-v<VERSION>/`. The path
//! is passed to `SkillsPlugin` so the agent's skill loader picks them
//! up like any other directory tree.
//!
//! Why a versioned cache dir: cleanly invalidates on upgrade — old
//! bundled skills don't shadow new ones — without needing extraction
//! logic to know about every file individually.
//!
//! The wasm worker's environment skill is the one exception to extraction:
//! the host parses its embedded bytes directly because it has no resources,
//! then sends the expanded prompt over the bounded context ABI. This keeps a
//! cache directory out of the sandbox contract and still uses the same asset.

use std::fs;
use std::path::PathBuf;

use include_dir::{include_dir, Dir};

use alva_protocol_skill::injector::SkillInjector;
use alva_protocol_skill::loader::SkillLoader;
use alva_protocol_skill::memory::{InMemorySkill, InMemorySkillRepository};
use alva_protocol_skill::types::{InjectionPolicy, Skill, SkillInvocation, SkillKind, SkillRef};

// Workspace layout: `crates/alva-app-cli/Cargo.toml` → `../../assets/skills`.
const BUNDLED_SKILLS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../assets/skills");

/// Cache-key version. Bumping forces re-extraction. Tied to the crate
/// version so each release gets a fresh bundled-skills directory.
pub const BUNDLED_SKILLS_VERSION: &str = env!("CARGO_PKG_VERSION");

const WASM_ENV_SKILL_NAME: &str = "wasm-env";

/// Parse the host-bundled wasm environment skill and build its full explicit
/// system-prompt injection. The guest never reads or mounts the skill store.
pub(crate) async fn wasm_environment_skill_injection() -> Result<String, String> {
    let relative_path = format!("{WASM_ENV_SKILL_NAME}/SKILL.md");
    let embedded = BUNDLED_SKILLS
        .get_file(&relative_path)
        .ok_or_else(|| format!("bundled skill asset {relative_path:?} is missing"))?;
    let content = embedded
        .contents_utf8()
        .ok_or_else(|| format!("bundled skill asset {relative_path:?} is not UTF-8"))?;
    let meta = alva_protocol_skill::fs::FsSkillRepository::parse_frontmatter(content)
        .map_err(|error| format!("parse bundled {relative_path}: {error}"))?;
    if meta.name != WASM_ENV_SKILL_NAME {
        return Err(format!(
            "{} declares skill name {:?}; expected {WASM_ENV_SKILL_NAME:?}",
            relative_path, meta.name
        ));
    }
    if meta.invocation != SkillInvocation::Explicit {
        return Err(format!(
            "bundled {WASM_ENV_SKILL_NAME} must use invocation: explicit so native skill directories do not advertise it"
        ));
    }

    let body = alva_protocol_skill::fs::FsSkillRepository::parse_body(content);
    let skill = Skill {
        meta: meta.clone(),
        kind: SkillKind::Bundled,
        root_path: PathBuf::from("<bundled>").join(WASM_ENV_SKILL_NAME),
        enabled: true,
    };
    let repo = std::sync::Arc::new(InMemorySkillRepository::new(vec![InMemorySkill {
        meta,
        kind: SkillKind::Bundled,
        body: body.markdown,
        resources: Vec::new(),
        enabled: true,
    }]));
    SkillInjector::new(SkillLoader::new(repo))
        .build_injection(
            &[SkillRef {
                name: WASM_ENV_SKILL_NAME.into(),
                injection: Some(InjectionPolicy::Explicit),
            }],
            &[skill],
        )
        .await
        .map_err(|error| format!("inject bundled {WASM_ENV_SKILL_NAME}: {error}"))
}

/// Extract the bundled skills tree to a versioned cache directory and
/// return the path. Idempotent: if the cache directory already exists
/// AND contains every bundled file, returns the path without rewriting.
pub fn ensure_extracted() -> std::io::Result<PathBuf> {
    let dest = bundled_skills_cache_dir();
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    if dest.exists() && is_complete(&dest) {
        return Ok(dest);
    }
    fs::create_dir_all(&dest)?;
    BUNDLED_SKILLS.extract(&dest)?;
    Ok(dest)
}

/// Resolved cache directory:
/// - `$XDG_CACHE_HOME/alva/bundled-skills-v<X>/`
/// - else `dirs::cache_dir()/alva/bundled-skills-v<X>/`
/// - else `~/.cache/alva/bundled-skills-v<X>/`
pub fn bundled_skills_cache_dir() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".cache")
        });
    base.join("alva")
        .join(format!("bundled-skills-v{}", BUNDLED_SKILLS_VERSION))
}

/// Walk the embedded tree and confirm every entry exists on disk under
/// `dest`. Cheap (no content compare) — we trust the version-stamped cache
/// dir name to invalidate correctly across releases.
fn is_complete(dest: &std::path::Path) -> bool {
    fn walk(dir: &Dir<'_>, dest: &std::path::Path) -> bool {
        for f in dir.files() {
            if !dest.join(f.path()).is_file() {
                return false;
            }
        }
        for d in dir.dirs() {
            if !walk(d, dest) {
                return false;
            }
        }
        true
    }
    walk(&BUNDLED_SKILLS, dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Single test exercising:
    /// 1. `ensure_extracted` writes the bundled tree on first call
    /// 2. Second call is idempotent (no rewrite)
    /// 3. `SkillsPlugin::with_bundled` actually finds the extracted skills
    ///    end-to-end (proving the install→read pipeline works)
    ///
    /// All three checks share a single `XDG_CACHE_HOME` env var because Rust
    /// runs unit tests in parallel by default and that env var is process-global.
    #[tokio::test(flavor = "multi_thread")]
    async fn extracts_idempotent_and_skill_is_discoverable() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CACHE_HOME", tmp.path());

        // 1) Extraction lands at the resolved cache dir under our temp root.
        let first = ensure_extracted().expect("first extract ok");
        let dest = bundled_skills_cache_dir();
        assert_eq!(first, dest);
        assert!(first.starts_with(tmp.path()));
        let skill_md = first.join("project-tooling").join("SKILL.md");
        assert!(
            skill_md.is_file(),
            "expected {} to exist",
            skill_md.display()
        );
        let body = fs::read_to_string(&skill_md).unwrap();
        assert!(body.contains("name: project-tooling"));

        // 2) Second call is a no-op (file mtime unchanged).
        let mtime_before = fs::metadata(&skill_md).unwrap().modified().unwrap();
        let second = ensure_extracted().expect("second extract ok");
        assert_eq!(first, second);
        let mtime_after = fs::metadata(&skill_md).unwrap().modified().unwrap();
        assert_eq!(mtime_before, mtime_after, "second call must not rewrite");

        // 3) End-to-end: SkillsPlugin::with_bundled wires the extracted dir
        //    as the bundled source, the underlying FsSkillRepository scans it,
        //    and the project-tooling skill is in the listed skills.
        let primary = tmp.path().join("project-skills");
        std::fs::create_dir_all(primary.join("user")).unwrap();
        std::fs::create_dir_all(primary.join("mbb")).unwrap();
        let repo = alva_protocol_skill::fs::FsSkillRepository::new(
            first.clone(),
            primary.join("mbb"),
            primary.join("user"),
            primary.join("state.json"),
        );
        use alva_app_core::extension::skills::skill_ports::skill_repository::SkillRepository;
        let skills = repo.list_skills().await.expect("list_skills ok");
        let names: Vec<&str> = skills.iter().map(|s| s.meta.name.as_str()).collect();
        for required in [
            "project-tooling",
            "autonomous-ui-test",
            "autonomous-ui-repair",
            WASM_ENV_SKILL_NAME,
        ] {
            assert!(
                names.contains(&required),
                "bundled skill {required:?} must be discoverable; got: {names:?}"
            );
        }

        let wasm_env = skills
            .iter()
            .find(|skill| skill.meta.name == WASM_ENV_SKILL_NAME)
            .expect("wasm-env bundled skill");
        assert_eq!(wasm_env.meta.invocation, SkillInvocation::Explicit);

        let injection = wasm_environment_skill_injection().await.unwrap();
        for required in [
            "# WASIp1 Worker Environment",
            "`/workspace`",
            "`/grants/<index>`",
            "```tool-names",
            "synchronous `fetch` binding",
            "There is no shell",
            "`request_escalation`",
            "bulk file work",
            "final assistant response",
            "result channel",
        ] {
            assert!(
                injection.contains(required),
                "wasm environment injection must cover {required:?}: {injection}"
            );
        }

        // A normal/native agent sees only auto-invoked skills in its stable
        // directory. The worker-only explicit skill must not leak into it.
        let primary = tmp.path().join("native-primary");
        let model = alva_test::mock_provider::MockLanguageModel::new();
        let agent = alva_app_core::BaseAgent::builder()
            .workspace(tmp.path())
            .system_prompt("native agent")
            .plugin(Box::new(
                alva_app_core::extension::SkillsPlugin::with_bundled(primary, Some(first)),
            ))
            .build(std::sync::Arc::new(model))
            .await
            .unwrap();
        let native_prompt = agent.system_prompt_segments().await.join("\n");
        assert!(
            !native_prompt.contains("## Skill: wasm-env"),
            "{native_prompt}"
        );
        assert!(
            !native_prompt.contains("# WASIp1 Worker Environment"),
            "{native_prompt}"
        );
    }
}

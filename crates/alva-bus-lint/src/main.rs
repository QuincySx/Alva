// INPUT:  syn (parse .rs), walkdir (traverse crates/)
// OUTPUT: exit 0 on clean, 1 with diagnostics on violation
// POS:    CI lint — enforces cross-crate type surface <= 2 on #[bus_cap] traits.
//
// Rationale: a Cap trait that drags types from many crates into its
// signature becomes a God interface and forces every consumer to pull
// half the workspace. Capping the external-crate surface at 2 keeps
// Cap traits lean by construction, without having to police every
// `.provide` / `.get` callsite. See docs/BUS-RULES.md.
//
// Only traits are surface-checked. Structs/enums/events tagged with
// `#[bus_cap]` / `#[bus_event]` are counted for reporting but have no
// mechanical rule — struct APIs grow organically and the God-interface
// smell is trait-shaped.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use syn::{
    FnArg, GenericArgument, Item, ItemTrait, PathArguments, ReturnType, TraitItem, Type,
    TypeParamBound, UseTree,
};

const MAX_EXTERNAL: usize = 2;

const STD_ROOTS: &[&str] = &["std", "core", "alloc"];
const TRAIT_BOUND_NOISE: &[&str] = &[
    "Send", "Sync", "Sized", "Fn", "FnMut", "FnOnce", "Copy", "Clone",
];

fn main() {
    let root = find_workspace_root().expect("alva-bus-lint: no workspace root found");
    let crates_dir = root.join("crates");

    let mut traits_ok = 0usize;
    let mut structs_or_enums = 0usize;
    let mut events = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for rs in walk_rs(&crates_dir) {
        let self_crate = find_self_crate(&rs).unwrap_or_default();
        let Ok(src) = fs::read_to_string(&rs) else {
            continue;
        };
        let Ok(file) = syn::parse_file(&src) else {
            continue;
        };

        let use_map = collect_uses(&file.items);

        for item in &file.items {
            match item {
                Item::Trait(t) if has_marker(&t.attrs, "bus_cap") => {
                    let surface = collect_trait_surface(t, &use_map, &self_crate);
                    if surface.len() > MAX_EXTERNAL {
                        errors.push(format!(
                            "{}: trait `{}` has {} external crates in its signature (limit {}): {}",
                            display_rel(&rs, &root),
                            t.ident,
                            surface.len(),
                            MAX_EXTERNAL,
                            surface.iter().cloned().collect::<Vec<_>>().join(", ")
                        ));
                    } else {
                        traits_ok += 1;
                    }
                }
                Item::Struct(s) if has_marker(&s.attrs, "bus_cap") => structs_or_enums += 1,
                Item::Enum(e) if has_marker(&e.attrs, "bus_cap") => structs_or_enums += 1,
                Item::Type(ty) if has_marker(&ty.attrs, "bus_cap") => structs_or_enums += 1,
                Item::Struct(s) if has_marker(&s.attrs, "bus_event") => events += 1,
                Item::Enum(e) if has_marker(&e.attrs, "bus_event") => events += 1,
                _ => {}
            }
        }
    }

    if errors.is_empty() {
        println!(
            "alva-bus-lint: OK — {} Cap trait(s), {} Cap struct/enum(s), {} Event(s) — max external surface {}",
            traits_ok, structs_or_enums, events, MAX_EXTERNAL
        );
    } else {
        for e in &errors {
            eprintln!("VIOLATION: {}", e);
        }
        eprintln!("alva-bus-lint: FAILED — {} violation(s)", errors.len());
        std::process::exit(1);
    }
}

// ---------- workspace discovery ----------

fn find_workspace_root() -> Option<PathBuf> {
    let start = std::env::current_dir().ok()?;
    for dir in start.ancestors() {
        let cargo = dir.join("Cargo.toml");
        if cargo.exists() {
            if let Ok(s) = fs::read_to_string(&cargo) {
                if s.contains("[workspace]") {
                    return Some(dir.to_path_buf());
                }
            }
        }
    }
    None
}

fn find_self_crate(rs_file: &Path) -> Option<String> {
    for dir in rs_file.ancestors() {
        let cargo = dir.join("Cargo.toml");
        if cargo.exists() {
            if let Ok(s) = fs::read_to_string(&cargo) {
                if let Some(name) = parse_package_name(&s) {
                    return Some(name.replace('-', "_"));
                }
            }
        }
    }
    None
}

fn parse_package_name(toml_src: &str) -> Option<String> {
    let mut in_package = false;
    for line in toml_src.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_package = t == "[package]";
            continue;
        }
        if in_package {
            if let Some(rest) = t.strip_prefix("name") {
                let rest = rest.trim_start_matches([' ', '=']).trim();
                return rest
                    .strip_prefix('"')
                    .and_then(|r| r.split_once('"').map(|(n, _)| n.to_string()));
            }
        }
    }
    None
}

fn walk_rs(root: &Path) -> impl Iterator<Item = PathBuf> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| e.file_name() != "target")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().map(|x| x == "rs").unwrap_or(false))
        .map(|e| e.into_path())
}

fn display_rel(p: &Path, root: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).display().to_string()
}

// ---------- attribute detection ----------

fn has_marker(attrs: &[syn::Attribute], name: &str) -> bool {
    attrs.iter().any(|a| {
        a.path()
            .segments
            .last()
            .map(|s| s.ident == name)
            .unwrap_or(false)
    })
}

// ---------- use-map ----------

fn collect_uses(items: &[Item]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for item in items {
        if let Item::Use(u) = item {
            walk_use(&u.tree, None, &mut out);
        }
    }
    out
}

fn walk_use(tree: &UseTree, crate_of: Option<String>, out: &mut BTreeMap<String, String>) {
    match tree {
        UseTree::Path(p) => {
            let cn = crate_of.unwrap_or_else(|| p.ident.to_string());
            walk_use(&p.tree, Some(cn), out);
        }
        UseTree::Name(n) => {
            if let Some(cn) = crate_of {
                out.insert(n.ident.to_string(), cn);
            }
        }
        UseTree::Rename(r) => {
            if let Some(cn) = crate_of {
                out.insert(r.rename.to_string(), cn);
            }
        }
        UseTree::Group(g) => {
            for it in &g.items {
                walk_use(it, crate_of.clone(), out);
            }
        }
        UseTree::Glob(_) => {}
    }
}

// ---------- surface collection ----------

fn collect_trait_surface(
    t: &ItemTrait,
    use_map: &BTreeMap<String, String>,
    self_crate: &str,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for item in &t.items {
        if let TraitItem::Fn(f) = item {
            for arg in &f.sig.inputs {
                if let FnArg::Typed(pt) = arg {
                    visit_type(&pt.ty, use_map, self_crate, &mut out);
                }
            }
            if let ReturnType::Type(_, ty) = &f.sig.output {
                visit_type(ty, use_map, self_crate, &mut out);
            }
        }
    }
    out
}

fn visit_type(
    ty: &Type,
    use_map: &BTreeMap<String, String>,
    self_crate: &str,
    out: &mut BTreeSet<String>,
) {
    match ty {
        Type::Path(tp) => {
            if let Some(first) = tp.path.segments.first() {
                let first_s = first.ident.to_string();
                let last_s = tp.path.segments.last().unwrap().ident.to_string();
                let resolved = if tp.path.segments.len() > 1 {
                    // Fully qualified path
                    match first_s.as_str() {
                        "crate" | "self" | "super" | "Self" => None,
                        _ => Some(first_s.clone()),
                    }
                } else {
                    // Unqualified — look in the file's use-map
                    use_map.get(&last_s).cloned()
                };
                if let Some(c) = resolved {
                    record_crate(c, self_crate, out);
                }
            }
            // Recurse into generic args on every segment
            for seg in &tp.path.segments {
                if let PathArguments::AngleBracketed(ab) = &seg.arguments {
                    for arg in &ab.args {
                        if let GenericArgument::Type(inner) = arg {
                            visit_type(inner, use_map, self_crate, out);
                        }
                    }
                }
            }
        }
        Type::Reference(r) => visit_type(&r.elem, use_map, self_crate, out),
        Type::Slice(s) => visit_type(&s.elem, use_map, self_crate, out),
        Type::Array(a) => visit_type(&a.elem, use_map, self_crate, out),
        Type::Tuple(t) => {
            for el in &t.elems {
                visit_type(el, use_map, self_crate, out);
            }
        }
        Type::TraitObject(to) => {
            for bound in &to.bounds {
                if let TypeParamBound::Trait(tr) = bound {
                    if let Some(first) = tr.path.segments.first() {
                        let name = first.ident.to_string();
                        if TRAIT_BOUND_NOISE.contains(&name.as_str()) {
                            continue;
                        }
                        let last = tr.path.segments.last().unwrap().ident.to_string();
                        let resolved = if tr.path.segments.len() > 1 {
                            match name.as_str() {
                                "crate" | "self" | "super" | "Self" => None,
                                _ => Some(name.clone()),
                            }
                        } else {
                            use_map.get(&last).cloned()
                        };
                        if let Some(c) = resolved {
                            record_crate(c, self_crate, out);
                        }
                    }
                }
            }
        }
        Type::ImplTrait(it) => {
            for bound in &it.bounds {
                if let TypeParamBound::Trait(tr) = bound {
                    if let Some(first) = tr.path.segments.first() {
                        let name = first.ident.to_string();
                        if TRAIT_BOUND_NOISE.contains(&name.as_str()) {
                            continue;
                        }
                        let last = tr.path.segments.last().unwrap().ident.to_string();
                        let resolved = if tr.path.segments.len() > 1 {
                            Some(name.clone())
                        } else {
                            use_map.get(&last).cloned()
                        };
                        if let Some(c) = resolved {
                            record_crate(c, self_crate, out);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn record_crate(c: String, self_crate: &str, out: &mut BTreeSet<String>) {
    let norm = c.replace('-', "_");
    if STD_ROOTS.contains(&norm.as_str()) {
        return;
    }
    if !self_crate.is_empty() && norm == self_crate {
        return;
    }
    out.insert(norm);
}

#[cfg(test)]
mod tests {
    //! Pure-helper tests for the lint internals. The end-to-end main()
    //! pass needs a real workspace tree; that's covered by running the
    //! binary in CI rather than from these unit tests.

    use super::*;

    // -- parse_package_name -------------------------------------------------

    #[test]
    fn parse_package_name_happy() {
        let toml = r#"[package]
name = "alva-bus-lint"
version = "0.1.0"
"#;
        assert_eq!(parse_package_name(toml), Some("alva-bus-lint".to_string()));
    }

    #[test]
    fn parse_package_name_ignores_name_outside_package_section() {
        // A `name = "..."` line under [[bin]] or [dependencies] must not
        // be mistaken for the package name.
        let toml = r#"[package]
version = "0.1.0"

[[bin]]
name = "alva-bus-lint"
path = "src/main.rs"
"#;
        assert_eq!(
            parse_package_name(toml),
            None,
            "must only read [package].name"
        );
    }

    #[test]
    fn parse_package_name_handles_extra_whitespace() {
        let toml = r#"[package]
name      =     "spaced-name"
"#;
        assert_eq!(parse_package_name(toml), Some("spaced-name".to_string()));
    }

    // -- record_crate -------------------------------------------------------

    #[test]
    fn record_crate_normalizes_dashes_and_filters_std() {
        let mut out = BTreeSet::new();
        // std/core/alloc are always dropped
        record_crate("std".into(), "", &mut out);
        record_crate("core".into(), "", &mut out);
        record_crate("alloc".into(), "", &mut out);
        // dashes normalized to underscores when stored
        record_crate("alva-kernel-abi".into(), "", &mut out);
        record_crate("alva-agent-core".into(), "", &mut out);
        assert_eq!(
            out.iter().cloned().collect::<Vec<_>>(),
            vec!["alva_agent_core".to_string(), "alva_kernel_abi".to_string()]
        );
    }

    #[test]
    fn record_crate_filters_self_crate() {
        let mut out = BTreeSet::new();
        // self_crate is already in the normalized (underscore) form.
        record_crate("alva-kernel-abi".into(), "alva_kernel_abi", &mut out);
        record_crate("alva-agent-core".into(), "alva_kernel_abi", &mut out);
        assert!(
            out.iter().all(|c| c != "alva_kernel_abi"),
            "self crate must not appear in surface set"
        );
        assert!(out.contains("alva_agent_core"));
    }

    // -- has_marker (drives the whole lint trigger) -------------------------

    #[test]
    fn has_marker_matches_path_last_segment() {
        // Trait with `#[bus_cap]` — the marker must be detected
        let file: syn::File = syn::parse_str("#[bus_cap]\ntrait Foo {}").unwrap();
        let attrs = match &file.items[0] {
            syn::Item::Trait(t) => &t.attrs,
            _ => panic!("expected Item::Trait"),
        };
        assert!(has_marker(attrs, "bus_cap"));
        assert!(
            !has_marker(attrs, "bus_event"),
            "non-matching name should be false"
        );
    }

    #[test]
    fn has_marker_matches_fully_qualified_marker_path() {
        // Some crates apply the attr fully-qualified: `#[alva_macros::bus_cap]`.
        // `has_marker` checks the LAST path segment so this should still trip.
        let file: syn::File = syn::parse_str("#[alva_macros::bus_cap]\ntrait Foo {}").unwrap();
        let attrs = match &file.items[0] {
            syn::Item::Trait(t) => &t.attrs,
            _ => panic!("expected Item::Trait"),
        };
        assert!(
            has_marker(attrs, "bus_cap"),
            "must match on fully-qualified path"
        );
    }
}

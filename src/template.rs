//! Templates for `btm-mod new <kind> <dir>`.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use semver::Version;

use crate::digest::Digest;
use crate::fs::MANIFEST_FILE;
use crate::manifest::{Kind, Manifest, Module, OptionSpec, Presentation};
use crate::names;
use crate::range::Range;

/// The engine range a new mod starts with. Narrow it to the versions you test against.
pub const TEMPLATE_ENGINE_RANGE: &str = "*";

/// Why a template could not be written.
#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("{0:?} is not a mod id; pass a valid one with --id (for example weather-storms)")]
    BadId(String),
    #[error("{} already exists and is not empty", .0.display())]
    NotEmpty(PathBuf),
    #[error("{}: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// The default id for a directory: its name, lowercased, with runs of other characters
/// turned into single hyphens.
pub fn default_id(dir: &Path) -> String {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

/// The Rust crate name for a mod id: `weather-storms` to `weather_storms`.
pub fn crate_name(id: &str) -> String {
    id.replace(['-', '.'], "_")
}

/// Writes a template for `kind` into `dir` (created if missing; it must be empty) and
/// returns the files written.
pub fn create(kind: Kind, dir: &Path, id: &str) -> Result<Vec<PathBuf>, TemplateError> {
    if !names::is_mod_id(id) {
        return Err(TemplateError::BadId(id.to_owned()));
    }
    if dir.exists() {
        let mut entries = std::fs::read_dir(dir).map_err(|source| TemplateError::Io {
            path: dir.to_owned(),
            source,
        })?;
        if entries.next().is_some() {
            return Err(TemplateError::NotEmpty(dir.to_owned()));
        }
    }
    let crate_name = crate_name(id);
    let wasm = format!("{crate_name}.wasm");
    let mut files: Vec<(String, String)> = Vec::new();
    let mut m = Manifest {
        id: id.to_owned(),
        version: Version::new(0, 1, 0),
        kind: BTreeSet::from([kind]),
        engine: Range::parse(TEMPLATE_ENGINE_RANGE).map_err(|e| TemplateError::Io {
            path: dir.to_owned(),
            source: io::Error::other(e),
        })?,
        requires: BTreeMap::new(),
        after: Vec::new(),
        before: Vec::new(),
        conflicts: Vec::new(),
        content: Vec::new(),
        profile: None,
        modules: Vec::new(),
        presentation: Vec::new(),
        options: BTreeMap::new(),
    };
    let placeholder = Digest([0; 32]);
    match kind {
        Kind::Content => {
            m.content = vec!["data/*.hpi".into()];
            files.push((
                "data/README.txt".into(),
                "Put the mod's archives (.hpi, .ufo, .ccx, .gp3) here.\n".into(),
            ));
        }
        Kind::Profile => {
            m.profile = Some("oamod.yaml".into());
            files.push(("oamod.yaml".into(), PROFILE.into()));
        }
        Kind::Script => {
            files.push(("script.lua".into(), lua_script(id)));
            m.modules.push(Module {
                file: "script.lua".into(),
                sha256: Digest::of(lua_script(id).as_bytes()),
                implements: None,
                wraps: Vec::new(),
                phase: None,
                state_bytes: None,
                publish: Vec::new(),
                subscribe: Vec::new(),
            });
        }
        Kind::Module => {
            files.extend(rust_crate(id, &crate_name, "module"));
            m.modules.push(Module {
                file: wasm,
                sha256: placeholder,
                implements: None,
                wraps: Vec::new(),
                phase: None,
                state_bytes: Some(4096),
                publish: Vec::new(),
                subscribe: Vec::new(),
            });
            m.options.insert(
                "strength".into(),
                OptionSpec::Int {
                    min: Some(0),
                    max: Some(10),
                    default: 5,
                },
            );
        }
        Kind::Presentation => {
            files.extend(rust_crate(id, &crate_name, "presentation"));
            m.presentation.push(Presentation {
                file: wasm,
                sha256: Some(placeholder),
                handles: Vec::new(),
            });
        }
        Kind::Ruleset => {
            let req = Range::parse("^1").map_err(|e| TemplateError::Io {
                path: dir.to_owned(),
                source: io::Error::other(e),
            })?;
            m.requires.insert("community-patch".into(), req);
        }
    }
    files.insert(0, (MANIFEST_FILE.into(), m.to_json()));

    let mut written = Vec::new();
    for (rel, text) in files {
        let path = dir.join(&rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| TemplateError::Io {
                path: parent.to_owned(),
                source,
            })?;
        }
        std::fs::write(&path, text).map_err(|source| TemplateError::Io {
            path: path.clone(),
            source,
        })?;
        written.push(path);
    }
    Ok(written)
}

const PROFILE: &str = "\
# Byte Tactics profile (L1), Open Annihilation's oamod.yaml format.
# Resolved in the order baseline, default, preset, profile.
# Limits, constants and quirk flags go here (milestone M3.1).
";

fn lua_script(id: &str) -> String {
    format!(
        "\
-- {id}: a Byte Tactics synced script (L2).
-- Subscribe to events in btm_mod.json (modules[0].events.subscribe), then run
-- `btm-mod check --update-hashes .` after every edit to refresh the sha256.

local M = {{}}

function M.on_event(kind, ...)
end

return M
"
    )
}

fn rust_crate(id: &str, crate_name: &str, what: &str) -> Vec<(String, String)> {
    let cargo = format!(
        "\
[package]
name = \"{crate_name}\"
version = \"0.1.0\"
edition = \"2021\"
publish = false

[lib]
crate-type = [\"cdylib\"]

[profile.release]
opt-level = \"s\"
lto = true
codegen-units = 1
panic = \"abort\"

# A standalone crate, not part of any surrounding workspace.
[workspace]
"
    );
    let lib = format!(
        "\
//! {id}: a Byte Tactics {what} mod.
//!
//! Build it and refresh the manifest's hash:
//!
//! ```text
//! cargo build --release --target wasm32-unknown-unknown
//! cp target/wasm32-unknown-unknown/release/{crate_name}.wasm .
//! btm-mod check --update-hashes .
//! ```
//!
//! The imports and exports a module must have are set by the engine's module ABI
//! (`docs/module-abi.md` in Byte Tactics). This skeleton exports only an init function.

/// Called once when the module is loaded. Returns 0 on success.
#[no_mangle]
pub extern \"C\" fn btm_init() -> i32 {{
    0
}}
"
    );
    vec![
        ("Cargo.toml".into(), cargo),
        ("src/lib.rs".into(), lib),
        (".gitignore".into(), "/target/\n".into()),
    ]
}

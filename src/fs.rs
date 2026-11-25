//! Reading mods from disk: the manifest, and the SHA-256 of every file it references.
//! This is the only part of the library that does IO; the resolver itself is pure.

use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use crate::digest::Digest;
use crate::manifest::{Manifest, ManifestError};
use crate::resolve::{glob_options, ModPackage};

/// The manifest's file name in a mod directory.
pub const MANIFEST_FILE: &str = "btm_mod.json";

/// Why a mod directory could not be read.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("{}: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{}: {source}", .path.display())]
    Manifest {
        path: PathBuf,
        #[source]
        source: Box<ManifestError>,
    },
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> LoadError + '_ {
    move |source| LoadError::Io {
        path: path.to_owned(),
        source,
    }
}

/// Reads and validates `<dir>/btm_mod.json`.
pub fn read_manifest(dir: &Path) -> Result<Manifest, LoadError> {
    let path = dir.join(MANIFEST_FILE);
    let text = std::fs::read_to_string(&path).map_err(io_err(&path))?;
    Manifest::parse(&text).map_err(|e| LoadError::Manifest {
        path,
        source: Box::new(e),
    })
}

/// SHA-256 of a file's contents.
pub fn hash_file(path: &Path) -> Result<Digest, LoadError> {
    let f = File::open(path).map_err(io_err(path))?;
    Digest::of_reader(io::BufReader::new(f)).map_err(io_err(path))
}

/// Reads a mod directory: the manifest, plus the digest of every file it references
/// (module, presentation and profile files, and every file the content globs match).
/// Referenced files that do not exist are left out; [`crate::resolve::check_files`]
/// reports them.
pub fn load_package(dir: &Path) -> Result<ModPackage, LoadError> {
    let manifest = read_manifest(dir)?;
    let mut files = BTreeMap::new();
    let explicit = manifest
        .modules
        .iter()
        .map(|m| &m.file)
        .chain(manifest.presentation.iter().map(|p| &p.file))
        .chain(manifest.profile.iter());
    for rel in explicit {
        let path = dir.join(rel);
        if path.is_file() {
            files.insert(rel.clone(), hash_file(&path)?);
        }
    }
    let base = glob::Pattern::escape(&dir.to_string_lossy());
    for pattern in &manifest.content {
        let full = format!("{base}/{pattern}");
        let paths = glob::glob_with(&full, glob_options()).map_err(|e| LoadError::Io {
            path: dir.to_owned(),
            source: io::Error::new(io::ErrorKind::InvalidInput, e.msg),
        })?;
        for entry in paths {
            let path = entry.map_err(|e| LoadError::Io {
                path: e.path().to_owned(),
                source: io::Error::other(e.to_string()),
            })?;
            if !path.is_file() {
                continue;
            }
            if let Some(rel) = relative(dir, &path) {
                files.insert(rel, hash_file(&path)?);
            }
        }
    }
    Ok(ModPackage { manifest, files })
}

/// `path` relative to `dir` with `/` separators, if it is valid UTF-8 and inside `dir`.
fn relative(dir: &Path, path: &Path) -> Option<String> {
    let rest = path.strip_prefix(dir).ok()?;
    let parts: Option<Vec<&str>> = rest.components().map(|c| c.as_os_str().to_str()).collect();
    Some(parts?.join("/"))
}

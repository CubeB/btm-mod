//! The resolver (modding plan, section 2.2): from a set of mods, the engine's version,
//! interfaces and phases, and the chosen option values, to the resolved mod set and its
//! hash. Pure and deterministic: the result depends only on the inputs' contents, never
//! on the order they are given in. The first problem found is returned; checks run in a
//! fixed order, mods by id, so the same bad input always gives the same error.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use semver::Version;

use crate::canon;
use crate::digest::Digest;
use crate::manifest::{InterfaceRef, Manifest, OptionSpec, SlotRef, MAX_STATE_BYTES};
use crate::names;

/// Alignment of each module's state region.
pub const STATE_ALIGN: u64 = 64;

/// One mod as found on disk: its manifest and the SHA-256 of each file it references,
/// keyed by the path relative to the mod directory (with `/` separators). Files not
/// referenced by the manifest are ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModPackage {
    pub manifest: Manifest,
    pub files: BTreeMap<String, Digest>,
}

/// What the engine provides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Environment {
    /// The engine version that `engine` ranges are checked against.
    pub engine: Version,
    /// Built-in interfaces (`wasm/modules.json`) with their versions.
    pub interfaces: BTreeMap<String, Version>,
    /// Built-in tick phases, in tick order.
    pub phases: Vec<String>,
}

/// A chosen option value.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OptionValue {
    Int(i64),
    Bool(bool),
    Enum(String),
}

impl fmt::Display for OptionValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OptionValue::Int(i) => write!(f, "{i}"),
            OptionValue::Bool(b) => write!(f, "{b}"),
            OptionValue::Enum(s) => f.write_str(s),
        }
    }
}

/// Chosen option values: mod id, then option name. Anything left out takes its default.
pub type OptionValues = BTreeMap<String, BTreeMap<String, OptionValue>>;

/// What a file is for. The discriminant is its byte in the canonical encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FileRole {
    Content = 0,
    Profile = 1,
    Module = 2,
    /// Never hashed: presentation is optional per player.
    Presentation = 3,
}

impl FileRole {
    pub fn name(self) -> &'static str {
        match self {
            FileRole::Content => "content",
            FileRole::Profile => "profile",
            FileRole::Module => "module",
            FileRole::Presentation => "presentation",
        }
    }
}

/// One file of a resolved mod.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResolvedFile {
    pub role: FileRole,
    pub path: String,
    pub sha256: Digest,
}

/// An event reference after resolution: the full name and, for a mod event kind, its
/// number. `None` means a built-in kind, numbered by the engine.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResolvedEvent {
    pub name: String,
    pub number: Option<u32>,
}

/// One module after resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedModule {
    pub file: String,
    pub sha256: Digest,
    pub implements: Option<InterfaceRef>,
    /// Sorted.
    pub wraps: Vec<SlotRef>,
    /// The module's own phase id (`<mod id>/<file>`), when it declares one.
    pub phase: Option<String>,
    /// Zero when the module has no state region.
    pub state_bytes: u64,
    /// Full names of the kinds it publishes, in declaration order.
    pub publish: Vec<String>,
    /// Sorted by name.
    pub subscribe: Vec<ResolvedEvent>,
}

/// One presentation file after resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedPresentation {
    pub file: String,
    pub sha256: Digest,
    pub handles: Vec<ResolvedEvent>,
}

/// One mod of the resolved set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedMod {
    pub id: String,
    pub version: Version,
    /// False for a pure presentation mod, which is never hashed.
    pub synced: bool,
    /// Every file, sorted by role then path.
    pub files: Vec<ResolvedFile>,
    pub modules: Vec<ResolvedModule>,
    pub presentation: Vec<ResolvedPresentation>,
    /// Every option with its value (chosen or default), sorted by name.
    pub options: Vec<(String, OptionValue)>,
    /// SHA-256 of this mod's synced record (see `docs/resolved-set.md`).
    pub digest: Digest,
}

/// A mod event kind, numbered at resolve (open decision 2 of section 2.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventKind {
    /// 0, 1, 2, ... in resolved mod order, then module order, then declaration order.
    /// The engine maps these onto its range for open event kinds.
    pub number: u32,
    /// `namespace.Name`.
    pub name: String,
    /// The mod that publishes it.
    pub publisher: String,
}

/// A module's region of the mod-state area.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateRegion {
    pub mod_id: String,
    pub file: String,
    /// Offset from the start of the mod-state area, a multiple of [`STATE_ALIGN`].
    pub offset: u64,
    pub len: u64,
}

/// The resolved mod set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSet {
    /// Synced mods in resolved order, then pure presentation mods in their order.
    pub mods: Vec<ResolvedMod>,
    /// Every tick phase, built-in and mod, in tick order.
    pub phases: Vec<String>,
    pub events: Vec<EventKind>,
    pub state: Vec<StateRegion>,
    /// The resolved mod-set hash: SHA-256 of the canonical encoding.
    pub hash: Digest,
}

impl ResolvedSet {
    /// The synced mods, in resolved order.
    pub fn synced(&self) -> impl Iterator<Item = &ResolvedMod> {
        self.mods.iter().filter(|m| m.synced)
    }

    /// The canonical encoding the hash is taken over.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        canon::set_bytes(self)
    }
}

/// Why a mod set does not resolve.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    #[error("invalid environment: {0}")]
    InvalidEnvironment(String),
    #[error("mod {0} is given more than once")]
    DuplicateMod(String),
    #[error("{id} needs engine {range}, but the engine is {engine}")]
    EngineMismatch {
        id: String,
        range: String,
        engine: String,
    },
    #[error("{id} implements {interface}, but the engine's {name} is version {available} (the major versions must match)")]
    InterfaceMajor {
        id: String,
        interface: String,
        name: String,
        available: String,
    },
    #[error("{interface} is implemented by both {first} and {second}")]
    DuplicateImplementation {
        interface: String,
        first: String,
        second: String,
    },
    #[error("{id} requires {name} {range}, which is neither an available interface nor a mod in the set")]
    MissingRequirement {
        id: String,
        name: String,
        range: String,
    },
    #[error("{id} requires {what} {name} {range}, but version {found} is present")]
    VersionMismatch {
        id: String,
        what: &'static str,
        name: String,
        range: String,
        found: String,
    },
    #[error("{id} requires {name}, which is both an interface and a mod; rename one of them")]
    AmbiguousRequirement { id: String, name: String },
    #[error("{id} is synced but requires {name}, a presentation-only mod that other players may not have")]
    SyncedRequiresPresentation { id: String, name: String },
    #[error("{id} conflicts with {other}, and both are in the set")]
    Conflict { id: String, other: String },
    #[error("{id}: {file} wraps {slot}, but no interface {interface} is available")]
    UnknownInterface {
        id: String,
        file: String,
        slot: String,
        interface: String,
    },
    #[error("ordering cycle: {}", .0.join(" -> "))]
    Cycle(Vec<String>),
    #[error("{id}: {file} is missing from the mod directory")]
    MissingFile { id: String, file: String },
    #[error("{id}: {file} has SHA-256 {actual}, but the manifest says {declared}")]
    HashMismatch {
        id: String,
        file: String,
        declared: String,
        actual: String,
    },
    #[error("{id}: content glob {glob} matches no files")]
    EmptyGlob { id: String, glob: String },
    #[error("option values are given for {0}, which is not in the set")]
    OptionForUnknownMod(String),
    #[error("{id} has no option {option}")]
    UnknownOption { id: String, option: String },
    #[error("{id}.{option} is {expected}, but the value given is {given}")]
    OptionType {
        id: String,
        option: String,
        expected: &'static str,
        given: String,
    },
    #[error("{id}.{option} = {value} is outside {min}..={max}")]
    OptionOutOfRange {
        id: String,
        option: String,
        value: i64,
        min: i64,
        max: i64,
    },
    #[error("{id}.{option} = {value} is not one of: {values}")]
    OptionNotInEnum {
        id: String,
        option: String,
        value: String,
        values: String,
    },
    #[error("{id}: {file} places its phase relative to {phase}, which is not a built-in phase")]
    UnknownPhase {
        id: String,
        file: String,
        phase: String,
    },
    #[error(
        "{id}: {file} asks for a phase after {after} and before {before}, but {after} runs later"
    )]
    PhaseContradiction {
        id: String,
        file: String,
        after: String,
        before: String,
    },
    #[error("event kind {name} is published by both {first} and {second}")]
    DuplicateEvent {
        name: String,
        first: String,
        second: String,
    },
    #[error("{id}: {file} refers to event {event}, which no mod in the set publishes and no built-in interface owns")]
    UnknownEvent {
        id: String,
        file: String,
        event: String,
    },
    #[error("{id}: {file} handles {event}, which matches more than one of the mod's events ({candidates}); qualify it")]
    AmbiguousEvent {
        id: String,
        file: String,
        event: String,
        candidates: String,
    },
    #[error("the modules' state regions exceed {MAX_STATE_BYTES} bytes")]
    StateOverflow,
}

type R<T> = Result<T, ResolveError>;

/// Checks a single mod's files against its manifest: every module, presentation and
/// profile file is present and matches any declared `sha256`, and every content glob
/// matches at least one file. Returns the files sorted by role then path.
pub fn check_files(pkg: &ModPackage) -> R<Vec<ResolvedFile>> {
    let m = &pkg.manifest;
    let id = &m.id;
    let lookup = |file: &str| -> R<Digest> {
        pkg.files
            .get(file)
            .copied()
            .ok_or_else(|| ResolveError::MissingFile {
                id: id.clone(),
                file: file.to_owned(),
            })
    };
    let mismatch = |file: &str, declared: &Digest, actual: &Digest| ResolveError::HashMismatch {
        id: id.clone(),
        file: file.to_owned(),
        declared: declared.to_hex(),
        actual: actual.to_hex(),
    };
    let mut out = BTreeSet::new();
    for glob in &m.content {
        let found = content_matches(glob, pkg.files.keys());
        if found.is_empty() {
            return Err(ResolveError::EmptyGlob {
                id: id.clone(),
                glob: glob.clone(),
            });
        }
        for path in found {
            let sha256 = lookup(path)?;
            out.insert(ResolvedFile {
                role: FileRole::Content,
                path: path.to_owned(),
                sha256,
            });
        }
    }
    if let Some(p) = &m.profile {
        out.insert(ResolvedFile {
            role: FileRole::Profile,
            path: p.clone(),
            sha256: lookup(p)?,
        });
    }
    for module in &m.modules {
        let actual = lookup(&module.file)?;
        if actual != module.sha256 {
            return Err(mismatch(&module.file, &module.sha256, &actual));
        }
        out.insert(ResolvedFile {
            role: FileRole::Module,
            path: module.file.clone(),
            sha256: actual,
        });
    }
    for p in &m.presentation {
        let actual = lookup(&p.file)?;
        if let Some(declared) = &p.sha256 {
            if *declared != actual {
                return Err(mismatch(&p.file, declared, &actual));
            }
        }
        out.insert(ResolvedFile {
            role: FileRole::Presentation,
            path: p.file.clone(),
            sha256: actual,
        });
    }
    Ok(out.into_iter().collect())
}

/// The match options used for content globs everywhere: case-sensitive, and `*` does
/// not cross `/`.
pub fn glob_options() -> glob::MatchOptions {
    glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    }
}

/// The paths among `paths` that `glob` matches.
pub fn content_matches<'a>(glob: &str, paths: impl Iterator<Item = &'a String>) -> Vec<&'a str> {
    let Ok(pattern) = glob::Pattern::new(glob) else {
        return Vec::new();
    };
    paths
        .filter(|p| pattern.matches_with(p, glob_options()))
        .map(String::as_str)
        .collect()
}

/// Resolves a mod set. See the module docs and `docs/resolved-set.md`.
pub fn resolve(
    packages: &[ModPackage],
    env: &Environment,
    options: &OptionValues,
) -> R<ResolvedSet> {
    check_environment(env)?;

    let mut by_id: BTreeMap<&str, &ModPackage> = BTreeMap::new();
    let mut dups = BTreeSet::new();
    for p in packages {
        if by_id.insert(p.manifest.id.as_str(), p).is_some() {
            dups.insert(p.manifest.id.as_str());
        }
    }
    if let Some(d) = dups.first() {
        return Err(ResolveError::DuplicateMod((*d).to_owned()));
    }
    let man = |id: &str| by_id.get(id).map(|p| &p.manifest);

    // Engine ranges.
    for (id, p) in &by_id {
        if !p.manifest.engine.matches(&env.engine) {
            return Err(ResolveError::EngineMismatch {
                id: (*id).to_owned(),
                range: p.manifest.engine.to_string(),
                engine: env.engine.to_string(),
            });
        }
    }

    // Interfaces implemented by synced mods: replacements of built-ins, or new ones.
    let mut implementers: BTreeMap<&str, &str> = BTreeMap::new();
    let mut provided: BTreeMap<&str, (&Version, &str)> = BTreeMap::new();
    for (id, p) in by_id.iter().filter(|(_, p)| p.manifest.is_synced()) {
        for module in &p.manifest.modules {
            let Some(imp) = &module.implements else {
                continue;
            };
            if let Some(first) = implementers.insert(&imp.name, id) {
                return Err(ResolveError::DuplicateImplementation {
                    interface: imp.name.clone(),
                    first: first.to_owned(),
                    second: (*id).to_owned(),
                });
            }
            match env.interfaces.get(&imp.name) {
                Some(avail) if avail.major != imp.version.major => {
                    return Err(ResolveError::InterfaceMajor {
                        id: (*id).to_owned(),
                        interface: imp.to_string(),
                        name: imp.name.clone(),
                        available: avail.to_string(),
                    });
                }
                Some(_) => {}
                None => {
                    provided.insert(&imp.name, (&imp.version, id));
                }
            }
        }
    }
    let interface_version = |name: &str| -> Option<&Version> {
        env.interfaces
            .get(name)
            .or_else(|| provided.get(name).map(|(v, _)| *v))
    };

    // Requirements.
    for (id, p) in &by_id {
        for (name, range) in &p.manifest.requires {
            let iface = interface_version(name);
            let module = man(name);
            match (iface, module) {
                (Some(_), Some(_)) => {
                    return Err(ResolveError::AmbiguousRequirement {
                        id: (*id).to_owned(),
                        name: name.clone(),
                    })
                }
                (None, None) => {
                    return Err(ResolveError::MissingRequirement {
                        id: (*id).to_owned(),
                        name: name.clone(),
                        range: range.to_string(),
                    })
                }
                (Some(v), None) => {
                    if !range.matches(v) {
                        return Err(ResolveError::VersionMismatch {
                            id: (*id).to_owned(),
                            what: "interface",
                            name: name.clone(),
                            range: range.to_string(),
                            found: v.to_string(),
                        });
                    }
                }
                (None, Some(dep)) => {
                    if !range.matches(&dep.version) {
                        return Err(ResolveError::VersionMismatch {
                            id: (*id).to_owned(),
                            what: "mod",
                            name: name.clone(),
                            range: range.to_string(),
                            found: dep.version.to_string(),
                        });
                    }
                    if p.manifest.is_synced() && !dep.is_synced() {
                        return Err(ResolveError::SyncedRequiresPresentation {
                            id: (*id).to_owned(),
                            name: name.clone(),
                        });
                    }
                }
            }
        }
    }

    // Conflicts.
    for (id, p) in &by_id {
        if let Some(other) = p
            .manifest
            .conflicts
            .iter()
            .find(|c| by_id.contains_key(c.as_str()))
        {
            return Err(ResolveError::Conflict {
                id: (*id).to_owned(),
                other: other.clone(),
            });
        }
    }

    // Wrapped interfaces must exist.
    for (id, p) in &by_id {
        for module in &p.manifest.modules {
            for w in &module.wraps {
                if interface_version(&w.interface).is_none() {
                    return Err(ResolveError::UnknownInterface {
                        id: (*id).to_owned(),
                        file: module.file.clone(),
                        slot: w.to_string(),
                        interface: w.interface.clone(),
                    });
                }
            }
        }
    }

    // Order: synced mods among themselves, then presentation-only mods among themselves.
    // Ordering constraints across the two groups are ignored, so a player's optional
    // presentation mods can never change the synced order.
    let mut order: Vec<&str> = Vec::new();
    for synced in [true, false] {
        let nodes: BTreeSet<&str> = by_id
            .iter()
            .filter(|(_, p)| p.manifest.is_synced() == synced)
            .map(|(id, _)| *id)
            .collect();
        let mut edges: BTreeSet<(&str, &str)> = BTreeSet::new();
        for id in &nodes {
            let Some(m) = man(id) else { continue };
            let mut add = |from: &str, to: &str| {
                if from != to && nodes.contains(from) && nodes.contains(to) {
                    if let (Some(f), Some(t)) = (nodes.get(from), nodes.get(to)) {
                        edges.insert((*f, *t));
                    }
                }
            };
            for name in m.requires.keys() {
                add(name, id);
                if let Some((_, provider)) = provided.get(name.as_str()) {
                    add(provider, id);
                }
            }
            for module in &m.modules {
                for w in &module.wraps {
                    if let Some((_, provider)) = provided.get(w.interface.as_str()) {
                        add(provider, id);
                    }
                }
            }
            for a in &m.after {
                add(a, id);
            }
            for b in &m.before {
                add(id, b);
            }
        }
        order.extend(topo(&nodes, &edges)?);
    }

    // Files and options, per mod.
    for (id, given) in options {
        let Some(m) = man(id) else {
            return Err(ResolveError::OptionForUnknownMod(id.clone()));
        };
        if let Some(name) = given.keys().find(|k| !m.options.contains_key(*k)) {
            return Err(ResolveError::UnknownOption {
                id: id.clone(),
                option: name.clone(),
            });
        }
    }
    let mut files: BTreeMap<&str, Vec<ResolvedFile>> = BTreeMap::new();
    let mut opts: BTreeMap<&str, Vec<(String, OptionValue)>> = BTreeMap::new();
    for (id, p) in &by_id {
        files.insert(id, check_files(p)?);
        opts.insert(id, option_values(&p.manifest, options.get(*id))?);
    }

    // Phases: each mod phase goes into the gap right after its `after` phase, or right
    // before its `before` phase; within a gap, in resolved mod order.
    let builtin: BTreeMap<&str, usize> = env
        .phases
        .iter()
        .enumerate()
        .map(|(i, p)| (p.as_str(), i))
        .collect();
    let mut gaps: Vec<Vec<String>> = vec![Vec::new(); env.phases.len() + 1];
    let mut phase_ids: BTreeMap<(&str, &str), String> = BTreeMap::new();
    for id in order
        .iter()
        .filter(|id| man(id).is_some_and(Manifest::is_synced))
    {
        let Some(m) = man(id) else { continue };
        for module in &m.modules {
            let Some(ph) = &module.phase else { continue };
            let find = |name: &Option<String>| -> R<Option<usize>> {
                match name {
                    None => Ok(None),
                    Some(n) => builtin.get(n.as_str()).copied().map(Some).ok_or_else(|| {
                        ResolveError::UnknownPhase {
                            id: (*id).to_owned(),
                            file: module.file.clone(),
                            phase: n.clone(),
                        }
                    }),
                }
            };
            let after = find(&ph.after)?;
            let before = find(&ph.before)?;
            let gap = match (after, before) {
                (Some(a), Some(b)) if a >= b => {
                    return Err(ResolveError::PhaseContradiction {
                        id: (*id).to_owned(),
                        file: module.file.clone(),
                        after: ph.after.clone().unwrap_or_default(),
                        before: ph.before.clone().unwrap_or_default(),
                    })
                }
                (Some(a), _) => a + 1,
                (None, Some(b)) => b,
                (None, None) => continue,
            };
            let pid = format!("{id}/{}", module.file);
            if let Some(g) = gaps.get_mut(gap) {
                g.push(pid.clone());
            }
            phase_ids.insert((id, module.file.as_str()), pid);
        }
    }
    let mut phases = Vec::new();
    for (i, gap) in gaps.into_iter().enumerate() {
        phases.extend(gap);
        if let Some(b) = env.phases.get(i) {
            phases.push(b.clone());
        }
    }

    // Event kinds, numbered in resolved order.
    let mut events: Vec<EventKind> = Vec::new();
    let mut numbers: BTreeMap<String, (u32, &str)> = BTreeMap::new();
    for id in order
        .iter()
        .filter(|id| man(id).is_some_and(Manifest::is_synced))
    {
        let Some(m) = man(id) else { continue };
        for module in &m.modules {
            let ns = namespace(m, module.implements.as_ref());
            for name in &module.publish {
                let full = format!("{ns}.{name}");
                if let Some((_, first)) = numbers.get(&full) {
                    return Err(ResolveError::DuplicateEvent {
                        name: full,
                        first: (*first).to_owned(),
                        second: (*id).to_owned(),
                    });
                }
                let number = u32::try_from(events.len()).unwrap_or(u32::MAX);
                numbers.insert(full.clone(), (number, id));
                events.push(EventKind {
                    number,
                    name: full,
                    publisher: (*id).to_owned(),
                });
            }
        }
    }
    let lookup_event = |id: &str, file: &str, ns: &str, name: &str| -> R<ResolvedEvent> {
        let full = format!("{ns}.{name}");
        if let Some((n, _)) = numbers.get(&full) {
            Ok(ResolvedEvent {
                name: full,
                number: Some(*n),
            })
        } else if env.interfaces.contains_key(ns) {
            Ok(ResolvedEvent {
                name: full,
                number: None,
            })
        } else {
            Err(ResolveError::UnknownEvent {
                id: id.to_owned(),
                file: file.to_owned(),
                event: full,
            })
        }
    };

    // State regions, in resolved order.
    let mut state = Vec::new();
    let mut cursor: u64 = 0;
    for id in order
        .iter()
        .filter(|id| man(id).is_some_and(Manifest::is_synced))
    {
        let Some(m) = man(id) else { continue };
        for module in &m.modules {
            let Some(len) = module.state_bytes else {
                continue;
            };
            let offset = cursor
                .checked_add(STATE_ALIGN - 1)
                .map(|c| c / STATE_ALIGN * STATE_ALIGN)
                .ok_or(ResolveError::StateOverflow)?;
            cursor = offset.checked_add(len).ok_or(ResolveError::StateOverflow)?;
            if cursor > MAX_STATE_BYTES {
                return Err(ResolveError::StateOverflow);
            }
            state.push(StateRegion {
                mod_id: (*id).to_owned(),
                file: module.file.clone(),
                offset,
                len,
            });
        }
    }

    // Build the resolved mods.
    let mut mods = Vec::new();
    for id in &order {
        let Some(m) = man(id) else { continue };
        let mut modules = Vec::new();
        for module in &m.modules {
            let ns = namespace(m, module.implements.as_ref());
            let mut subscribe = Vec::new();
            for e in &module.subscribe {
                let ens = e.namespace.as_deref().unwrap_or(ns);
                subscribe.push(lookup_event(id, &module.file, ens, &e.name)?);
            }
            subscribe.sort();
            let mut wraps = module.wraps.clone();
            wraps.sort();
            modules.push(ResolvedModule {
                file: module.file.clone(),
                sha256: module.sha256,
                implements: module.implements.clone(),
                wraps,
                phase: phase_ids.get(&(*id, module.file.as_str())).cloned(),
                state_bytes: module.state_bytes.unwrap_or(0),
                publish: module.publish.iter().map(|n| format!("{ns}.{n}")).collect(),
                subscribe,
            });
        }
        let mod_files = files.remove(id).unwrap_or_default();
        let mut presentation = Vec::new();
        for p in &m.presentation {
            let mut handles = Vec::new();
            for h in &p.handles {
                let ev = match &h.namespace {
                    Some(ns) => lookup_event(id, &p.file, ns, &h.name)?,
                    None => own_event(m, &modules, id, &p.file, &h.name, &numbers)?,
                };
                handles.push(ev);
            }
            let sha256 = mod_files
                .iter()
                .find(|f| f.role == FileRole::Presentation && f.path == p.file)
                .map(|f| f.sha256)
                .unwrap_or_default();
            presentation.push(ResolvedPresentation {
                file: p.file.clone(),
                sha256,
                handles,
            });
        }
        let mut rm = ResolvedMod {
            id: (*id).to_owned(),
            version: m.version.clone(),
            synced: m.is_synced(),
            files: mod_files,
            modules,
            presentation,
            options: opts.remove(id).unwrap_or_default(),
            digest: Digest::default(),
        };
        rm.digest = canon::mod_digest(&rm);
        mods.push(rm);
    }

    let mut set = ResolvedSet {
        mods,
        phases,
        events,
        state,
        hash: Digest::default(),
    };
    set.hash = Digest::of(&canon::set_bytes(&set));
    Ok(set)
}

/// The namespace of a module's events: its interface when it implements one, else the
/// mod id.
fn namespace<'a>(m: &'a Manifest, implements: Option<&'a InterfaceRef>) -> &'a str {
    implements.map_or(m.id.as_str(), |i| i.name.as_str())
}

/// A bare event name in `handles`: one of the mod's own published events.
fn own_event(
    m: &Manifest,
    modules: &[ResolvedModule],
    id: &str,
    file: &str,
    name: &str,
    numbers: &BTreeMap<String, (u32, &str)>,
) -> R<ResolvedEvent> {
    let suffix = format!(".{name}");
    let candidates: BTreeSet<&String> = modules
        .iter()
        .flat_map(|md| md.publish.iter())
        .filter(|full| full.ends_with(&suffix))
        .collect();
    let mut it = candidates.iter();
    match (it.next(), it.next()) {
        (Some(full), None) => Ok(ResolvedEvent {
            name: (*full).clone(),
            number: numbers.get(*full).map(|(n, _)| *n),
        }),
        (None, _) => Err(ResolveError::UnknownEvent {
            id: id.to_owned(),
            file: file.to_owned(),
            event: format!("{}.{name}", m.id),
        }),
        (Some(_), Some(_)) => Err(ResolveError::AmbiguousEvent {
            id: id.to_owned(),
            file: file.to_owned(),
            event: name.to_owned(),
            candidates: candidates
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        }),
    }
}

fn check_environment(env: &Environment) -> R<()> {
    if let Some(bad) = env.interfaces.keys().find(|k| !names::is_interface_name(k)) {
        return Err(ResolveError::InvalidEnvironment(format!(
            "{bad:?} is not an interface name"
        )));
    }
    let mut seen = BTreeSet::new();
    for p in &env.phases {
        if !names::is_snake_name(p) {
            return Err(ResolveError::InvalidEnvironment(format!(
                "{p:?} is not a phase name"
            )));
        }
        if !seen.insert(p) {
            return Err(ResolveError::InvalidEnvironment(format!(
                "phase {p:?} is listed twice"
            )));
        }
    }
    Ok(())
}

fn option_values(
    m: &Manifest,
    given: Option<&BTreeMap<String, OptionValue>>,
) -> R<Vec<(String, OptionValue)>> {
    let mut out = Vec::new();
    for (name, spec) in &m.options {
        let chosen = given.and_then(|g| g.get(name));
        let type_err = |expected: &'static str, v: &OptionValue| ResolveError::OptionType {
            id: m.id.clone(),
            option: name.clone(),
            expected,
            given: v.to_string(),
        };
        let value = match (spec, chosen) {
            (OptionSpec::Int { default, .. }, None) => OptionValue::Int(*default),
            (OptionSpec::Bool { default }, None) => OptionValue::Bool(*default),
            (OptionSpec::Enum { default, .. }, None) => OptionValue::Enum(default.clone()),
            (OptionSpec::Int { min, max, .. }, Some(v)) => {
                let OptionValue::Int(i) = v else {
                    return Err(type_err("an int", v));
                };
                let (lo, hi) = (min.unwrap_or(i64::MIN), max.unwrap_or(i64::MAX));
                if *i < lo || *i > hi {
                    return Err(ResolveError::OptionOutOfRange {
                        id: m.id.clone(),
                        option: name.clone(),
                        value: *i,
                        min: lo,
                        max: hi,
                    });
                }
                v.clone()
            }
            (OptionSpec::Bool { .. }, Some(v)) => {
                if !matches!(v, OptionValue::Bool(_)) {
                    return Err(type_err("a bool", v));
                }
                v.clone()
            }
            (OptionSpec::Enum { values, .. }, Some(v)) => {
                let OptionValue::Enum(s) = v else {
                    return Err(type_err("an enum", v));
                };
                if !values.contains(s) {
                    return Err(ResolveError::OptionNotInEnum {
                        id: m.id.clone(),
                        option: name.clone(),
                        value: s.clone(),
                        values: values.join(", "),
                    });
                }
                v.clone()
            }
        };
        out.push((name.clone(), value));
    }
    Ok(out)
}

/// Kahn's algorithm, always taking the smallest ready id. On a cycle, returns the cycle
/// that starts at the smallest id still unplaced, written forwards and closed
/// (`a -> b -> a`).
fn topo<'a>(nodes: &BTreeSet<&'a str>, edges: &BTreeSet<(&'a str, &'a str)>) -> R<Vec<&'a str>> {
    let mut indegree: BTreeMap<&'a str, usize> = nodes.iter().map(|n| (*n, 0)).collect();
    let mut succ: BTreeMap<&'a str, BTreeSet<&'a str>> = BTreeMap::new();
    let mut pred: BTreeMap<&'a str, BTreeSet<&'a str>> = BTreeMap::new();
    for (from, to) in edges {
        succ.entry(from).or_default().insert(to);
        pred.entry(to).or_default().insert(from);
        if let Some(d) = indegree.get_mut(to) {
            *d += 1;
        }
    }
    let mut ready: BTreeSet<&str> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(n, _)| *n)
        .collect();
    let mut out = Vec::new();
    while let Some(n) = ready.pop_first() {
        out.push(n);
        for s in succ.get(n).into_iter().flatten() {
            if let Some(d) = indegree.get_mut(s) {
                *d -= 1;
                if *d == 0 {
                    ready.insert(s);
                }
            }
        }
    }
    if out.len() == nodes.len() {
        return Ok(out);
    }
    let placed: BTreeSet<&str> = out.iter().copied().collect();
    let remaining: BTreeSet<&str> = nodes.difference(&placed).copied().collect();
    // Every unplaced node has an unplaced predecessor; walk backwards until a repeat.
    let mut path: Vec<&str> = Vec::new();
    let mut cur = remaining.first().copied().unwrap_or_default();
    loop {
        if let Some(pos) = path.iter().position(|p| *p == cur) {
            let mut cycle: Vec<&str> = path.split_off(pos);
            cycle.reverse();
            let start = cycle
                .iter()
                .enumerate()
                .min_by_key(|(_, n)| **n)
                .map_or(0, |(i, _)| i);
            cycle.rotate_left(start);
            let first = cycle.first().copied().unwrap_or_default();
            cycle.push(first);
            return Err(ResolveError::Cycle(
                cycle.into_iter().map(str::to_owned).collect(),
            ));
        }
        path.push(cur);
        let next = pred
            .get(cur)
            .and_then(|ps| ps.iter().find(|p| remaining.contains(*p)).copied());
        match next {
            Some(p) => cur = p,
            None => {
                return Err(ResolveError::Cycle(
                    path.iter().map(|s| (*s).to_owned()).collect(),
                ))
            }
        }
    }
}

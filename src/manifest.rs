//! The `btm_mod.json` v1 manifest (modding plan, section 2.2).
//!
//! The file is plain JSON. The plan's example is written as `jsonc` only so that it can
//! carry explanatory comments; a real `btm_mod.json` with comments (or trailing commas, or
//! duplicate keys) is rejected. Unknown fields are rejected too, at every level, so a typo
//! never silently drops a setting. Every error names the JSON path of the offending value,
//! such as `$.modules[0].phase.after`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use semver::Version;

use crate::digest::Digest;
use crate::json::{self, child, index, quote, Node};
use crate::names;
use crate::range::Range;

/// The only manifest format version this crate reads.
pub const MANIFEST_VERSION: u32 = 1;
/// Largest state region one module may ask for (the state block's ceiling, 256 MiB).
pub const MAX_STATE_BYTES: u64 = 256 * 1024 * 1024;

/// The layers of section 2.1. A mod may be several kinds at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// L0: TA archives and data files (`content`).
    Content,
    /// L1: an `oamod.yaml` profile (`profile`).
    Profile,
    /// L2: synced scripts, Lua or WASM (`modules`).
    Script,
    /// L3: subsystem modules (`modules`).
    Module,
    /// L4: presentation, optional per player and never hashed (`presentation`).
    Presentation,
    /// L5: a curated bundle of other mods (`requires`).
    Ruleset,
}

impl Kind {
    /// Every kind, in layer order.
    pub const ALL: [Kind; 6] = [
        Kind::Content,
        Kind::Profile,
        Kind::Script,
        Kind::Module,
        Kind::Presentation,
        Kind::Ruleset,
    ];

    /// The manifest spelling.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Content => "content",
            Kind::Profile => "profile",
            Kind::Script => "script",
            Kind::Module => "module",
            Kind::Presentation => "presentation",
            Kind::Ruleset => "ruleset",
        }
    }

    /// Parses the manifest spelling.
    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == s)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// `weather@1`: an interface by name and version. Only a major version is usually given;
/// missing minor and patch numbers are zero.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct InterfaceRef {
    pub name: String,
    pub version: Version,
}

impl fmt::Display for InterfaceRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let v = &self.version;
        if v.minor == 0 && v.patch == 0 && v.pre.is_empty() && v.build.is_empty() {
            write!(f, "{}@{}", self.name, v.major)
        } else {
            write!(f, "{}@{v}", self.name)
        }
    }
}

/// `wind_meteors.UpdateWind`: one entry point of an interface.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SlotRef {
    pub interface: String,
    pub slot: String,
}

impl fmt::Display for SlotRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.interface, self.slot)
    }
}

/// An event kind. `economy.WindChanged` names its namespace (an interface name or a mod
/// id); a bare `LightningStrike` (only allowed in `presentation[].handles`) means an
/// event the same mod publishes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EventRef {
    pub namespace: Option<String>,
    pub name: String,
}

impl fmt::Display for EventRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.namespace {
            Some(ns) => write!(f, "{ns}.{}", self.name),
            None => f.write_str(&self.name),
        }
    }
}

/// Tick placement relative to the built-in phases.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Phase {
    pub after: Option<String>,
    pub before: Option<String>,
}

/// One synced module (L2 script or L3 subsystem module).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub file: String,
    pub sha256: Digest,
    pub implements: Option<InterfaceRef>,
    pub wraps: Vec<SlotRef>,
    pub phase: Option<Phase>,
    pub state_bytes: Option<u64>,
    /// Event names this module publishes, in declaration order (the order they are
    /// numbered in).
    pub publish: Vec<String>,
    pub subscribe: Vec<EventRef>,
}

/// One presentation file (L4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Presentation {
    pub file: String,
    pub sha256: Option<Digest>,
    pub handles: Vec<EventRef>,
}

/// A typed option and its default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptionSpec {
    Int {
        min: Option<i64>,
        max: Option<i64>,
        default: i64,
    },
    Bool {
        default: bool,
    },
    Enum {
        values: Vec<String>,
        default: String,
    },
}

impl OptionSpec {
    /// The manifest spelling of the type.
    pub fn type_name(&self) -> &'static str {
        match self {
            OptionSpec::Int { .. } => "int",
            OptionSpec::Bool { .. } => "bool",
            OptionSpec::Enum { .. } => "enum",
        }
    }
}

/// A parsed and validated `btm_mod.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub id: String,
    pub version: Version,
    pub kind: BTreeSet<Kind>,
    pub engine: Range,
    /// Interface names or mod ids, each with a range.
    pub requires: BTreeMap<String, Range>,
    pub after: Vec<String>,
    pub before: Vec<String>,
    pub conflicts: Vec<String>,
    pub content: Vec<String>,
    pub profile: Option<String>,
    pub modules: Vec<Module>,
    pub presentation: Vec<Presentation>,
    pub options: BTreeMap<String, OptionSpec>,
}

/// What is wrong with a manifest value.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Problem {
    #[error("unknown field {field} (expected one of: {allowed})")]
    UnknownField { field: String, allowed: String },
    #[error("missing required field `{0}`")]
    MissingField(&'static str),
    #[error("expected {expected}, found {found}")]
    WrongType {
        expected: &'static str,
        found: &'static str,
    },
    #[error("manifest version {0} is not supported (this tool reads version 1)")]
    UnsupportedVersion(String),
    #[error("{0} is not a mod id: use a registry name (lowercase letters, digits and single hyphens, starting with a letter, such as \"weather-storms\") or reverse-DNS (dot-separated labels, such as \"io.github.alice.storms\")")]
    BadModId(String),
    #[error("{0} is neither a mod id nor an interface name")]
    BadRequirement(String),
    #[error("{value} is not a valid {what} ({rule})")]
    BadName {
        what: &'static str,
        value: String,
        rule: &'static str,
    },
    #[error("{value} is not a semver version: {reason}")]
    BadVersion { value: String, reason: String },
    #[error("{value} is not a version range: {reason}")]
    BadRange { value: String, reason: String },
    #[error("{0} is not a SHA-256 digest (expected 64 lowercase hex digits)")]
    BadDigest(String),
    #[error("path {value} {reason}")]
    BadPath { value: String, reason: &'static str },
    #[error("{value} is not a valid glob: {reason}")]
    BadGlob { value: String, reason: String },
    #[error("{0} is not a known kind (expected one of: content, profile, script, module, presentation, ruleset)")]
    UnknownKind(String),
    #[error("{0} is listed more than once")]
    Duplicate(String),
    #[error("a mod cannot refer to itself ({0})")]
    SelfReference(String),
    #[error("{0} is in both `after` and `before`")]
    AfterAndBefore(String),
    #[error("{0} is both required and listed as a conflict")]
    RequiresConflict(String),
    #[error("must not be empty")]
    Empty,
    #[error("kind `{kind}` needs a non-empty `{field}`")]
    KindNeedsField { kind: Kind, field: &'static str },
    #[error("`{field}` is given, but `kind` does not include {kinds}")]
    FieldNeedsKind {
        field: &'static str,
        kinds: &'static str,
    },
    #[error("a `.lua` module needs kind `script`")]
    LuaNeedsScript,
    #[error("a module file must end in `.wasm` or `.lua`")]
    BadModuleFile,
    #[error("{value} is out of range (allowed {min}..={max})")]
    OutOfRange { value: String, min: i128, max: i128 },
    #[error("{0} is not of the form name@version, such as \"weather@1\"")]
    BadInterfaceRef(String),
    #[error("{0} is not of the form interface.Slot, such as \"wind_meteors.UpdateWind\"")]
    BadSlotRef(String),
    #[error("{0} is not of the form namespace.EventName, such as \"economy.WindChanged\"")]
    BadEventRef(String),
    #[error("a phase needs `after`, `before` or both")]
    EmptyPhase,
    #[error("`after` and `before` name the same phase ({0})")]
    SamePhase(String),
    #[error("{0} is implemented by more than one module of this mod")]
    DuplicateImplements(String),
    #[error("{0} is unknown (expected int, bool or enum)")]
    UnknownOptionType(String),
    #[error("min {min} is greater than max {max}")]
    MinAboveMax { min: i64, max: i64 },
    #[error("default {default} is outside {min}..={max}")]
    DefaultOutOfBounds { default: i64, min: i64, max: i64 },
    #[error("default {default} is not one of the values ({values})")]
    DefaultNotInValues { default: String, values: String },
}

/// A manifest error: invalid JSON, or a value at a JSON path that breaks a rule.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    #[error("invalid JSON: {0} (btm_mod.json is plain JSON: no comments, no trailing commas)")]
    Syntax(String),
    #[error("{path}: {problem}")]
    Invalid { path: String, problem: Problem },
}

impl ManifestError {
    /// The JSON path, for [`ManifestError::Invalid`].
    pub fn path(&self) -> Option<&str> {
        match self {
            ManifestError::Syntax(_) => None,
            ManifestError::Invalid { path, .. } => Some(path),
        }
    }

    /// The problem, for [`ManifestError::Invalid`].
    pub fn problem(&self) -> Option<&Problem> {
        match self {
            ManifestError::Syntax(_) => None,
            ManifestError::Invalid { problem, .. } => Some(problem),
        }
    }
}

type R<T> = Result<T, ManifestError>;

fn err<T>(path: &str, problem: Problem) -> R<T> {
    Err(ManifestError::Invalid {
        path: path.to_owned(),
        problem,
    })
}

const TOP_FIELDS: &[&str] = &[
    "manifest",
    "id",
    "version",
    "kind",
    "engine",
    "requires",
    "after",
    "before",
    "conflicts",
    "content",
    "profile",
    "modules",
    "presentation",
    "options",
];
const MODULE_FIELDS: &[&str] = &[
    "file",
    "sha256",
    "implements",
    "wraps",
    "phase",
    "state_bytes",
    "events",
];
const PRESENTATION_FIELDS: &[&str] = &["file", "sha256", "handles"];

/// An object's fields, checked against an allow-list.
struct Fields<'a> {
    path: String,
    fields: &'a [(String, Node)],
}

impl<'a> Fields<'a> {
    fn new(node: &'a Node, path: &str, allowed: &[&str]) -> R<Self> {
        let Node::Obj(fields) = node else {
            return wrong(node, path, "an object");
        };
        if let Some((k, _)) = fields.iter().find(|(k, _)| !allowed.contains(&k.as_str())) {
            return err(
                &child(path, k),
                Problem::UnknownField {
                    field: quote(k),
                    allowed: allowed.join(", "),
                },
            );
        }
        Ok(Self {
            path: path.to_owned(),
            fields,
        })
    }

    fn get(&self, key: &str) -> Option<(&'a Node, String)> {
        self.fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(k, v)| (v, child(&self.path, k)))
    }

    fn req(&self, key: &'static str) -> R<(&'a Node, String)> {
        match self.get(key) {
            Some(x) => Ok(x),
            None => err(&self.path, Problem::MissingField(key)),
        }
    }
}

fn wrong<T>(node: &Node, path: &str, expected: &'static str) -> R<T> {
    err(
        path,
        Problem::WrongType {
            expected,
            found: node.type_name(),
        },
    )
}

fn string<'a>(node: &'a Node, path: &str) -> R<&'a str> {
    match node {
        Node::Str(s) => Ok(s),
        _ => wrong(node, path, "a string"),
    }
}

fn integer(node: &Node, path: &str) -> R<i128> {
    match node {
        Node::Int(i) => Ok(*i),
        Node::Float(_) => err(
            path,
            Problem::WrongType {
                expected: "an integer",
                found: "a number with a fraction or exponent",
            },
        ),
        _ => wrong(node, path, "an integer"),
    }
}

fn int64(node: &Node, path: &str) -> R<i64> {
    let i = integer(node, path)?;
    match i64::try_from(i) {
        Ok(v) => Ok(v),
        Err(_) => err(
            path,
            Problem::OutOfRange {
                value: i.to_string(),
                min: i128::from(i64::MIN),
                max: i128::from(i64::MAX),
            },
        ),
    }
}

fn boolean(node: &Node, path: &str) -> R<bool> {
    match node {
        Node::Bool(b) => Ok(*b),
        _ => wrong(node, path, "a boolean"),
    }
}

/// An array of strings with no duplicates, each with its path.
fn strings<'a>(node: &'a Node, path: &str) -> R<Vec<(&'a str, String)>> {
    let Node::Arr(items) = node else {
        return wrong(node, path, "an array of strings");
    };
    let mut out: Vec<(&str, String)> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let p = index(path, i);
        let s = string(item, &p)?;
        if out.iter().any(|(o, _)| *o == s) {
            return err(&p, Problem::Duplicate(quote(s)));
        }
        out.push((s, p));
    }
    Ok(out)
}

fn version(s: &str, path: &str) -> R<Version> {
    Version::parse(s).or_else(|e| {
        err(
            path,
            Problem::BadVersion {
                value: quote(s),
                reason: e.to_string(),
            },
        )
    })
}

fn range(s: &str, path: &str) -> R<Range> {
    Range::parse(s).or_else(|reason| {
        err(
            path,
            Problem::BadRange {
                value: quote(s),
                reason,
            },
        )
    })
}

fn mod_id(s: &str, path: &str) -> R<String> {
    if names::is_mod_id(s) {
        Ok(s.to_owned())
    } else {
        err(path, Problem::BadModId(quote(s)))
    }
}

fn rel_path(s: &str, path: &str) -> R<String> {
    match names::path_problem(s, false) {
        None => Ok(s.to_owned()),
        Some(reason) => err(
            path,
            Problem::BadPath {
                value: quote(s),
                reason,
            },
        ),
    }
}

fn digest(s: &str, path: &str) -> R<Digest> {
    match Digest::from_hex(s) {
        Some(d) => Ok(d),
        None => err(path, Problem::BadDigest(quote(s))),
    }
}

/// A list of mod ids that may not include the mod itself.
fn mod_list(node: Option<(&Node, String)>, own: &str) -> R<Vec<(String, String)>> {
    let Some((node, path)) = node else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (s, p) in strings(node, &path)? {
        let id = mod_id(s, &p)?;
        if id == own {
            return err(&p, Problem::SelfReference(quote(s)));
        }
        out.push((id, p));
    }
    Ok(out)
}

fn interface_ref(s: &str, path: &str) -> R<InterfaceRef> {
    let bad = || err(path, Problem::BadInterfaceRef(quote(s)));
    let Some((name, ver)) = s.split_once('@') else {
        return bad();
    };
    if !names::is_interface_name(name) {
        return bad();
    }
    let parts: Vec<&str> = ver.split('.').collect();
    let full = match parts.len() {
        1 => format!("{ver}.0.0"),
        2 => format!("{ver}.0"),
        _ => ver.to_owned(),
    };
    match Version::parse(&full) {
        Ok(version) => Ok(InterfaceRef {
            name: name.to_owned(),
            version,
        }),
        Err(_) => bad(),
    }
}

fn slot_ref(s: &str, path: &str) -> R<SlotRef> {
    match s.split_once('.') {
        Some((iface, slot)) if names::is_interface_name(iface) && names::is_slot_name(slot) => {
            Ok(SlotRef {
                interface: iface.to_owned(),
                slot: slot.to_owned(),
            })
        }
        _ => err(path, Problem::BadSlotRef(quote(s))),
    }
}

/// `ns.Name` (namespace an interface name or mod id); a bare `Name` only when
/// `allow_bare`.
fn event_ref(s: &str, path: &str, allow_bare: bool) -> R<EventRef> {
    match s.rsplit_once('.') {
        Some((ns, name))
            if names::is_event_name(name)
                && (names::is_interface_name(ns) || names::is_mod_id(ns)) =>
        {
            Ok(EventRef {
                namespace: Some(ns.to_owned()),
                name: name.to_owned(),
            })
        }
        None if allow_bare && names::is_event_name(s) => Ok(EventRef {
            namespace: None,
            name: s.to_owned(),
        }),
        _ => err(path, Problem::BadEventRef(quote(s))),
    }
}

fn snake_name(s: &str, path: &str, what: &'static str) -> R<String> {
    if names::is_snake_name(s) {
        Ok(s.to_owned())
    } else {
        err(
            path,
            Problem::BadName {
                what,
                value: quote(s),
                rule: "lowercase letters, digits and underscores, starting with a letter",
            },
        )
    }
}

impl Manifest {
    /// Parses and validates a manifest.
    pub fn parse(text: &str) -> Result<Manifest, ManifestError> {
        let root = json::parse(text).map_err(|e| ManifestError::Syntax(e.to_string()))?;
        Self::from_node(&root)
    }

    fn from_node(root: &Node) -> R<Manifest> {
        let f = Fields::new(root, "$", TOP_FIELDS)?;

        let (n, p) = f.req("manifest")?;
        let mv = integer(n, &p)?;
        if mv != i128::from(MANIFEST_VERSION) {
            return err(&p, Problem::UnsupportedVersion(mv.to_string()));
        }

        let (n, p) = f.req("id")?;
        let id = mod_id(string(n, &p)?, &p)?;

        let (n, p) = f.req("version")?;
        let version = version(string(n, &p)?, &p)?;

        let (n, p) = f.req("kind")?;
        let mut kind = BTreeSet::new();
        let list = strings(n, &p)?;
        if list.is_empty() {
            return err(&p, Problem::Empty);
        }
        for (s, kp) in list {
            match Kind::parse(s) {
                Some(k) => {
                    kind.insert(k);
                }
                None => return err(&kp, Problem::UnknownKind(quote(s))),
            }
        }

        let (n, p) = f.req("engine")?;
        let engine = range(string(n, &p)?, &p)?;

        let mut requires = BTreeMap::new();
        if let Some((n, p)) = f.get("requires") {
            let Node::Obj(entries) = n else {
                return wrong(n, &p, "an object");
            };
            for (k, v) in entries {
                let kp = child(&p, k);
                if !names::is_mod_id(k) && !names::is_interface_name(k) {
                    return err(&kp, Problem::BadRequirement(quote(k)));
                }
                if *k == id {
                    return err(&kp, Problem::SelfReference(quote(k)));
                }
                requires.insert(k.clone(), range(string(v, &kp)?, &kp)?);
            }
        }

        let after = mod_list(f.get("after"), &id)?;
        let before = mod_list(f.get("before"), &id)?;
        if let Some((b, bp)) = before
            .iter()
            .find(|(b, _)| after.iter().any(|(a, _)| a == b))
        {
            return err(bp, Problem::AfterAndBefore(quote(b)));
        }
        let conflicts = mod_list(f.get("conflicts"), &id)?;
        if let Some((c, cp)) = conflicts.iter().find(|(c, _)| requires.contains_key(c)) {
            return err(cp, Problem::RequiresConflict(quote(c)));
        }

        let mut content = Vec::new();
        if let Some((n, p)) = f.get("content") {
            for (s, gp) in strings(n, &p)? {
                if let Some(reason) = names::path_problem(s, true) {
                    return err(
                        &gp,
                        Problem::BadPath {
                            value: quote(s),
                            reason,
                        },
                    );
                }
                if let Err(e) = glob::Pattern::new(s) {
                    return err(
                        &gp,
                        Problem::BadGlob {
                            value: quote(s),
                            reason: e.msg.to_owned(),
                        },
                    );
                }
                content.push(s.to_owned());
            }
        }

        let profile = match f.get("profile") {
            Some((n, p)) => Some(rel_path(string(n, &p)?, &p)?),
            None => None,
        };

        let mut modules: Vec<Module> = Vec::new();
        if let Some((n, p)) = f.get("modules") {
            let Node::Arr(items) = n else {
                return wrong(n, &p, "an array of objects");
            };
            for (i, item) in items.iter().enumerate() {
                let mp = index(&p, i);
                let m = parse_module(item, &mp, &kind)?;
                if modules.iter().any(|o| o.file == m.file) {
                    return err(&child(&mp, "file"), Problem::Duplicate(quote(&m.file)));
                }
                if let Some(imp) = &m.implements {
                    if modules
                        .iter()
                        .any(|o| o.implements.as_ref().is_some_and(|x| x.name == imp.name))
                    {
                        return err(
                            &child(&mp, "implements"),
                            Problem::DuplicateImplements(quote(&imp.name)),
                        );
                    }
                }
                modules.push(m);
            }
        }

        let mut presentation: Vec<Presentation> = Vec::new();
        if let Some((n, p)) = f.get("presentation") {
            let Node::Arr(items) = n else {
                return wrong(n, &p, "an array of objects");
            };
            for (i, item) in items.iter().enumerate() {
                let pp = index(&p, i);
                let pr = parse_presentation(item, &pp)?;
                if presentation.iter().any(|o| o.file == pr.file)
                    || modules.iter().any(|m| m.file == pr.file)
                {
                    return err(&child(&pp, "file"), Problem::Duplicate(quote(&pr.file)));
                }
                presentation.push(pr);
            }
        }

        let mut options = BTreeMap::new();
        if let Some((n, p)) = f.get("options") {
            let Node::Obj(entries) = n else {
                return wrong(n, &p, "an object");
            };
            for (k, v) in entries {
                let op = child(&p, k);
                let name = snake_name(k, &op, "option name")?;
                options.insert(name, parse_option(v, &op)?);
            }
        }

        let m = Manifest {
            id,
            version,
            kind,
            engine,
            requires,
            after: after.into_iter().map(|(s, _)| s).collect(),
            before: before.into_iter().map(|(s, _)| s).collect(),
            conflicts: conflicts.into_iter().map(|(s, _)| s).collect(),
            content,
            profile,
            modules,
            presentation,
            options,
        };
        m.check_kinds()?;
        Ok(m)
    }

    /// Each declared kind needs its field. `modules` and `presentation` also need their
    /// kind, because together they decide what is synced; `content` and a `profile` may
    /// ride along with any kind (the plan's example is a module mod with archives).
    fn check_kinds(&self) -> R<()> {
        let has = |k: Kind| self.kind.contains(&k);
        // (kind declared, field set, kind, field, kinds that allow the field or None)
        let checks: [(bool, bool, Kind, &'static str, Option<&'static str>); 4] = [
            (
                has(Kind::Content),
                !self.content.is_empty(),
                Kind::Content,
                "content",
                None,
            ),
            (
                has(Kind::Profile),
                self.profile.is_some(),
                Kind::Profile,
                "profile",
                None,
            ),
            (
                has(Kind::Module) || has(Kind::Script),
                !self.modules.is_empty(),
                if has(Kind::Script) {
                    Kind::Script
                } else {
                    Kind::Module
                },
                "modules",
                Some("`module` or `script`"),
            ),
            (
                has(Kind::Presentation),
                !self.presentation.is_empty(),
                Kind::Presentation,
                "presentation",
                Some("`presentation`"),
            ),
        ];
        for (kind_on, field_set, kind, field, kinds) in checks {
            if kind_on && !field_set {
                return err("$.kind", Problem::KindNeedsField { kind, field });
            }
            if let Some(kinds) = kinds {
                if field_set && !kind_on {
                    return err(&child("$", field), Problem::FieldNeedsKind { field, kinds });
                }
            }
        }
        if has(Kind::Ruleset) && self.requires.is_empty() {
            return err(
                "$.kind",
                Problem::KindNeedsField {
                    kind: Kind::Ruleset,
                    field: "requires",
                },
            );
        }
        Ok(())
    }

    /// Whether the mod takes part in the sync hash: every mod except a pure
    /// presentation mod, one whose only kind is `presentation` and that ships no
    /// content or profile (open decision 3 of section 2.6).
    pub fn is_synced(&self) -> bool {
        self.kind.iter().any(|k| *k != Kind::Presentation)
            || !self.content.is_empty()
            || self.profile.is_some()
            || !self.modules.is_empty()
    }

    /// Writes the manifest as pretty JSON in the field order of the plan's example. The
    /// output parses back to an equal manifest.
    pub fn to_json(&self) -> String {
        json::to_pretty(&self.to_node())
    }

    fn to_node(&self) -> Node {
        let s = |v: &str| Node::Str(v.to_owned());
        let list = |v: &[String]| Node::Arr(v.iter().map(|x| s(x)).collect());
        let mut o: Vec<(String, Node)> = vec![
            ("manifest".into(), Node::Int(i128::from(MANIFEST_VERSION))),
            ("id".into(), s(&self.id)),
            ("version".into(), s(&self.version.to_string())),
            (
                "kind".into(),
                Node::Arr(self.kind.iter().map(|k| s(k.name())).collect()),
            ),
            ("engine".into(), s(&self.engine.to_string())),
        ];
        if !self.requires.is_empty() {
            o.push((
                "requires".into(),
                Node::Obj(
                    self.requires
                        .iter()
                        .map(|(k, v)| (k.clone(), s(&v.to_string())))
                        .collect(),
                ),
            ));
        }
        for (name, v) in [
            ("after", &self.after),
            ("before", &self.before),
            ("conflicts", &self.conflicts),
            ("content", &self.content),
        ] {
            if !v.is_empty() {
                o.push((name.into(), list(v)));
            }
        }
        if let Some(p) = &self.profile {
            o.push(("profile".into(), s(p)));
        }
        if !self.modules.is_empty() {
            o.push((
                "modules".into(),
                Node::Arr(self.modules.iter().map(module_node).collect()),
            ));
        }
        if !self.presentation.is_empty() {
            let items = self
                .presentation
                .iter()
                .map(|p| {
                    let mut f = vec![("file".into(), s(&p.file))];
                    if let Some(d) = &p.sha256 {
                        f.push(("sha256".into(), s(&d.to_hex())));
                    }
                    if !p.handles.is_empty() {
                        f.push((
                            "handles".into(),
                            Node::Arr(p.handles.iter().map(|h| s(&h.to_string())).collect()),
                        ));
                    }
                    Node::Obj(f)
                })
                .collect();
            o.push(("presentation".into(), Node::Arr(items)));
        }
        if !self.options.is_empty() {
            let items = self
                .options
                .iter()
                .map(|(k, v)| (k.clone(), option_node(v)))
                .collect();
            o.push(("options".into(), Node::Obj(items)));
        }
        Node::Obj(o)
    }
}

fn parse_module(node: &Node, path: &str, kind: &BTreeSet<Kind>) -> R<Module> {
    let f = Fields::new(node, path, MODULE_FIELDS)?;
    let (n, p) = f.req("file")?;
    let file = rel_path(string(n, &p)?, &p)?;
    if file.ends_with(".lua") {
        if !kind.contains(&Kind::Script) {
            return err(&p, Problem::LuaNeedsScript);
        }
    } else if !file.ends_with(".wasm") {
        return err(&p, Problem::BadModuleFile);
    }
    let (n, p) = f.req("sha256")?;
    let sha256 = digest(string(n, &p)?, &p)?;

    let implements = match f.get("implements") {
        Some((n, p)) => Some(interface_ref(string(n, &p)?, &p)?),
        None => None,
    };

    let mut wraps = Vec::new();
    if let Some((n, p)) = f.get("wraps") {
        for (s, sp) in strings(n, &p)? {
            wraps.push(slot_ref(s, &sp)?);
        }
    }

    let phase = match f.get("phase") {
        Some((n, p)) => {
            let pf = Fields::new(n, &p, &["after", "before"])?;
            let mut ph = Phase::default();
            if let Some((n, ap)) = pf.get("after") {
                ph.after = Some(snake_name(string(n, &ap)?, &ap, "phase name")?);
            }
            if let Some((n, bp)) = pf.get("before") {
                ph.before = Some(snake_name(string(n, &bp)?, &bp, "phase name")?);
            }
            match (&ph.after, &ph.before) {
                (None, None) => return err(&p, Problem::EmptyPhase),
                (Some(a), Some(b)) if a == b => {
                    return err(&child(&p, "before"), Problem::SamePhase(quote(b)))
                }
                _ => {}
            }
            Some(ph)
        }
        None => None,
    };

    let state_bytes = match f.get("state_bytes") {
        Some((n, p)) => {
            let v = integer(n, &p)?;
            let max = i128::from(MAX_STATE_BYTES);
            if !(1..=max).contains(&v) {
                return err(
                    &p,
                    Problem::OutOfRange {
                        value: v.to_string(),
                        min: 1,
                        max,
                    },
                );
            }
            u64::try_from(v).ok()
        }
        None => None,
    };

    let mut publish = Vec::new();
    let mut subscribe = Vec::new();
    if let Some((n, p)) = f.get("events") {
        let ef = Fields::new(n, &p, &["publish", "subscribe"])?;
        if let Some((n, pp)) = ef.get("publish") {
            for (s, sp) in strings(n, &pp)? {
                if !names::is_event_name(s) {
                    return err(
                        &sp,
                        Problem::BadName {
                            what: "event name",
                            value: quote(s),
                            rule: "an ASCII capital, then letters and digits",
                        },
                    );
                }
                publish.push(s.to_owned());
            }
        }
        if let Some((n, sp)) = ef.get("subscribe") {
            for (s, ip) in strings(n, &sp)? {
                subscribe.push(event_ref(s, &ip, false)?);
            }
        }
    }

    Ok(Module {
        file,
        sha256,
        implements,
        wraps,
        phase,
        state_bytes,
        publish,
        subscribe,
    })
}

fn parse_presentation(node: &Node, path: &str) -> R<Presentation> {
    let f = Fields::new(node, path, PRESENTATION_FIELDS)?;
    let (n, p) = f.req("file")?;
    let file = rel_path(string(n, &p)?, &p)?;
    if !file.ends_with(".wasm") && !file.ends_with(".lua") {
        return err(&p, Problem::BadModuleFile);
    }
    let sha256 = match f.get("sha256") {
        Some((n, p)) => Some(digest(string(n, &p)?, &p)?),
        None => None,
    };
    let mut handles = Vec::new();
    if let Some((n, p)) = f.get("handles") {
        for (s, hp) in strings(n, &p)? {
            handles.push(event_ref(s, &hp, true)?);
        }
    }
    Ok(Presentation {
        file,
        sha256,
        handles,
    })
}

fn parse_option(node: &Node, path: &str) -> R<OptionSpec> {
    let Node::Obj(fields) = node else {
        return wrong(node, path, "an object");
    };
    // Read `type` first so unknown fields can be judged against the right list.
    let Some((_, n)) = fields.iter().find(|(k, _)| k == "type") else {
        return err(path, Problem::MissingField("type"));
    };
    let tp = child(path, "type");
    let ty = string(n, &tp)?;
    match ty {
        "int" => {
            let f = Fields::new(node, path, &["type", "min", "max", "default"])?;
            let min = match f.get("min") {
                Some((n, p)) => Some(int64(n, &p)?),
                None => None,
            };
            let max = match f.get("max") {
                Some((n, p)) => Some(int64(n, &p)?),
                None => None,
            };
            if let (Some(lo), Some(hi)) = (min, max) {
                if lo > hi {
                    return err(
                        &child(path, "max"),
                        Problem::MinAboveMax { min: lo, max: hi },
                    );
                }
            }
            let (n, dp) = f.req("default")?;
            let default = int64(n, &dp)?;
            let lo = min.unwrap_or(i64::MIN);
            let hi = max.unwrap_or(i64::MAX);
            if default < lo || default > hi {
                return err(
                    &dp,
                    Problem::DefaultOutOfBounds {
                        default,
                        min: lo,
                        max: hi,
                    },
                );
            }
            Ok(OptionSpec::Int { min, max, default })
        }
        "bool" => {
            let f = Fields::new(node, path, &["type", "default"])?;
            let (n, dp) = f.req("default")?;
            Ok(OptionSpec::Bool {
                default: boolean(n, &dp)?,
            })
        }
        "enum" => {
            let f = Fields::new(node, path, &["type", "values", "default"])?;
            let (n, vp) = f.req("values")?;
            let list = strings(n, &vp)?;
            if list.is_empty() {
                return err(&vp, Problem::Empty);
            }
            let mut values = Vec::new();
            for (s, sp) in list {
                if !names::is_enum_value(s) {
                    return err(
                        &sp,
                        Problem::BadName {
                            what: "enum value",
                            value: quote(s),
                            rule: "1 to 64 printable ASCII characters, no spaces",
                        },
                    );
                }
                values.push(s.to_owned());
            }
            let (n, dp) = f.req("default")?;
            let default = string(n, &dp)?.to_owned();
            if !values.contains(&default) {
                return err(
                    &dp,
                    Problem::DefaultNotInValues {
                        default: quote(&default),
                        values: values.join(", "),
                    },
                );
            }
            Ok(OptionSpec::Enum { values, default })
        }
        other => err(&tp, Problem::UnknownOptionType(quote(other))),
    }
}

fn module_node(m: &Module) -> Node {
    let s = |v: &str| Node::Str(v.to_owned());
    let mut f = vec![
        ("file".into(), s(&m.file)),
        ("sha256".into(), s(&m.sha256.to_hex())),
    ];
    if let Some(i) = &m.implements {
        f.push(("implements".into(), s(&i.to_string())));
    }
    if !m.wraps.is_empty() {
        f.push((
            "wraps".into(),
            Node::Arr(m.wraps.iter().map(|w| s(&w.to_string())).collect()),
        ));
    }
    if let Some(ph) = &m.phase {
        let mut p = Vec::new();
        if let Some(a) = &ph.after {
            p.push(("after".into(), s(a)));
        }
        if let Some(b) = &ph.before {
            p.push(("before".into(), s(b)));
        }
        f.push(("phase".into(), Node::Obj(p)));
    }
    if let Some(b) = m.state_bytes {
        f.push(("state_bytes".into(), Node::Int(i128::from(b))));
    }
    if !m.publish.is_empty() || !m.subscribe.is_empty() {
        let mut e = Vec::new();
        if !m.publish.is_empty() {
            e.push((
                "publish".into(),
                Node::Arr(m.publish.iter().map(|x| s(x)).collect()),
            ));
        }
        if !m.subscribe.is_empty() {
            e.push((
                "subscribe".into(),
                Node::Arr(m.subscribe.iter().map(|x| s(&x.to_string())).collect()),
            ));
        }
        f.push(("events".into(), Node::Obj(e)));
    }
    Node::Obj(f)
}

fn option_node(o: &OptionSpec) -> Node {
    let mut f = vec![("type".into(), Node::Str(o.type_name().into()))];
    match o {
        OptionSpec::Int { min, max, default } => {
            if let Some(v) = min {
                f.push(("min".into(), Node::Int(i128::from(*v))));
            }
            if let Some(v) = max {
                f.push(("max".into(), Node::Int(i128::from(*v))));
            }
            f.push(("default".into(), Node::Int(i128::from(*default))));
        }
        OptionSpec::Bool { default } => f.push(("default".into(), Node::Bool(*default))),
        OptionSpec::Enum { values, default } => {
            f.push((
                "values".into(),
                Node::Arr(values.iter().map(|v| Node::Str(v.clone())).collect()),
            ));
            f.push(("default".into(), Node::Str(default.clone())));
        }
    }
    Node::Obj(f)
}

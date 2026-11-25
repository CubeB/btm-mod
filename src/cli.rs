//! The `btm-mod` command line. The plan's `btm mod <cmd>` is `btm-mod <cmd>` here.
//!
//! Exit codes: 0 success, 1 a check or resolve failed, 2 bad usage, 3 not implemented.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use semver::Version;

use crate::fs::{self, MANIFEST_FILE};
use crate::json::{self, Node};
use crate::manifest::{Kind, Manifest, OptionSpec};
use crate::resolve::{self, Environment, OptionValue, OptionValues, ResolvedSet};
use crate::template;

pub const USAGE: &str = "\
btm-mod: Byte Tactics mod tooling

usage:
  btm-mod new <kind> <dir> [--id <id>]
      Write a template btm_mod.json and skeleton. <kind> is one of content, profile,
      script, module, presentation, ruleset. A module or presentation mod is a minimal
      Rust cdylib crate; a script is Lua. The id defaults to the directory name.
  btm-mod check <dir> [--update-hashes]
      Validate the manifest, hash the files and check every sha256. With
      --update-hashes, first write the actual digests into the manifest.
  btm-mod resolve <dir>... --engine <version> [--interface <name>=<version>]...
                  [--phases <a,b,...>] [--option <mod>.<name>=<value>]... [--json]
      Resolve a mod set: print the load order, phases, event numbering, state layout
      and the resolved mod-set hash, or the first error.
  btm-mod preview <dir>...
      Not implemented yet: a dry-run diff of changed definition values needs game data
      (milestone M3.3).

exit codes: 0 ok, 1 failed, 2 usage, 3 not implemented
";

/// Runs the CLI with `args` (without the program name). Returns the exit code.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match dispatch(args, out) {
        Ok(()) => 0,
        Err(Fail::Usage(msg)) => {
            let _ = writeln!(err, "error: {msg}\n\n{USAGE}");
            2
        }
        Err(Fail::Failed(msg)) => {
            let _ = writeln!(err, "error: {msg}");
            1
        }
        Err(Fail::NotImplemented(msg)) => {
            let _ = writeln!(err, "{msg}");
            3
        }
    }
}

enum Fail {
    Usage(String),
    Failed(String),
    NotImplemented(String),
}

fn failed(e: impl std::fmt::Display) -> Fail {
    Fail::Failed(e.to_string())
}

fn dispatch(args: &[String], out: &mut dyn Write) -> Result<(), Fail> {
    let Some((cmd, rest)) = args.split_first() else {
        return Err(Fail::Usage("no command given".into()));
    };
    match cmd.as_str() {
        "new" => cmd_new(rest, out),
        "check" => cmd_check(rest, out),
        "resolve" => cmd_resolve(rest, out),
        "preview" => Err(Fail::NotImplemented(
            "btm-mod preview is not implemented yet: a diff of changed definition values \
             needs the game's unit and weapon data (milestone M3.3)"
                .into(),
        )),
        "help" | "--help" | "-h" => {
            let _ = write!(out, "{USAGE}");
            Ok(())
        }
        "--version" | "-V" => {
            let _ = writeln!(out, "btm-mod {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => Err(Fail::Usage(format!("unknown command {other:?}"))),
    }
}

/// Splits arguments into positionals and `--flag value` / `--switch` options.
struct Args {
    positional: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

fn parse_args(args: &[String], with_value: &[&str], switches: &[&str]) -> Result<Args, Fail> {
    let mut positional = Vec::new();
    let mut flags = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some(name) = a.strip_prefix("--") {
            if switches.contains(&name) {
                flags.push((name.to_owned(), None));
            } else if with_value.contains(&name) {
                let Some(v) = it.next() else {
                    return Err(Fail::Usage(format!("--{name} needs a value")));
                };
                flags.push((name.to_owned(), Some(v.clone())));
            } else {
                return Err(Fail::Usage(format!("unknown option {a:?}")));
            }
        } else {
            positional.push(a.clone());
        }
    }
    Ok(Args { positional, flags })
}

impl Args {
    fn values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.flags
            .iter()
            .filter(move |(n, _)| n == name)
            .filter_map(|(_, v)| v.as_deref())
    }

    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(n, _)| n == name)
    }
}

fn cmd_new(args: &[String], out: &mut dyn Write) -> Result<(), Fail> {
    let a = parse_args(args, &["id"], &[])?;
    let [kind, dir] = a.positional.as_slice() else {
        return Err(Fail::Usage("new needs <kind> <dir>".into()));
    };
    let Some(kind) = Kind::parse(kind) else {
        return Err(Fail::Usage(format!(
            "unknown kind {kind:?} (expected content, profile, script, module, presentation or ruleset)"
        )));
    };
    let dir = PathBuf::from(dir);
    let id = a
        .values("id")
        .last()
        .map_or_else(|| template::default_id(&dir), str::to_owned);
    let written = template::create(kind, &dir, &id).map_err(failed)?;
    let _ = writeln!(out, "created {kind} mod {id} in {}", dir.display());
    for p in written {
        let _ = writeln!(out, "  {}", p.display());
    }
    Ok(())
}

fn cmd_check(args: &[String], out: &mut dyn Write) -> Result<(), Fail> {
    let a = parse_args(args, &[], &["update-hashes"])?;
    let [dir] = a.positional.as_slice() else {
        return Err(Fail::Usage("check needs exactly one <dir>".into()));
    };
    let dir = Path::new(dir);
    if a.has("update-hashes") {
        update_hashes(dir, out)?;
    }
    let pkg = fs::load_package(dir).map_err(failed)?;
    let files = resolve::check_files(&pkg).map_err(failed)?;
    let m = &pkg.manifest;
    let kinds: Vec<&str> = m.kind.iter().map(|k| k.name()).collect();
    let _ = writeln!(
        out,
        "ok: {} {} ({}), {} file(s)",
        m.id,
        m.version,
        kinds.join(", "),
        files.len()
    );
    for f in &files {
        let _ = writeln!(out, "  {} {:<12} {}", f.sha256, f.role.name(), f.path);
    }
    let _ = writeln!(
        out,
        "note: WASM ABI and fuel checks (tools/wasm_modules.py) are not part of this tool yet"
    );
    Ok(())
}

/// Rewrites the manifest with the actual digest of every module and presentation file
/// that exists.
fn update_hashes(dir: &Path, out: &mut dyn Write) -> Result<(), Fail> {
    let mut m: Manifest = fs::read_manifest(dir).map_err(failed)?;
    let mut changed = 0;
    for module in &mut m.modules {
        let path = dir.join(&module.file);
        if path.is_file() {
            let d = fs::hash_file(&path).map_err(failed)?;
            if d != module.sha256 {
                module.sha256 = d;
                changed += 1;
            }
        }
    }
    for p in &mut m.presentation {
        let path = dir.join(&p.file);
        if path.is_file() {
            let d = fs::hash_file(&path).map_err(failed)?;
            if p.sha256 != Some(d) {
                p.sha256 = Some(d);
                changed += 1;
            }
        }
    }
    if changed > 0 {
        let path = dir.join(MANIFEST_FILE);
        std::fs::write(&path, m.to_json())
            .map_err(|e| Fail::Failed(format!("{}: {e}", path.display())))?;
    }
    let _ = writeln!(out, "updated {changed} sha256 value(s)");
    Ok(())
}

fn cmd_resolve(args: &[String], out: &mut dyn Write) -> Result<(), Fail> {
    let a = parse_args(
        args,
        &["engine", "interface", "phases", "option"],
        &["json"],
    )?;
    if a.positional.is_empty() {
        return Err(Fail::Usage("resolve needs at least one <dir>".into()));
    }
    let Some(engine) = a.values("engine").last() else {
        return Err(Fail::Usage("resolve needs --engine <version>".into()));
    };
    let engine =
        Version::parse(engine).map_err(|e| Fail::Usage(format!("--engine {engine:?}: {e}")))?;
    let mut interfaces = BTreeMap::new();
    for spec in a.values("interface") {
        let Some((name, ver)) = spec.split_once('=') else {
            return Err(Fail::Usage(format!(
                "--interface {spec:?}: expected <name>=<version>"
            )));
        };
        let ver =
            Version::parse(ver).map_err(|e| Fail::Usage(format!("--interface {spec:?}: {e}")))?;
        interfaces.insert(name.to_owned(), ver);
    }
    let phases: Vec<String> = a
        .values("phases")
        .flat_map(|p| p.split(','))
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect();
    let env = Environment {
        engine,
        interfaces,
        phases,
    };

    let mut packages = Vec::new();
    for dir in &a.positional {
        packages.push(fs::load_package(Path::new(dir)).map_err(failed)?);
    }
    let mut options: OptionValues = BTreeMap::new();
    for spec in a.values("option") {
        let (mod_id, name, value) = parse_option(spec, &packages)?;
        options.entry(mod_id).or_default().insert(name, value);
    }

    let set = resolve::resolve(&packages, &env, &options).map_err(failed)?;
    let lobby = set.to_lobby();
    if a.has("json") {
        let _ = write!(out, "{}", json::to_pretty(&set_json(&set, &lobby)));
    } else {
        print_set(&set, out);
        match &lobby {
            Ok(l) => {
                let _ = writeln!(out, "lobby: {} mod(s) listed", l.mods.len());
            }
            Err(e) => {
                let _ = writeln!(out, "lobby: does not fit: {e}");
            }
        }
    }
    Ok(())
}

/// `<mod>.<name>=<value>`, typed by the option's declared type. Unknown mods and options
/// are passed through as text so the resolver reports them.
fn parse_option(
    spec: &str,
    packages: &[resolve::ModPackage],
) -> Result<(String, String, OptionValue), Fail> {
    let bad = || Fail::Usage(format!("--option {spec:?}: expected <mod>.<name>=<value>"));
    let (key, value) = spec.split_once('=').ok_or_else(bad)?;
    let (mod_id, name) = key.rsplit_once('.').ok_or_else(bad)?;
    let declared = packages
        .iter()
        .find(|p| p.manifest.id == mod_id)
        .and_then(|p| p.manifest.options.get(name));
    let v = match declared {
        Some(OptionSpec::Int { .. }) => OptionValue::Int(value.parse().map_err(|_| {
            Fail::Failed(format!("{mod_id}.{name} is an int, but {value:?} is not"))
        })?),
        Some(OptionSpec::Bool { .. }) => match value {
            "true" => OptionValue::Bool(true),
            "false" => OptionValue::Bool(false),
            _ => {
                return Err(Fail::Failed(format!(
                    "{mod_id}.{name} is a bool: use true or false, not {value:?}"
                )))
            }
        },
        Some(OptionSpec::Enum { .. }) | None => OptionValue::Enum(value.to_owned()),
    };
    Ok((mod_id.to_owned(), name.to_owned(), v))
}

fn print_set(set: &ResolvedSet, out: &mut dyn Write) {
    let _ = writeln!(out, "order:");
    for (i, m) in set.mods.iter().enumerate() {
        let note = if m.synced {
            ""
        } else {
            "  (presentation only, not hashed)"
        };
        let _ = writeln!(out, "  {}. {} {}{note}", i + 1, m.id, m.version);
        for (name, v) in &m.options {
            let _ = writeln!(out, "       {name} = {v}");
        }
    }
    if !set.phases.is_empty() {
        let _ = writeln!(out, "phases: {}", set.phases.join(", "));
    }
    if !set.events.is_empty() {
        let _ = writeln!(out, "events:");
        for e in &set.events {
            let _ = writeln!(out, "  {} {} ({})", e.number, e.name, e.publisher);
        }
    }
    if !set.state.is_empty() {
        let _ = writeln!(out, "state:");
        for r in &set.state {
            let _ = writeln!(
                out,
                "  {}/{} at {} ({} bytes)",
                r.mod_id, r.file, r.offset, r.len
            );
        }
    }
    let _ = writeln!(out, "hash: {}", set.hash);
}

fn set_json(
    set: &ResolvedSet,
    lobby: &Result<crate::lobby::LobbyModSet, crate::lobby::LobbyError>,
) -> Node {
    let s = |v: &str| Node::Str(v.to_owned());
    let n = |v: u64| Node::Int(i128::from(v));
    let mods = set
        .mods
        .iter()
        .map(|m| {
            let files = m
                .files
                .iter()
                .map(|f| {
                    Node::Obj(vec![
                        ("role".into(), s(f.role.name())),
                        ("path".into(), s(&f.path)),
                        ("sha256".into(), s(&f.sha256.to_hex())),
                    ])
                })
                .collect();
            let options = m
                .options
                .iter()
                .map(|(k, v)| {
                    let v = match v {
                        OptionValue::Int(i) => Node::Int(i128::from(*i)),
                        OptionValue::Bool(b) => Node::Bool(*b),
                        OptionValue::Enum(e) => s(e),
                    };
                    (k.clone(), v)
                })
                .collect();
            Node::Obj(vec![
                ("id".into(), s(&m.id)),
                ("version".into(), s(&m.version.to_string())),
                ("synced".into(), Node::Bool(m.synced)),
                ("digest".into(), s(&m.digest.to_hex())),
                ("files".into(), Node::Arr(files)),
                ("options".into(), Node::Obj(options)),
            ])
        })
        .collect();
    let events = set
        .events
        .iter()
        .map(|e| {
            Node::Obj(vec![
                ("number".into(), n(u64::from(e.number))),
                ("name".into(), s(&e.name)),
                ("publisher".into(), s(&e.publisher)),
            ])
        })
        .collect();
    let state = set
        .state
        .iter()
        .map(|r| {
            Node::Obj(vec![
                ("mod".into(), s(&r.mod_id)),
                ("file".into(), s(&r.file)),
                ("offset".into(), n(r.offset)),
                ("len".into(), n(r.len)),
            ])
        })
        .collect();
    let lobby = match lobby {
        Ok(l) => Node::Obj(vec![(
            "mods".into(),
            Node::Arr(
                l.mods
                    .iter()
                    .map(|m| {
                        Node::Obj(vec![
                            ("id".into(), s(&m.id)),
                            ("version".into(), s(&m.version)),
                        ])
                    })
                    .collect(),
            ),
        )]),
        Err(e) => Node::Obj(vec![("error".into(), s(&e.to_string()))]),
    };
    Node::Obj(vec![
        ("hash".into(), s(&set.hash.to_hex())),
        ("mods".into(), Node::Arr(mods)),
        (
            "phases".into(),
            Node::Arr(set.phases.iter().map(|p| s(p)).collect()),
        ),
        ("events".into(), Node::Arr(events)),
        ("state".into(), Node::Arr(state)),
        ("lobby".into(), lobby),
    ])
}

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;

/// A fresh, empty directory under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let p =
            std::env::temp_dir().join(format!("btm-mod-test-{}-{n}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Out {
    let o = Command::new(env!("CARGO_BIN_EXE_btm-mod"))
        .args(args)
        .output()
        .unwrap();
    Out {
        code: o.status.code().unwrap_or(-1),
        stdout: String::from_utf8(o.stdout).unwrap(),
        stderr: String::from_utf8(o.stderr).unwrap(),
    }
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn help_and_usage_errors() {
    let o = run(&["help"]);
    assert_eq!(o.code, 0);
    assert!(o.stdout.contains("btm-mod new <kind> <dir>"));
    assert_eq!(run(&[]).code, 2);
    assert_eq!(run(&["frobnicate"]).code, 2);
    assert_eq!(run(&["check"]).code, 2);
    assert_eq!(run(&["check", "a", "--bogus"]).code, 2);
    let o = run(&["new", "mutator", "x"]);
    assert_eq!(o.code, 2);
    assert!(o.stderr.contains("unknown kind"));
    assert!(run(&["--version"]).stdout.starts_with("btm-mod "));
}

#[test]
fn preview_is_a_documented_stub() {
    let o = run(&["preview", "."]);
    assert_eq!(o.code, 3);
    assert!(o.stderr.contains("not implemented"));
}

#[test]
fn new_templates_check() {
    let t = TempDir::new();
    for (kind, passes) in [
        ("profile", true),
        ("script", true),
        ("ruleset", true),
        ("content", false),
        ("module", false),
        ("presentation", false),
    ] {
        let dir = t.path(&format!("my-{kind}"));
        let o = run(&["new", kind, s(&dir)]);
        assert_eq!(o.code, 0, "{kind}: {}", o.stderr);
        assert!(o.stdout.contains(&format!("created {kind} mod my-{kind}")));
        assert!(dir.join("btm_mod.json").is_file());
        let o = run(&["check", s(&dir)]);
        assert_eq!(o.code == 0, passes, "{kind}: {}{}", o.stdout, o.stderr);
    }
    // The Rust template is a minimal cdylib crate.
    let cargo = std::fs::read_to_string(t.path("my-module/Cargo.toml")).unwrap();
    assert!(cargo.contains("crate-type = [\"cdylib\"]"));
    assert!(cargo.contains("name = \"my_module\""));
    assert!(t.path("my-module/src/lib.rs").is_file());
    let o = run(&["check", s(&t.path("my-module"))]);
    assert!(
        o.stderr.contains("my_module.wasm is missing"),
        "{}",
        o.stderr
    );
    let o = run(&["check", s(&t.path("my-content"))]);
    assert!(
        o.stderr.contains("data/*.hpi matches no files"),
        "{}",
        o.stderr
    );
}

#[test]
fn new_refuses_non_empty_dirs_and_bad_ids() {
    let t = TempDir::new();
    write(&t.path("full/x.txt"), "x");
    let o = run(&["new", "module", s(&t.path("full"))]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("not empty"));
    let o = run(&["new", "module", s(&t.path("Bad_Name")), "--id", "Bad_Name"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("not a mod id"));
    let o = run(&[
        "new",
        "module",
        s(&t.path("Fancy Name")),
        "--id",
        "io.example.fancy",
    ]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let m = std::fs::read_to_string(t.path("Fancy Name/btm_mod.json")).unwrap();
    assert!(m.contains("\"id\": \"io.example.fancy\""));
}

#[test]
fn check_hashes_files_and_updates_them() {
    let t = TempDir::new();
    let dir = t.path("storms");
    assert_eq!(run(&["new", "module", s(&dir)]).code, 0);
    write(&dir.join("storms.wasm"), "\0asm fake");
    let o = run(&["check", s(&dir)]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("has SHA-256"), "{}", o.stderr);
    let o = run(&["check", "--update-hashes", s(&dir)]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("updated 1 sha256 value(s)"));
    assert!(o.stdout.contains("ok: storms 0.1.0 (module), 1 file(s)"));
    let expected = btm_mod::Digest::of(b"\0asm fake").to_hex();
    assert!(o.stdout.contains(&expected));
    // Tampering is caught.
    write(&dir.join("storms.wasm"), "\0asm changed");
    let o = run(&["check", s(&dir)]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains(&format!("the manifest says {expected}")));
}

#[test]
fn check_reports_json_paths() {
    let t = TempDir::new();
    write(
        &t.path("bad/btm_mod.json"),
        r#"{"manifest": 1, "id": "bad", "version": "1.0.0", "kind": ["profile"],
            "engine": "*", "profile": "oamod.yaml", "colour": "red"}"#,
    );
    let o = run(&["check", s(&t.path("bad"))]);
    assert_eq!(o.code, 1);
    assert!(
        o.stderr.contains("$.colour: unknown field \"colour\""),
        "{}",
        o.stderr
    );
    write(
        &t.path("jsonc/btm_mod.json"),
        "{\n  // a comment\n  \"manifest\": 1\n}",
    );
    let o = run(&["check", s(&t.path("jsonc"))]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("no comments"), "{}", o.stderr);
    let o = run(&["check", s(&t.path("missing"))]);
    assert_eq!(o.code, 1);
}

/// Two content mods with real archives, one depending on the other.
fn two_mods(t: &TempDir) -> (PathBuf, PathBuf) {
    let base = t.path("base");
    write(
        &base.join("btm_mod.json"),
        r#"{"manifest": 1, "id": "zz-base", "version": "1.0.0", "kind": ["content"],
            "engine": ">=0.4 <0.5", "content": ["data/*.hpi", "maps/**/*.ufo"]}"#,
    );
    write(&base.join("data/units.hpi"), "units");
    write(&base.join("data/ignored.txt"), "x");
    write(&base.join("maps/a/b/m.ufo"), "map");
    let addon = t.path("addon");
    write(
        &addon.join("btm_mod.json"),
        r#"{"manifest": 1, "id": "aa-addon", "version": "0.3.0", "kind": ["content"],
            "engine": "*", "requires": {"zz-base": "^1", "economy": "^5"},
            "content": ["data/*.hpi"],
            "options": {"speed": {"type": "int", "min": 1, "max": 3, "default": 1},
                        "night": {"type": "bool", "default": false}}}"#,
    );
    write(&addon.join("data/more.hpi"), "more");
    (base, addon)
}

#[test]
fn resolve_prints_order_and_hash() {
    let t = TempDir::new();
    let (base, addon) = two_mods(&t);
    let args = |a: &Path, b: &Path| -> Vec<String> {
        vec![
            "resolve".into(),
            s(a).into(),
            s(b).into(),
            "--engine".into(),
            "0.4.1".into(),
            "--interface".into(),
            "economy=5.0.0".into(),
        ]
    };
    let argv = args(&addon, &base);
    let o = run(&argv.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(o.code, 0, "{}", o.stderr);
    let base_at = o.stdout.find("1. zz-base 1.0.0").unwrap();
    let addon_at = o.stdout.find("2. aa-addon 0.3.0").unwrap();
    assert!(base_at < addon_at, "{}", o.stdout);
    assert!(o.stdout.contains("speed = 1"));
    assert!(o.stdout.contains("lobby: 2 mod(s) listed"));
    let hash_line = o.stdout.lines().find(|l| l.starts_with("hash: ")).unwrap();
    // Argument order does not matter.
    let argv = args(&base, &addon);
    let o2 = run(&argv.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(o2.stdout, o.stdout);

    // JSON output carries the same hash and every file.
    let mut argv = args(&base, &addon);
    argv.push("--json".into());
    let o3 = run(&argv.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(o3.code, 0, "{}", o3.stderr);
    let v: Value = serde_json::from_str(&o3.stdout).unwrap();
    assert_eq!(format!("hash: {}", v["hash"].as_str().unwrap()), hash_line);
    let files: Vec<&str> = v["mods"][0]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(files, ["data/units.hpi", "maps/a/b/m.ufo"]);

    // Options change the hash.
    let mut argv = args(&base, &addon);
    argv.extend(["--option".into(), "aa-addon.speed=2".into()]);
    argv.extend(["--option".into(), "aa-addon.night=true".into()]);
    let o4 = run(&argv.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(o4.code, 0, "{}", o4.stderr);
    assert!(o4.stdout.contains("speed = 2") && o4.stdout.contains("night = true"));
    assert!(!o4.stdout.contains(hash_line));

    // So do file contents.
    write(&base.join("data/units.hpi"), "units v2");
    let argv = args(&base, &addon);
    let o5 = run(&argv.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(!o5.stdout.contains(hash_line));
}

#[test]
fn resolve_reports_errors() {
    let t = TempDir::new();
    let (base, addon) = two_mods(&t);
    let o = run(&[
        "resolve",
        s(&addon),
        "--engine",
        "0.4.1",
        "--interface",
        "economy=5.0.0",
    ]);
    assert_eq!(o.code, 1);
    assert!(
        o.stderr.contains("aa-addon requires zz-base ^1"),
        "{}",
        o.stderr
    );
    let o = run(&["resolve", s(&base), "--engine", "0.5.0"]);
    assert_eq!(o.code, 1);
    assert!(
        o.stderr.contains("zz-base needs engine >=0.4, <0.5"),
        "{}",
        o.stderr
    );
    let o = run(&[
        "resolve",
        s(&base),
        s(&addon),
        "--engine",
        "0.4.1",
        "--interface",
        "economy=5.0.0",
        "--option",
        "aa-addon.speed=9",
    ]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("outside 1..=3"), "{}", o.stderr);
    let o = run(&[
        "resolve",
        s(&base),
        s(&addon),
        "--engine",
        "0.4.1",
        "--interface",
        "economy=5.0.0",
        "--option",
        "aa-addon.speed=fast",
    ]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("is an int"), "{}", o.stderr);
    // Usage errors.
    assert_eq!(run(&["resolve", s(&base)]).code, 2);
    assert_eq!(run(&["resolve", "--engine", "0.4.1"]).code, 2);
    assert_eq!(run(&["resolve", s(&base), "--engine", "x"]).code, 2);
    assert_eq!(
        run(&[
            "resolve",
            s(&base),
            "--engine",
            "0.4.1",
            "--option",
            "nodot"
        ])
        .code,
        2
    );
}

#[test]
fn resolve_shows_phases_events_and_state() {
    let t = TempDir::new();
    let dir = t.path("storms");
    write(&dir.join("storms.wasm"), "wasm");
    let sha = btm_mod::Digest::of(b"wasm").to_hex();
    write(
        &dir.join("btm_mod.json"),
        &format!(
            r#"{{"manifest": 1, "id": "storms", "version": "1.0.0", "kind": ["module"],
                "engine": "*",
                "modules": [{{"file": "storms.wasm", "sha256": "{sha}", "implements": "weather@1",
                              "phase": {{"after": "wind"}}, "state_bytes": 128,
                              "events": {{"publish": ["StormStarted"]}}}}]}}"#
        ),
    );
    let o = run(&[
        "resolve",
        s(&dir),
        "--engine",
        "0.4.0",
        "--phases",
        "wind,meteors",
    ]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(
        o.stdout
            .contains("phases: wind, storms/storms.wasm, meteors"),
        "{}",
        o.stdout
    );
    assert!(o.stdout.contains("0 weather.StormStarted (storms)"));
    assert!(o.stdout.contains("storms/storms.wasm at 0 (128 bytes)"));
}

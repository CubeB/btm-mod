#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use btm_mod::lobby::{LobbyError, MAX_MODS};
use btm_mod::{
    resolve, Digest, Environment, Manifest, ModPackage, OptionValue, OptionValues, ResolveError,
    ResolvedSet,
};
use serde_json::{json, Value};

/// A mod from manifest fields. Defaults: version 1.0.0, engine `*`, and kind `content`
/// with one archive. Module and presentation files get contents equal to their name, and
/// any missing `sha256` is filled in to match.
fn pkg(id: &str, fields: Value) -> ModPackage {
    let mut v = json!({
        "manifest": 1, "id": id, "version": "1.0.0", "kind": ["content"], "engine": "*",
        "content": ["data/*.hpi"]
    });
    for (k, f) in fields.as_object().unwrap() {
        v[k] = f.clone();
    }
    if v["content"].as_array().is_some_and(Vec::is_empty) {
        v.as_object_mut().unwrap().remove("content");
    }
    let mut files = BTreeMap::new();
    if v.get("content").is_some() {
        files.insert(format!("data/{id}.hpi"), Digest::of(id.as_bytes()));
    }
    if let Some(p) = v.get("profile").and_then(Value::as_str) {
        files.insert(p.to_owned(), Digest::of(p.as_bytes()));
    }
    for list in ["modules", "presentation"] {
        if let Some(items) = v.get_mut(list).and_then(Value::as_array_mut) {
            for item in items {
                let file = item["file"].as_str().unwrap().to_owned();
                let d = Digest::of(file.as_bytes());
                files.insert(file, d);
                if item.get("sha256").is_none() {
                    item["sha256"] = json!(d.to_hex());
                }
            }
        }
    }
    ModPackage {
        manifest: Manifest::parse(&v.to_string()).unwrap(),
        files,
    }
}

fn module(file: &str, extra: Value) -> Value {
    let mut m = json!({ "file": file });
    for (k, f) in extra.as_object().unwrap() {
        m[k] = f.clone();
    }
    m
}

fn env() -> Environment {
    Environment {
        engine: "0.4.2".parse().unwrap(),
        interfaces: BTreeMap::from([
            ("economy".into(), "5.1.0".parse().unwrap()),
            ("wind_meteors".into(), "3.0.2".parse().unwrap()),
            ("pathfinding".into(), "4.0.0".parse().unwrap()),
        ]),
        phases: vec!["wind".into(), "meteors".into(), "economy".into()],
    }
}

fn no_options() -> OptionValues {
    BTreeMap::new()
}

fn ok(pkgs: &[ModPackage]) -> ResolvedSet {
    resolve(pkgs, &env(), &no_options()).unwrap()
}

fn fail(pkgs: &[ModPackage]) -> ResolveError {
    resolve(pkgs, &env(), &no_options()).unwrap_err()
}

fn order(set: &ResolvedSet) -> Vec<&str> {
    set.mods.iter().map(|m| m.id.as_str()).collect()
}

/// The plan's weather mod plus the mods around it.
fn weather_world() -> Vec<ModPackage> {
    vec![
        pkg(
            "weather-storms",
            json!({
                "version": "1.2.0",
                "kind": ["module", "presentation"],
                "engine": ">=0.4 <0.5",
                "requires": { "wind_meteors": "^3" },
                "after": ["community-patch"],
                "conflicts": ["sunny-skies"],
                "modules": [module("storms.wasm", json!({
                    "implements": "weather@1",
                    "wraps": ["wind_meteors.UpdateWind"],
                    "phase": { "after": "wind", "before": "meteors" },
                    "state_bytes": 65536,
                    "events": { "publish": ["StormStarted", "LightningStrike"],
                                "subscribe": ["economy.WindChanged"] }
                }))],
                "presentation": [{ "file": "storms_fx.wasm", "handles": ["LightningStrike"] }],
                "options": { "storm_frequency": { "type": "int", "min": 0, "max": 10, "default": 3 } }
            }),
        ),
        pkg("community-patch", json!({})),
        pkg(
            "storm-insurance",
            json!({
                "kind": ["script"], "content": [],
                "requires": { "weather": "^1" },
                "modules": [module("insurance.lua", json!({
                    "state_bytes": 100,
                    "events": { "publish": ["ClaimPaid"],
                                "subscribe": ["weather.LightningStrike"] }
                }))]
            }),
        ),
        pkg(
            "hud",
            json!({
                "kind": ["presentation"], "content": [],
                "presentation": [{ "file": "hud.wasm",
                                   "handles": ["weather.StormStarted", "economy.WindChanged"] }]
            }),
        ),
    ]
}

#[test]
fn resolves_the_plan_example() {
    let set = ok(&weather_world());
    assert_eq!(
        order(&set),
        [
            "community-patch",
            "weather-storms",
            "storm-insurance",
            "hud"
        ]
    );
    assert_eq!(
        set.phases,
        ["wind", "weather-storms/storms.wasm", "meteors", "economy"]
    );
    let events: Vec<(u32, &str, &str)> = set
        .events
        .iter()
        .map(|e| (e.number, e.name.as_str(), e.publisher.as_str()))
        .collect();
    assert_eq!(
        events,
        [
            (0, "weather.StormStarted", "weather-storms"),
            (1, "weather.LightningStrike", "weather-storms"),
            (2, "storm-insurance.ClaimPaid", "storm-insurance"),
        ]
    );
    let storms = &set.mods[1];
    assert_eq!(
        storms.options,
        [("storm_frequency".into(), OptionValue::Int(3))]
    );
    assert_eq!(storms.modules[0].subscribe[0].name, "economy.WindChanged");
    assert_eq!(storms.modules[0].subscribe[0].number, None);
    assert_eq!(storms.presentation[0].handles[0].number, Some(1));
    let insurance = &set.mods[2];
    assert_eq!(insurance.modules[0].subscribe[0].number, Some(1));
    let hud = &set.mods[3];
    assert!(!hud.synced);
    assert_eq!(hud.presentation[0].handles[0].number, Some(0));
    let regions: Vec<(&str, u64, u64)> = set
        .state
        .iter()
        .map(|r| (r.mod_id.as_str(), r.offset, r.len))
        .collect();
    assert_eq!(
        regions,
        [
            ("weather-storms", 0, 65536),
            ("storm-insurance", 65536, 100)
        ]
    );
    // Every file is listed with its hash, sorted by role and path.
    let files: Vec<(&str, &str)> = storms
        .files
        .iter()
        .map(|f| (f.role.name(), f.path.as_str()))
        .collect();
    assert_eq!(
        files,
        [
            ("content", "data/weather-storms.hpi"),
            ("module", "storms.wasm"),
            ("presentation", "storms_fx.wasm")
        ]
    );
}

#[test]
fn ties_break_by_id() {
    let set = ok(&[
        pkg("c", json!({})),
        pkg("a", json!({})),
        pkg("b", json!({})),
    ]);
    assert_eq!(order(&set), ["a", "b", "c"]);
}

#[test]
fn constraints_beat_ids() {
    let set = ok(&[
        pkg("a", json!({ "after": ["c"] })),
        pkg("b", json!({ "before": ["a"] })),
        pkg("c", json!({})),
        pkg("d", json!({ "requires": { "e": "^1" } })),
        pkg("e", json!({})),
    ]);
    // Ready sets: {b, c, e} -> b; {c, e} -> c; {a, e} -> a; {e} -> e; {d}.
    assert_eq!(order(&set), ["b", "c", "a", "e", "d"]);
}

#[test]
fn after_and_before_on_absent_mods_are_ignored() {
    let set = ok(&[
        pkg("a", json!({ "after": ["zzz"], "before": ["yyy"] })),
        pkg("b", json!({})),
    ]);
    assert_eq!(order(&set), ["a", "b"]);
}

#[test]
fn interface_providers_come_first() {
    let set = ok(&[
        pkg(
            "a-user",
            json!({ "kind": ["module"], "content": [],
                    "modules": [module("u.wasm", json!({ "wraps": ["zz_weather.Rain"] }))] }),
        ),
        pkg(
            "z-provider",
            json!({ "kind": ["module"], "content": [],
                    "modules": [module("p.wasm", json!({ "implements": "zz_weather@2" }))] }),
        ),
    ]);
    assert_eq!(order(&set), ["z-provider", "a-user"]);
}

#[test]
fn presentation_mods_never_change_the_synced_order_or_hash() {
    let a = pkg("a", json!({}));
    let b = pkg("b", json!({}));
    let hud = pkg(
        "hud",
        json!({ "kind": ["presentation"], "content": [], "after": ["b"], "before": ["a"],
                "presentation": [{ "file": "hud.wasm" }] }),
    );
    let without = ok(&[a.clone(), b.clone()]);
    let with = ok(&[a, b, hud]);
    assert_eq!(order(&with), ["a", "b", "hud"]);
    assert_eq!(with.hash, without.hash);
    assert_eq!(with.to_lobby(), without.to_lobby());
}

#[test]
fn presentation_files_of_synced_mods_are_not_hashed() {
    let mk = |fx: &str| {
        pkg(
            "fx",
            json!({ "kind": ["content", "presentation"],
                    "presentation": [{ "file": fx }] }),
        )
    };
    assert_eq!(ok(&[mk("a.wasm")]).hash, ok(&[mk("b.wasm")]).hash);
}

#[test]
fn phases_go_next_to_their_anchor() {
    let m = |id: &str, phase: Value| {
        pkg(
            id,
            json!({ "kind": ["module"], "content": [],
                    "modules": [module("m.wasm", json!({ "phase": phase }))] }),
        )
    };
    let set = ok(&[
        m("d", json!({ "after": "economy" })),
        m("c", json!({ "before": "economy" })),
        m("b", json!({ "after": "wind" })),
        m("a", json!({ "after": "wind", "before": "economy" })),
        m("e", json!({ "before": "wind" })),
    ]);
    assert_eq!(
        set.phases,
        [
            "e/m.wasm", "wind", "a/m.wasm", "b/m.wasm", "meteors", "c/m.wasm", "economy",
            "d/m.wasm"
        ]
    );
}

#[test]
fn hash_is_stable() {
    let set = ok(&weather_world());
    // A change here means the canonical encoding changed: bump its version and update
    // docs/resolved-set.md.
    assert_eq!(
        set.hash.to_hex(),
        "8d9d1b6395d2151bfd5b43e5016f73352ea9495c41873d79f01f4a6210a1e15e"
    );
    assert_eq!(Digest::of(&set.canonical_bytes()), set.hash);
}

#[test]
fn empty_set_encoding_matches_the_spec() {
    let env = Environment {
        engine: "0.4.0".parse().unwrap(),
        interfaces: BTreeMap::new(),
        phases: Vec::new(),
    };
    let set = resolve(&[], &env, &no_options()).unwrap();
    let tag = b"btm-mod/resolved-set/v1";
    let mut expect = (tag.len() as u32).to_le_bytes().to_vec();
    expect.extend_from_slice(tag);
    expect.extend_from_slice(&[0; 16]); // four empty lists
    assert_eq!(set.canonical_bytes(), expect);
    assert_eq!(set.hash, Digest::of(&expect));
}

#[test]
fn hash_changes_when_any_input_changes() {
    let base = ok(&weather_world()).hash;
    let mut seen = vec![base];
    let mut check = |name: &str, set: ResolvedSet| {
        assert!(!seen.contains(&set.hash), "{name} did not change the hash");
        seen.push(set.hash);
    };
    let edit = |id: &str, f: &dyn Fn(&mut ModPackage)| {
        let mut w = weather_world();
        for p in &mut w {
            if p.manifest.id == id {
                f(p);
            }
        }
        w
    };

    check(
        "version",
        ok(&edit("community-patch", &|p| {
            p.manifest.version = "1.0.1".parse().unwrap()
        })),
    );
    check(
        "content file",
        ok(&edit("community-patch", &|p| {
            p.files
                .insert("data/community-patch.hpi".into(), Digest::of(b"other"));
        })),
    );
    check(
        "extra content file",
        ok(&edit("community-patch", &|p| {
            p.files.insert("data/more.hpi".into(), Digest::of(b"more"));
        })),
    );
    check(
        "module file",
        ok(&edit("weather-storms", &|p| {
            let d = Digest::of(b"new");
            p.files.insert("storms.wasm".into(), d);
            p.manifest.modules[0].sha256 = d;
        })),
    );
    let mut opts = no_options();
    opts.entry("weather-storms".into())
        .or_default()
        .insert("storm_frequency".into(), OptionValue::Int(4));
    check("option", resolve(&weather_world(), &env(), &opts).unwrap());
    check(
        "order",
        ok(&edit("weather-storms", &|p| {
            p.manifest.after.clear();
            p.manifest.before = vec!["community-patch".into()];
        })),
    );
    check(
        "state size",
        ok(&edit("weather-storms", &|p| {
            p.manifest.modules[0].state_bytes = Some(65537);
        })),
    );
    check(
        "event declaration order",
        ok(&edit("weather-storms", &|p| {
            p.manifest.modules[0].publish.reverse()
        })),
    );
    check(
        "wraps",
        ok(&edit("weather-storms", &|p| {
            p.manifest.modules[0].wraps.clear()
        })),
    );
    check(
        "phase",
        ok(&edit("weather-storms", &|p| {
            p.manifest.modules[0].phase.as_mut().unwrap().before = None;
            p.manifest.modules[0].phase.as_mut().unwrap().after = Some("meteors".into());
        })),
    );
    check(
        "subscription",
        ok(&edit("weather-storms", &|p| {
            p.manifest.modules[0].subscribe.clear()
        })),
    );
    let mut w = weather_world();
    w.push(pkg("zz-extra", json!({})));
    check("added mod", ok(&w));
    let mut e = env();
    e.phases.push("cleanup".into());
    check(
        "built-in phases",
        resolve(&weather_world(), &e, &no_options()).unwrap(),
    );
}

/// A tiny xorshift generator, so the shuffles are the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }
}

#[test]
fn deterministic_under_shuffled_input() {
    let mut world = weather_world();
    world.extend([
        pkg("m1", json!({ "after": ["m3"] })),
        pkg("m2", json!({ "before": ["m1"], "requires": { "community-patch": "^1" } })),
        pkg("m3", json!({})),
        pkg(
            "m4",
            json!({ "kind": ["module"], "content": [],
                    "modules": [
                        module("x.wasm", json!({ "phase": { "before": "economy" }, "state_bytes": 7,
                                                 "events": { "publish": ["Ping"] } })),
                        module("y.wasm", json!({ "phase": { "after": "wind" }, "state_bytes": 9,
                                                 "events": { "subscribe": ["m4.Ping"] } }))
                    ],
                    "options": { "on": { "type": "bool", "default": false },
                                 "mode": { "type": "enum", "values": ["a", "b"], "default": "a" } } }),
        ),
        pkg("m5", json!({ "profile": "oamod.yaml" })),
    ]);
    let mut opts = no_options();
    opts.entry("m4".into())
        .or_default()
        .insert("mode".into(), OptionValue::Enum("b".into()));
    let reference = resolve(&world, &env(), &opts).unwrap();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..200 {
        let mut w = world.clone();
        rng.shuffle(&mut w);
        for p in &mut w {
            // Reordering the entries of a mod's file map cannot matter either (it is a
            // BTreeMap), but the order of a mod's after/before/conflicts lists must not.
            let n = p.manifest.after.len();
            if n > 1 {
                p.manifest.after.rotate_left(1);
            }
        }
        assert_eq!(resolve(&w, &env(), &opts).unwrap(), reference);
    }
    // The same holds for failures: the first error does not depend on input order.
    let mut bad = world.clone();
    bad.push(pkg("sunny-skies", json!({})));
    bad.push(pkg("x-cycle", json!({ "after": ["y-cycle"] })));
    bad.push(pkg("y-cycle", json!({ "after": ["x-cycle"] })));
    let first = resolve(&bad, &env(), &opts).unwrap_err();
    for _ in 0..50 {
        rng.shuffle(&mut bad);
        assert_eq!(resolve(&bad, &env(), &opts).unwrap_err(), first);
    }
}

#[test]
fn missing_requirements() {
    let e = fail(&[pkg("a", json!({ "requires": { "b": "^1" } }))]);
    assert_eq!(
        e,
        ResolveError::MissingRequirement {
            id: "a".into(),
            name: "b".into(),
            range: "^1".into()
        }
    );
    assert_eq!(
        e.to_string(),
        "a requires b ^1, which is neither an available interface nor a mod in the set"
    );
    let e = fail(&[pkg("a", json!({ "requires": { "rain_clouds": "*" } }))]);
    assert!(matches!(e, ResolveError::MissingRequirement { .. }));
}

#[test]
fn version_mismatches() {
    let e = fail(&[
        pkg("a", json!({ "requires": { "b": "^2" } })),
        pkg("b", json!({})),
    ]);
    assert_eq!(
        e.to_string(),
        "a requires mod b ^2, but version 1.0.0 is present"
    );
    let e = fail(&[pkg("a", json!({ "requires": { "economy": "^4" } }))]);
    assert_eq!(
        e.to_string(),
        "a requires interface economy ^4, but version 5.1.0 is present"
    );
    let e = fail(&[pkg("a", json!({ "requires": { "economy": ">=5.2" } }))]);
    assert!(matches!(e, ResolveError::VersionMismatch { .. }));
}

#[test]
fn cycles_are_named() {
    let e = fail(&[
        pkg("a", json!({ "after": ["b"] })),
        pkg("b", json!({ "after": ["c"] })),
        pkg("c", json!({ "after": ["a"] })),
        pkg("d", json!({ "after": ["a"] })),
        pkg("a-free", json!({})),
    ]);
    assert_eq!(
        e,
        ResolveError::Cycle(vec!["a".into(), "c".into(), "b".into(), "a".into()])
    );
    assert_eq!(e.to_string(), "ordering cycle: a -> c -> b -> a");
    // Mixed edge kinds: requires plus before.
    let e = fail(&[
        pkg("x", json!({ "requires": { "y": "*" } })),
        pkg("y", json!({ "after": ["x"] })),
    ]);
    assert_eq!(e.to_string(), "ordering cycle: x -> y -> x");
    let e = fail(&[
        pkg("p", json!({ "before": ["q"] })),
        pkg("q", json!({ "before": ["p"] })),
    ]);
    assert_eq!(e.to_string(), "ordering cycle: p -> q -> p");
}

#[test]
fn conflicts() {
    let mut w = weather_world();
    w.push(pkg("sunny-skies", json!({})));
    assert_eq!(
        fail(&w),
        ResolveError::Conflict {
            id: "weather-storms".into(),
            other: "sunny-skies".into()
        }
    );
}

#[test]
fn engine_mismatch() {
    let e = fail(&[pkg("a", json!({ "engine": ">=0.5" }))]);
    assert_eq!(
        e.to_string(),
        "a needs engine >=0.5, but the engine is 0.4.2"
    );
}

#[test]
fn other_failures() {
    assert_eq!(
        fail(&[pkg("a", json!({})), pkg("a", json!({}))]),
        ResolveError::DuplicateMod("a".into())
    );
    // An interface and a mod with the same name.
    let e = fail(&[
        pkg("x", json!({ "requires": { "economy": "*" } })),
        pkg("economy", json!({})),
    ]);
    assert!(matches!(e, ResolveError::AmbiguousRequirement { .. }));
    // A synced mod may not need a presentation-only one.
    let e = fail(&[
        pkg("x", json!({ "requires": { "hud": "*" } })),
        pkg(
            "hud",
            json!({ "kind": ["presentation"], "content": [],
                    "presentation": [{ "file": "h.wasm" }] }),
        ),
    ]);
    assert!(matches!(e, ResolveError::SyncedRequiresPresentation { .. }));
    let impl_mod = |id: &str, iface: &str| {
        pkg(
            id,
            json!({ "kind": ["module"], "content": [],
                    "modules": [module("m.wasm", json!({ "implements": iface }))] }),
        )
    };
    assert!(matches!(
        fail(&[impl_mod("a", "economy@4")]),
        ResolveError::InterfaceMajor { .. }
    ));
    ok(&[impl_mod("a", "economy@5")]);
    assert_eq!(
        fail(&[impl_mod("a", "rain@1"), impl_mod("b", "rain@1")]),
        ResolveError::DuplicateImplementation {
            interface: "rain".into(),
            first: "a".into(),
            second: "b".into()
        }
    );
    assert!(matches!(
        fail(&[impl_mod("a", "economy@5"), impl_mod("b", "economy@5")]),
        ResolveError::DuplicateImplementation { .. }
    ));
    let e = fail(&[pkg(
        "a",
        json!({ "kind": ["module"], "content": [],
                "modules": [module("m.wasm", json!({ "wraps": ["rain.Fall"] }))] }),
    )]);
    assert!(matches!(e, ResolveError::UnknownInterface { .. }));
}

#[test]
fn file_failures() {
    let mut p = pkg(
        "a",
        json!({ "kind": ["module"], "content": [], "modules": [module("m.wasm", json!({}))] }),
    );
    p.files.insert("m.wasm".into(), Digest::of(b"tampered"));
    assert!(matches!(
        fail(&[p.clone()]),
        ResolveError::HashMismatch { .. }
    ));
    p.files.clear();
    assert_eq!(
        fail(&[p]),
        ResolveError::MissingFile {
            id: "a".into(),
            file: "m.wasm".into()
        }
    );
    let mut p = pkg("a", json!({}));
    p.files.clear();
    p.files.insert("data/sub/x.hpi".into(), Digest::of(b"x")); // `*` does not cross `/`
    p.files.insert("data/x.HPI".into(), Digest::of(b"x")); // case-sensitive
    assert!(matches!(fail(&[p]), ResolveError::EmptyGlob { .. }));
    let mut p = pkg("a", json!({ "profile": "oamod.yaml" }));
    p.files.remove("oamod.yaml");
    assert!(matches!(fail(&[p]), ResolveError::MissingFile { .. }));
    let mut p = pkg(
        "a",
        json!({ "kind": ["content", "presentation"],
                "presentation": [{ "file": "fx.wasm", "sha256": Digest::of(b"x").to_hex() }] }),
    );
    p.files.insert("fx.wasm".into(), Digest::of(b"y"));
    assert!(matches!(fail(&[p]), ResolveError::HashMismatch { .. }));
}

#[test]
fn option_failures() {
    let w = weather_world();
    let run = |id: &str, name: &str, v: OptionValue| {
        let mut o = no_options();
        o.entry(id.into()).or_default().insert(name.into(), v);
        resolve(&w, &env(), &o).unwrap_err()
    };
    assert_eq!(
        run("nobody", "x", OptionValue::Int(1)),
        ResolveError::OptionForUnknownMod("nobody".into())
    );
    assert!(matches!(
        run("weather-storms", "rainfall", OptionValue::Int(1)),
        ResolveError::UnknownOption { .. }
    ));
    assert!(matches!(
        run("weather-storms", "storm_frequency", OptionValue::Bool(true)),
        ResolveError::OptionType { .. }
    ));
    assert_eq!(
        run("weather-storms", "storm_frequency", OptionValue::Int(11)).to_string(),
        "weather-storms.storm_frequency = 11 is outside 0..=10"
    );
    let m = pkg(
        "m",
        json!({ "options": { "on": { "type": "bool", "default": false },
                             "mode": { "type": "enum", "values": ["a", "b"], "default": "a" } } }),
    );
    let run = |name: &str, v: OptionValue| {
        let mut o = no_options();
        o.entry("m".into()).or_default().insert(name.into(), v);
        resolve(std::slice::from_ref(&m), &env(), &o)
    };
    assert!(matches!(
        run("mode", OptionValue::Enum("c".into())).unwrap_err(),
        ResolveError::OptionNotInEnum { .. }
    ));
    assert!(matches!(
        run("mode", OptionValue::Int(1)).unwrap_err(),
        ResolveError::OptionType { .. }
    ));
    assert!(matches!(
        run("on", OptionValue::Enum("true".into())).unwrap_err(),
        ResolveError::OptionType { .. }
    ));
    let set = run("on", OptionValue::Bool(true)).unwrap();
    assert_eq!(
        set.mods[0].options,
        [
            ("mode".into(), OptionValue::Enum("a".into())),
            ("on".into(), OptionValue::Bool(true))
        ]
    );
}

#[test]
fn phase_and_event_failures() {
    let m = |id: &str, md: Value| {
        pkg(
            id,
            json!({ "kind": ["module"], "content": [], "modules": [md] }),
        )
    };
    let e = fail(&[m(
        "a",
        module("m.wasm", json!({ "phase": { "after": "rain" } })),
    )]);
    assert!(matches!(e, ResolveError::UnknownPhase { .. }));
    let e = fail(&[m(
        "a",
        module(
            "m.wasm",
            json!({ "phase": { "after": "economy", "before": "wind" } }),
        ),
    )]);
    assert_eq!(
        e.to_string(),
        "a: m.wasm asks for a phase after economy and before wind, but economy runs later"
    );
    let publisher = |id: &str| {
        m(
            id,
            module(
                "m.wasm",
                json!({ "implements": "rain@1", "events": { "publish": ["Drop"] } }),
            ),
        )
    };
    let e = fail(&[
        publisher("a"),
        m(
            "b",
            module(
                "n.wasm",
                json!({ "implements": "economy@5", "events": { "publish": ["Drop"] } }),
            ),
        ),
        m(
            "c",
            module(
                "n.wasm",
                json!({ "implements": "rain@1", "events": { "publish": ["Drop"] } }),
            ),
        ),
    ]);
    assert!(matches!(e, ResolveError::DuplicateImplementation { .. }));
    let e = fail(&[
        m(
            "a",
            module("m.wasm", json!({ "events": { "publish": ["Drop"] } })),
        ),
        m(
            "b",
            module(
                "n.wasm",
                json!({ "implements": "a@1", "events": { "publish": ["Drop"] } }),
            ),
        ),
    ]);
    assert_eq!(
        e,
        ResolveError::DuplicateEvent {
            name: "a.Drop".into(),
            first: "a".into(),
            second: "b".into()
        }
    );
    let e = fail(&[
        publisher("a"),
        m(
            "b",
            module(
                "n.wasm",
                json!({ "events": { "subscribe": ["rain.Splash"] } }),
            ),
        ),
    ]);
    assert!(matches!(e, ResolveError::UnknownEvent { .. }));
    let e = fail(&[pkg(
        "hud",
        json!({ "kind": ["presentation"], "content": [],
                "presentation": [{ "file": "h.wasm", "handles": ["Drop"] }] }),
    )]);
    assert!(matches!(e, ResolveError::UnknownEvent { .. }));
    let e = fail(&[pkg(
        "two",
        json!({ "kind": ["module", "presentation"], "content": [],
                "modules": [
                    module("a.wasm", json!({ "implements": "one_ns@1", "events": { "publish": ["Tick"] } })),
                    module("b.wasm", json!({ "implements": "two_ns@1", "events": { "publish": ["Tick"] } }))
                ],
                "presentation": [{ "file": "h.wasm", "handles": ["Tick"] }] }),
    )]);
    assert!(matches!(e, ResolveError::AmbiguousEvent { .. }));
}

#[test]
fn state_budget() {
    let big = |id: &str| {
        pkg(
            id,
            json!({ "kind": ["module"], "content": [],
                    "modules": [module("m.wasm", json!({ "state_bytes": 200 * 1024 * 1024 }))] }),
        )
    };
    ok(&[big("a")]);
    assert_eq!(fail(&[big("a"), big("b")]), ResolveError::StateOverflow);
}

#[test]
fn invalid_environment() {
    let mut e = env();
    e.phases.push("wind".into());
    assert!(matches!(
        resolve(&[], &e, &no_options()).unwrap_err(),
        ResolveError::InvalidEnvironment(_)
    ));
    let mut e = env();
    e.interfaces.insert("Bad".into(), "1.0.0".parse().unwrap());
    assert!(matches!(
        resolve(&[], &e, &no_options()).unwrap_err(),
        ResolveError::InvalidEnvironment(_)
    ));
}

#[test]
fn lobby_conversion() {
    let set = ok(&weather_world());
    let lobby = set.to_lobby().unwrap();
    assert_eq!(lobby.hash, set.hash.0);
    let ids: Vec<&str> = lobby.mods.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        ["community-patch", "weather-storms", "storm-insurance"]
    );
    assert_eq!(lobby.mods[1].version, "1.2.0");
    assert_eq!(lobby.mods[1].hash, set.mods[1].digest.0);

    let long = pkg(
        "io.github.someone.a-very-long-mod",
        json!({ "version": "1.0.0-beta.1+build.2024" }),
    );
    let lobby = ok(&[long]).to_lobby().unwrap();
    assert_eq!(lobby.mods[0].id.len(), 24);
    assert!(lobby.mods[0].id.starts_with("io.github.someo~"));
    assert_eq!(lobby.mods[0].version, "1.0.0-beta.1");

    let many: Vec<ModPackage> = (0..=MAX_MODS)
        .map(|i| pkg(&format!("m{i}"), json!({})))
        .collect();
    assert_eq!(
        ok(&many).to_lobby().unwrap_err(),
        LobbyError::TooMany(MAX_MODS + 1)
    );
    assert!(ok(&many[..MAX_MODS]).to_lobby().is_ok());
}

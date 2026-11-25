#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use btm_mod::manifest::{Kind, OptionSpec, Problem};
use btm_mod::{Manifest, ManifestError};
use serde_json::{json, Value};

const SHA: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

/// The plan's section 2.2 example, as real JSON.
fn example() -> Value {
    json!({
        "manifest": 1,
        "id": "weather-storms",
        "version": "1.2.0",
        "kind": ["module", "presentation"],
        "engine": ">=0.4 <0.5",
        "requires": { "wind_meteors": "^3" },
        "after": ["community-patch"], "before": [],
        "conflicts": ["sunny-skies"],
        "content": ["data/*.hpi"],
        "profile": "oamod.yaml",
        "modules": [{
            "file": "storms.wasm", "sha256": SHA,
            "implements": "weather@1",
            "wraps": ["wind_meteors.UpdateWind"],
            "phase": { "after": "wind", "before": "meteors" },
            "state_bytes": 65536,
            "events": { "publish": ["StormStarted", "LightningStrike"],
                        "subscribe": ["economy.WindChanged"] }
        }],
        "presentation": [{ "file": "storms_fx.wasm", "handles": ["LightningStrike"] }],
        "options": { "storm_frequency": { "type": "int", "min": 0, "max": 10, "default": 3 } }
    })
}

fn parse(v: &Value) -> Result<Manifest, ManifestError> {
    Manifest::parse(&v.to_string())
}

/// Sets (or adds) the value at a JSON pointer.
fn set(mut v: Value, pointer: &str, new: Value) -> Value {
    let (parent, last) = pointer.rsplit_once('/').unwrap();
    match v.pointer_mut(parent).unwrap() {
        Value::Object(m) => {
            m.insert(last.to_owned(), new);
        }
        Value::Array(a) => {
            let i: usize = last.parse().unwrap();
            if i == a.len() {
                a.push(new);
            } else {
                a[i] = new;
            }
        }
        _ => panic!("bad pointer {pointer}"),
    }
    v
}

fn remove(mut v: Value, pointer: &str) -> Value {
    let (parent, last) = pointer.rsplit_once('/').unwrap();
    v.pointer_mut(parent)
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove(last)
        .unwrap();
    v
}

/// Parses, expects an error at `path`, and returns its problem.
fn problem(v: Value, path: &str) -> Problem {
    let e = parse(&v).expect_err("expected an error");
    assert_eq!(e.path(), Some(path), "wrong path in: {e}");
    e.problem().unwrap().clone()
}

#[test]
fn the_plan_example_parses() {
    let m = parse(&example()).unwrap();
    assert_eq!(m.id, "weather-storms");
    assert_eq!(m.version.to_string(), "1.2.0");
    assert_eq!(
        m.kind.iter().copied().collect::<Vec<_>>(),
        [Kind::Module, Kind::Presentation]
    );
    assert_eq!(m.engine.to_string(), ">=0.4, <0.5");
    assert_eq!(m.requires["wind_meteors"].to_string(), "^3");
    assert_eq!(m.after, ["community-patch"]);
    assert!(m.before.is_empty());
    assert_eq!(m.conflicts, ["sunny-skies"]);
    assert_eq!(m.content, ["data/*.hpi"]);
    assert_eq!(m.profile.as_deref(), Some("oamod.yaml"));
    let md = &m.modules[0];
    assert_eq!(md.file, "storms.wasm");
    assert_eq!(md.sha256.to_hex(), SHA);
    assert_eq!(md.implements.as_ref().unwrap().to_string(), "weather@1");
    assert_eq!(md.wraps[0].to_string(), "wind_meteors.UpdateWind");
    let ph = md.phase.as_ref().unwrap();
    assert_eq!(
        (ph.after.as_deref(), ph.before.as_deref()),
        (Some("wind"), Some("meteors"))
    );
    assert_eq!(md.state_bytes, Some(65536));
    assert_eq!(md.publish, ["StormStarted", "LightningStrike"]);
    assert_eq!(md.subscribe[0].to_string(), "economy.WindChanged");
    assert_eq!(m.presentation[0].file, "storms_fx.wasm");
    assert_eq!(m.presentation[0].sha256, None);
    assert_eq!(m.presentation[0].handles[0].to_string(), "LightningStrike");
    assert_eq!(
        m.options["storm_frequency"],
        OptionSpec::Int {
            min: Some(0),
            max: Some(10),
            default: 3
        }
    );
    assert!(m.is_synced());
}

#[test]
fn round_trips() {
    let mut variants = vec![example()];
    variants.push(set(
        example(),
        "/options",
        json!({
            "fast": { "type": "bool", "default": true },
            "mode": { "type": "enum", "values": ["calm", "wild"], "default": "wild" },
            "open": { "type": "int", "default": -4 }
        }),
    ));
    variants.push(json!({
        "manifest": 1, "id": "io.github.alice.hud", "version": "0.1.0-beta.2+build.5",
        "kind": ["presentation"], "engine": "*",
        "presentation": [{ "file": "ui/hud.wasm", "sha256": SHA,
                           "handles": ["weather.LightningStrike"] }]
    }));
    variants.push(json!({
        "manifest": 1, "id": "rules", "version": "2.0.0", "kind": ["script"],
        "engine": "^0.4",
        "modules": [{ "file": "rules.lua", "sha256": SHA, "implements": "rules@2.1",
                      "phase": { "before": "economy" } }]
    }));
    for v in variants {
        let m = parse(&v).unwrap();
        let text = m.to_json();
        let back = Manifest::parse(&text).unwrap();
        assert_eq!(back, m, "{text}");
        assert_eq!(back.to_json(), text, "writing is stable");
    }
}

#[test]
fn presentation_only_is_not_synced() {
    let v = json!({
        "manifest": 1, "id": "hud", "version": "0.1.0", "kind": ["presentation"],
        "engine": "*", "presentation": [{ "file": "hud.wasm" }]
    });
    assert!(!parse(&v).unwrap().is_synced());
}

#[test]
fn json_syntax_errors() {
    for bad in [
        "{ // comment\n \"manifest\": 1 }",
        "{ /* comment */ }",
        "{\"manifest\": 1,}",
        "{\"manifest\": 1} trailing",
        "",
    ] {
        let e = Manifest::parse(bad).unwrap_err();
        assert!(matches!(e, ManifestError::Syntax(_)), "{bad:?}: {e}");
        assert!(e.to_string().contains("plain JSON"), "{e}");
    }
    let e = Manifest::parse(r#"{"manifest": 1, "id": "a", "id": "b"}"#).unwrap_err();
    assert!(e.to_string().contains("$: duplicate key \"id\""), "{e}");
    let e = Manifest::parse(r#"{"modules": [{"file": "a", "file": "b"}]}"#).unwrap_err();
    assert!(
        e.to_string()
            .contains("$.modules[0]: duplicate key \"file\""),
        "{e}"
    );
}

#[test]
fn unknown_fields_are_rejected_everywhere() {
    let cases = [
        ("/colour", "$.colour"),
        ("/modules/0/colour", "$.modules[0].colour"),
        ("/modules/0/phase/during", "$.modules[0].phase.during"),
        ("/modules/0/events/route", "$.modules[0].events.route"),
        ("/presentation/0/colour", "$.presentation[0].colour"),
        (
            "/options/storm_frequency/step",
            "$.options.storm_frequency.step",
        ),
    ];
    for (pointer, path) in cases {
        let p = problem(set(example(), pointer, json!(1)), path);
        assert!(matches!(p, Problem::UnknownField { .. }), "{p}");
    }
    // A field of another option type is unknown for this one.
    let v = set(
        example(),
        "/options/flag",
        json!({ "type": "bool", "default": true, "min": 0 }),
    );
    let p = problem(v, "$.options.flag.min");
    assert_eq!(
        p.to_string(),
        "unknown field \"min\" (expected one of: type, default)"
    );
    // Keys that are not identifiers are quoted in the path.
    let p = problem(set(example(), "/a b", json!(1)), "$[\"a b\"]");
    assert!(matches!(p, Problem::UnknownField { .. }));
}

#[test]
fn missing_fields() {
    for (pointer, path, field) in [
        ("/manifest", "$", "manifest"),
        ("/id", "$", "id"),
        ("/version", "$", "version"),
        ("/kind", "$", "kind"),
        ("/engine", "$", "engine"),
        ("/modules/0/file", "$.modules[0]", "file"),
        ("/modules/0/sha256", "$.modules[0]", "sha256"),
        ("/presentation/0/file", "$.presentation[0]", "file"),
        (
            "/options/storm_frequency/type",
            "$.options.storm_frequency",
            "type",
        ),
        (
            "/options/storm_frequency/default",
            "$.options.storm_frequency",
            "default",
        ),
    ] {
        assert_eq!(
            problem(remove(example(), pointer), path),
            Problem::MissingField(field)
        );
    }
    let v = set(
        example(),
        "/options/m",
        json!({ "type": "enum", "default": "a" }),
    );
    assert_eq!(problem(v, "$.options.m"), Problem::MissingField("values"));
}

#[test]
fn wrong_types() {
    for (pointer, value, path) in [
        ("/id", json!(5), "$.id"),
        ("/kind", json!("module"), "$.kind"),
        ("/kind/0", json!(1), "$.kind[0]"),
        ("/requires", json!(["wind_meteors"]), "$.requires"),
        (
            "/requires/wind_meteors",
            json!(3),
            "$.requires.wind_meteors",
        ),
        ("/modules", json!({}), "$.modules"),
        ("/modules/0", json!("storms.wasm"), "$.modules[0]"),
        ("/modules/0/phase", json!("wind"), "$.modules[0].phase"),
        ("/after", json!("community-patch"), "$.after"),
        ("/options", json!([]), "$.options"),
        (
            "/options/storm_frequency/default",
            json!("3"),
            "$.options.storm_frequency.default",
        ),
        (
            "/options/storm_frequency/default",
            json!(3.5),
            "$.options.storm_frequency.default",
        ),
        (
            "/modules/0/state_bytes",
            json!(1e3),
            "$.modules[0].state_bytes",
        ),
        ("/manifest", json!("1"), "$.manifest"),
    ] {
        let p = problem(set(example(), pointer, value), path);
        assert!(matches!(p, Problem::WrongType { .. }), "{path}: {p}");
    }
    let p = problem(json!([]), "$");
    assert_eq!(p.to_string(), "expected an object, found an array");
}

#[test]
fn manifest_version() {
    let p = problem(set(example(), "/manifest", json!(2)), "$.manifest");
    assert_eq!(p, Problem::UnsupportedVersion("2".into()));
}

#[test]
fn ids() {
    for bad in ["Weather", "weather_storms", "-x", "a--b", "io..x", ""] {
        let p = problem(set(example(), "/id", json!(bad)), "$.id");
        assert!(matches!(p, Problem::BadModId(_)), "{bad}: {p}");
    }
    for ok in ["io.github.alice.storms", "x", "storms2"] {
        parse(&set(example(), "/id", json!(ok))).unwrap();
    }
    let p = problem(set(example(), "/after/0", json!("Bad")), "$.after[0]");
    assert!(matches!(p, Problem::BadModId(_)));
    let p = problem(
        set(example(), "/conflicts/1", json!("x y")),
        "$.conflicts[1]",
    );
    assert!(matches!(p, Problem::BadModId(_)));
    let p = problem(
        set(example(), "/requires/Bad Name", json!("^1")),
        "$.requires[\"Bad Name\"]",
    );
    assert!(matches!(p, Problem::BadRequirement(_)));
}

#[test]
fn versions_and_ranges() {
    for bad in ["1.2", "v1.2.0", "1.2.0.0", "01.2.0"] {
        let p = problem(set(example(), "/version", json!(bad)), "$.version");
        assert!(matches!(p, Problem::BadVersion { .. }), "{bad}");
    }
    for bad in ["", "^x", ">=1 || <0.5", "1.0 - 2.0"] {
        let p = problem(set(example(), "/engine", json!(bad)), "$.engine");
        assert!(matches!(p, Problem::BadRange { .. }), "{bad}");
    }
    let p = problem(
        set(example(), "/requires/wind_meteors", json!("three")),
        "$.requires.wind_meteors",
    );
    assert!(matches!(p, Problem::BadRange { .. }));
}

#[test]
fn digests_paths_and_globs() {
    for bad in ["abc", &SHA.to_uppercase(), &format!("{SHA}0")] {
        let p = problem(
            set(example(), "/modules/0/sha256", json!(bad)),
            "$.modules[0].sha256",
        );
        assert!(matches!(p, Problem::BadDigest(_)));
    }
    let p = problem(
        set(example(), "/presentation/0/sha256", json!("x")),
        "$.presentation[0].sha256",
    );
    assert!(matches!(p, Problem::BadDigest(_)));
    for bad in [
        "../storms.wasm",
        "/storms.wasm",
        "a\\b.wasm",
        "c:/x.wasm",
        "*.wasm",
        "a//b.wasm",
    ] {
        let p = problem(
            set(example(), "/modules/0/file", json!(bad)),
            "$.modules[0].file",
        );
        assert!(matches!(p, Problem::BadPath { .. }), "{bad}: {p}");
    }
    let p = problem(
        set(example(), "/profile", json!("../oamod.yaml")),
        "$.profile",
    );
    assert!(matches!(p, Problem::BadPath { .. }));
    let p = problem(
        set(example(), "/content/0", json!("data/[.hpi")),
        "$.content[0]",
    );
    assert!(matches!(p, Problem::BadGlob { .. }), "{p}");
    let p = problem(
        set(example(), "/content/1", json!("../*.hpi")),
        "$.content[1]",
    );
    assert!(matches!(p, Problem::BadPath { .. }));
    let p = problem(
        set(example(), "/modules/0/file", json!("storms.dll")),
        "$.modules[0].file",
    );
    assert_eq!(p, Problem::BadModuleFile);
    let p = problem(
        set(example(), "/modules/0/file", json!("storms.lua")),
        "$.modules[0].file",
    );
    assert_eq!(p, Problem::LuaNeedsScript);
}

#[test]
fn kinds() {
    let p = problem(set(example(), "/kind/2", json!("mutator")), "$.kind[2]");
    assert!(matches!(p, Problem::UnknownKind(_)));
    let p = problem(set(example(), "/kind", json!([])), "$.kind");
    assert_eq!(p, Problem::Empty);
    let p = problem(set(example(), "/kind/2", json!("module")), "$.kind[2]");
    assert!(matches!(p, Problem::Duplicate(_)));
    // Content and a profile may ride along with any kind (as in the plan's example), and
    // may also be declared.
    let all = json!(["module", "presentation", "content", "profile"]);
    parse(&set(example(), "/kind", all.clone())).unwrap();
    // Modules and presentation decide what is synced, so they need their kind.
    let v = set(example(), "/kind", json!(["presentation"]));
    assert_eq!(
        problem(v, "$.modules"),
        Problem::FieldNeedsKind {
            field: "modules",
            kinds: "`module` or `script`"
        }
    );
    let v = set(example(), "/kind", json!(["module"]));
    assert!(matches!(
        problem(v, "$.presentation"),
        Problem::FieldNeedsKind { .. }
    ));
    // Each declared kind needs its field.
    for (kind, field, drop) in [
        (Kind::Module, "modules", "/modules"),
        (Kind::Presentation, "presentation", "/presentation"),
        (Kind::Content, "content", "/content"),
        (Kind::Profile, "profile", "/profile"),
    ] {
        let v = remove(set(example(), "/kind", all.clone()), drop);
        assert_eq!(
            problem(v, "$.kind"),
            Problem::KindNeedsField { kind, field },
            "{field}"
        );
    }
    let v = json!({ "manifest": 1, "id": "pack", "version": "1.0.0", "kind": ["ruleset"],
                    "engine": "*" });
    assert_eq!(
        problem(v, "$.kind"),
        Problem::KindNeedsField {
            kind: Kind::Ruleset,
            field: "requires"
        }
    );
}

fn full_kinds() -> Value {
    set(
        example(),
        "/kind",
        json!(["module", "presentation", "content", "profile"]),
    )
}

#[test]
fn ordering_lists() {
    let p = problem(
        set(full_kinds(), "/after/1", json!("community-patch")),
        "$.after[1]",
    );
    assert!(matches!(p, Problem::Duplicate(_)));
    let p = problem(
        set(full_kinds(), "/before/0", json!("weather-storms")),
        "$.before[0]",
    );
    assert!(matches!(p, Problem::SelfReference(_)));
    let p = problem(
        set(full_kinds(), "/requires/weather-storms", json!("^1")),
        "$.requires[\"weather-storms\"]",
    );
    assert!(matches!(p, Problem::SelfReference(_)));
    let p = problem(
        set(full_kinds(), "/before/0", json!("community-patch")),
        "$.before[0]",
    );
    assert!(matches!(p, Problem::AfterAndBefore(_)));
    let v = set(full_kinds(), "/requires/sunny-skies", json!("*"));
    let p = problem(v, "$.conflicts[0]");
    assert!(matches!(p, Problem::RequiresConflict(_)));
}

#[test]
fn modules() {
    let base = full_kinds();
    for bad in ["weather", "Weather@1", "weather@x", "weather@1.2.3.4", "@1"] {
        let p = problem(
            set(base.clone(), "/modules/0/implements", json!(bad)),
            "$.modules[0].implements",
        );
        assert!(matches!(p, Problem::BadInterfaceRef(_)), "{bad}");
    }
    for bad in ["UpdateWind", "wind_meteors.1x", "Wind.UpdateWind"] {
        let p = problem(
            set(base.clone(), "/modules/0/wraps/0", json!(bad)),
            "$.modules[0].wraps[0]",
        );
        assert!(matches!(p, Problem::BadSlotRef(_)), "{bad}");
    }
    let p = problem(
        set(base.clone(), "/modules/0/phase", json!({})),
        "$.modules[0].phase",
    );
    assert_eq!(p, Problem::EmptyPhase);
    let p = problem(
        set(base.clone(), "/modules/0/phase/before", json!("wind")),
        "$.modules[0].phase.before",
    );
    assert!(matches!(p, Problem::SamePhase(_)));
    let p = problem(
        set(base.clone(), "/modules/0/phase/after", json!("Wind")),
        "$.modules[0].phase.after",
    );
    assert!(matches!(p, Problem::BadName { .. }));
    for bad in [json!(0), json!(268_435_457)] {
        let p = problem(
            set(base.clone(), "/modules/0/state_bytes", bad),
            "$.modules[0].state_bytes",
        );
        assert!(matches!(p, Problem::OutOfRange { .. }));
    }
    let p = problem(
        set(
            base.clone(),
            "/modules/0/events/publish/0",
            json!("stormStarted"),
        ),
        "$.modules[0].events.publish[0]",
    );
    assert!(matches!(p, Problem::BadName { .. }));
    let p = problem(
        set(
            base.clone(),
            "/modules/0/events/publish/2",
            json!("StormStarted"),
        ),
        "$.modules[0].events.publish[2]",
    );
    assert!(matches!(p, Problem::Duplicate(_)));
    for bad in ["WindChanged", "economy.windChanged", "Economy.WindChanged"] {
        let p = problem(
            set(base.clone(), "/modules/0/events/subscribe/0", json!(bad)),
            "$.modules[0].events.subscribe[0]",
        );
        assert!(matches!(p, Problem::BadEventRef(_)), "{bad}");
    }
    let p = problem(
        set(
            base.clone(),
            "/presentation/0/handles/0",
            json!("lightning"),
        ),
        "$.presentation[0].handles[0]",
    );
    assert!(matches!(p, Problem::BadEventRef(_)));
    let second = json!({ "file": "storms.wasm", "sha256": SHA });
    let p = problem(set(base.clone(), "/modules/1", second), "$.modules[1].file");
    assert!(matches!(p, Problem::Duplicate(_)));
    let second = json!({ "file": "more.wasm", "sha256": SHA, "implements": "weather@1" });
    let p = problem(
        set(base.clone(), "/modules/1", second),
        "$.modules[1].implements",
    );
    assert!(matches!(p, Problem::DuplicateImplements(_)));
    let p = problem(
        set(base, "/presentation/0/file", json!("storms.wasm")),
        "$.presentation[0].file",
    );
    assert!(matches!(p, Problem::Duplicate(_)));
}

#[test]
fn options() {
    let base = full_kinds();
    let opt = |o: Value| set(base.clone(), "/options/o", o);
    let p = problem(
        opt(json!({ "type": "float", "default": 1 })),
        "$.options.o.type",
    );
    assert!(matches!(p, Problem::UnknownOptionType(_)));
    let p = problem(
        opt(json!({ "type": "int", "min": 5, "max": 1, "default": 3 })),
        "$.options.o.max",
    );
    assert_eq!(p, Problem::MinAboveMax { min: 5, max: 1 });
    let p = problem(
        opt(json!({ "type": "int", "min": 0, "max": 10, "default": 11 })),
        "$.options.o.default",
    );
    assert_eq!(
        p,
        Problem::DefaultOutOfBounds {
            default: 11,
            min: 0,
            max: 10
        }
    );
    let p = problem(
        opt(json!({ "type": "int", "min": 0, "default": -1 })),
        "$.options.o.default",
    );
    assert!(matches!(p, Problem::DefaultOutOfBounds { .. }));
    let p = problem(
        opt(json!({ "type": "int", "default": 9_223_372_036_854_775_808u64 })),
        "$.options.o.default",
    );
    assert!(matches!(p, Problem::OutOfRange { .. }));
    let p = problem(
        opt(json!({ "type": "bool", "default": "yes" })),
        "$.options.o.default",
    );
    assert!(matches!(p, Problem::WrongType { .. }));
    let p = problem(
        opt(json!({ "type": "enum", "values": ["a", "b"], "default": "c" })),
        "$.options.o.default",
    );
    assert!(matches!(p, Problem::DefaultNotInValues { .. }));
    let p = problem(
        opt(json!({ "type": "enum", "values": [], "default": "c" })),
        "$.options.o.values",
    );
    assert_eq!(p, Problem::Empty);
    let p = problem(
        opt(json!({ "type": "enum", "values": ["a", "a"], "default": "a" })),
        "$.options.o.values[1]",
    );
    assert!(matches!(p, Problem::Duplicate(_)));
    let p = problem(
        opt(json!({ "type": "enum", "values": ["a b"], "default": "a b" })),
        "$.options.o.values[0]",
    );
    assert!(matches!(p, Problem::BadName { .. }));
    let p = problem(
        set(
            base.clone(),
            "/options/Storm",
            json!({ "type": "bool", "default": true }),
        ),
        "$.options.Storm",
    );
    assert!(matches!(p, Problem::BadName { .. }));
    let p = problem(opt(json!(3)), "$.options.o");
    assert!(matches!(p, Problem::WrongType { .. }));
}

#[test]
fn errors_read_well() {
    let e = parse(&set(full_kinds(), "/modules/0/phase/after", json!("Wind"))).unwrap_err();
    assert_eq!(
        e.to_string(),
        "$.modules[0].phase.after: \"Wind\" is not a valid phase name (lowercase letters, digits and underscores, starting with a letter)"
    );
}

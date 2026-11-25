# btm-mod

Mod tooling for Byte Tactics ([ozgb/btm](https://github.com/ozgb/btm)):

- **The `btm_mod.json` v1 manifest.** A typed model with strict validation: unknown fields are rejected, and every error names its JSON path.
- **The resolver.** It orders a mod set deterministically and fails fast, with a clear message, on a missing requirement, a version mismatch, a cycle, a conflict or an engine mismatch. It produces the resolved mod set and its hash, which the lobby, saves and replays compare.
- **The `btm-mod` CLI:** `new`, `check`, `resolve` and `preview`.

The design follows sections 2.1 to 2.6 of the Byte Tactics modding plan. MIT.

## The manifest

`btm_mod.json` is **plain JSON**. The plan's example is written as `jsonc` only so that it
can carry explanatory comments. Comments, trailing commas and duplicate keys are all errors
in a real file. The plan's example, minus its comments, is a valid manifest; see
`tests/manifest.rs`.

| Field | Meaning |
|---|---|
| `manifest` | Always `1` |
| `id` | Registry name (`weather-storms`) or reverse-DNS (`io.github.alice.storms`) |
| `version` | Semver |
| `kind` | Any of `content`, `profile`, `script`, `module`, `presentation`, `ruleset` |
| `engine` | Semver range; `">=0.4 <0.5"` and `">=0.4, <0.5"` are both accepted |
| `requires` | Interface name or mod id, mapped to a range |
| `after`, `before`, `conflicts` | Mod ids |
| `content` | Globs relative to the mod directory |
| `profile` | Path to an `oamod.yaml` |
| `modules[]` | `file` (`.wasm`, or `.lua` for scripts), `sha256`, `implements` (`name@major`), `wraps` (`interface.Slot`), `phase` (`after` and `before` built-in phases), `state_bytes`, and `events` (`publish` names and `subscribe` `namespace.Name`s) |
| `presentation[]` | `file`, optional `sha256`, `handles` |
| `options` | `{ "type": "int", "min", "max", "default" }`, `{ "type": "bool", "default" }`, or `{ "type": "enum", "values", "default" }` |

A declared kind needs its field. `modules` needs kind `module` or `script`, and
`presentation` needs kind `presentation`. `content` and `profile` may come with any kind.

## The resolved mod set

[`docs/resolved-set.md`](docs/resolved-set.md) specifies:

- the resolution rules;
- what is and isn't hashed (presentation is optional per player, so it is never hashed);
- the canonical byte encoding behind the hash;
- the conversion to the netplay lobby's mod list (16 mods, 24-byte ids, 12-byte versions).

```rust
let set = btm_mod::resolve(&packages, &environment, &option_values)?;
println!("{}", set.hash);
let lobby = set.to_lobby()?; // the arguments of btm_netplay::session::ModSet::new
```

`btm_mod::fs::load_package(dir)` reads a mod directory, giving its manifest and file
digests. The resolver itself does no IO.

## The CLI

The plan's `btm mod <cmd>` is `btm-mod <cmd>`.

```text
btm-mod new <kind> <dir> [--id <id>]
btm-mod check <dir> [--update-hashes]
btm-mod resolve <dir>... --engine <version> [--interface <name>=<version>]...
                [--phases <a,b,...>] [--option <mod>.<name>=<value>]... [--json]
btm-mod preview <dir>...
```

- **`new`** writes a template. A `module` or `presentation` mod is a minimal Rust
  `cdylib` crate, a `script` is Lua, and `content`, `profile` and `ruleset` are data only.
- **`check`** validates the manifest, hashes every referenced file and checks every
  `sha256`. Pass `--update-hashes` after a rebuild. The WASM ABI and fuel checks from
  `tools/wasm_modules.py` are not part of it yet.
- **`resolve`** prints the load order, options, phases, event numbering, state layout,
  the hash and whether the set fits the lobby, or else the first error. `--json` prints
  the same as JSON.
- **`preview`** is a stub (exit code 3). A diff of changed definition values needs the
  game's data, so it waits for M3.3.

The exit codes are 0 for success, 1 for a failed check or resolve, 2 for bad usage and 3
for not implemented.

## Development

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

The crate has no `unsafe` code and no `unwrap`, `expect` or `panic` outside tests (all
enforced by lints), and its errors are typed. The dependencies are `serde`, `serde_json`,
`semver`, `sha2`, `glob` and `thiserror`, all MIT or Apache-2.0, and they are pinned in
`Cargo.lock`.

# The resolved mod set and its hash

This is the specification of what `btm_mod::resolve` produces, and of the canonical
encoding that the resolved mod-set hash is taken over. The hash gates the lobby, goes into
the sync hash and is stored in saves and replays (modding plan, section 2.2). The launcher,
engine and CI must all use this code, or an implementation that produces the same bytes.

The format is versioned by its domain tag, `btm-mod/resolved-set/v1`. Any change to the
bytes below is a new version with a new tag. The test `hash_is_stable` in
`tests/resolve.rs` pins one hash so that an accidental change fails CI.

## Inputs

- **Mods.** For each mod, its `btm_mod.json` and the SHA-256 of every file it references,
  keyed by its path relative to the mod directory, with `/` as the separator.
- **The environment.** The engine version, the built-in interfaces with their versions,
  and the built-in tick phases in order.
- **Option values.** These are given per mod and option. Any option left out takes its
  default.

The order the mods are given in never matters, nor does the order of the entries in any
map. Checks run in a fixed order, with mods taken by id, so the same bad input always gives
the same first error.

## Resolution

1. **Duplicates.** Each mod id may appear once.
2. **Engine.** Every mod's `engine` range must match the engine version.
3. **Interfaces.** A synced module's `implements: "name@v"` either replaces a built-in
   interface, in which case the major versions must match, or adds a new interface at
   version `v`. One interface may have only one implementation in the set.
4. **Requirements.** Each `requires` key names an interface (built-in or new) or a mod, and
   its version must satisfy the range. A key that is both an interface and a mod is an
   error. A synced mod may not require a presentation-only mod.
5. **Conflicts.** If a mod and one of its `conflicts` are both present, that is an error.
6. **Wraps.** Each wrapped interface must exist.
7. **Order.** The synced mods are sorted topologically, with ties broken by taking the
   smallest ready id (byte order). The edges are:
   - each required mod comes before the mod that requires it;
   - the mod that provides a new interface comes before any mod that requires or wraps it;
   - `after` and `before` add edges to mods that are present (absent ones are ignored).

   Presentation-only mods are then sorted the same way among themselves and placed after
   all synced mods. Ordering constraints between the two groups are ignored, so a player's
   optional HUD can never change the synced order. A cycle is reported as
   `a -> c -> b -> a`, starting at the smallest id that could not be placed.
8. **Files.** Every module, presentation and profile file must be present and match any
   declared `sha256`. Every content glob must match at least one file. Globs are
   case-sensitive, and `*` does not cross `/` (use `**` for that).
9. **Options.** Values must name a mod and option that exist, and must have the declared
   type and lie within `min..=max` or among the enum `values`.
10. **Phases.** A module with a `phase` gets the phase id `<mod id>/<file>`. It goes in the
    gap right after its `after` phase, or right before its `before` phase when it gives
    only `before`. If it gives both, `after` must come earlier than `before`. In v1,
    `after` and `before` name built-in phases only. Within a gap, phases follow the
    resolved mod order and then module order, so `after` and `before` between mods also
    order their phases.
11. **Event kinds** are numbered at resolve, per open decision 2 of section 2.6. An event's
    full name is `<namespace>.<Name>`, where the namespace is the module's `implements`
    interface if it has one, and the mod id otherwise. Numbers run 0, 1, 2 and so on, in
    resolved mod order, then module order, then `publish` order. The engine maps these
    numbers onto its range of open kinds. A full name may be published only once. A
    subscription or `handles` entry resolves to a mod kind's number. Failing that, it
    resolves to a built-in kind when its namespace is a built-in interface; otherwise it is
    an error. A bare `Name` in `handles` means one of the mod's own events.
12. **State.** Each module with `state_bytes` gets a region of the mod-state area, in
    resolved order, with each offset aligned to 64 bytes. The total may not exceed
    256 MiB.

Only **synced** mods are hashed. A mod is synced unless its only kind is `presentation` and
it ships no content, profile or modules (open decision 3). The presentation files of a
synced mod are not hashed either.

## Canonical encoding

All integers are little-endian. `str` is a `u32` byte length followed by UTF-8 bytes.
`list<T>` is a `u32` count followed by the items. `opt<str>` is the byte `0`, or the byte
`1` followed by a `str`. `digest` is 32 raw bytes.

```text
set         = str("btm-mod/resolved-set/v1")
              list<mod_record>          synced mods, in resolved order
              list<str>                 every phase id, in tick order (built-in and mod)
              list<event>               in number order
              list<region>              in offset order

mod_record  = str(id)
              str(version)              semver, as written by the `semver` crate
              list<file>                role != presentation, sorted by (role, path)
              list<module>              in manifest order
              list<option>              sorted by name (byte order)

file        = u8(role)                  0 content, 1 profile, 2 module
              str(path)
              digest(sha256)

module      = str(file)
              opt<str>(implements)      "name@major.minor.patch"
              list<str>(wraps)          "interface.Slot", sorted
              opt<str>(phase id)
              u64(state_bytes)          0 when absent
              list<str>(publish)        full names, in declaration order
              list<str>(subscribe)      full names, sorted

option      = str(name) u8(type) value
              type 0 int:  i64
              type 1 bool: u8 (0 or 1)
              type 2 enum: str

event       = u32(number) str(full name) str(publisher mod id)

region      = str(mod id) str(file) u64(offset) u64(length)
```

- **Resolved mod-set hash:** `SHA-256(set)`.
- **Per-mod digest:** `SHA-256(str("btm-mod/mod/v1") || mod_record)`. It identifies one mod,
  covering its id, version, synced files, modules and option values, and it is what the
  lobby compares per mod.

With no mods and no built-in phases, `set` is the tag followed by four zero counts: 16 zero
bytes. The test `empty_set_encoding_matches_the_spec` checks this.

### What is deliberately not in the hash

- **The engine version and the built-in interface versions.** The netplay layer already
  refuses mismatched builds, and leaving them out keeps the hash about the mods. The
  built-in phase list does go in, as part of the phase order.
- **Presentation files and presentation-only mods.** They are optional per player.
- **Manifest formatting.** Only the resolved values above count, so reformatting a
  `btm_mod.json` doesn't change the hash.

### Not yet covered

These parts of the resolved set are listed in section 2.2 but have no inputs yet:

- quirk flags and the scheduler choice, which arrive with profiles in M3.1;
- the window and table-range part of the address layout, which arrives in M3.4.

Adding any of them means a new version of the encoding.

## Lobby mod list

`ResolvedSet::to_lobby` gives the arguments of `btm_netplay::session::ModSet::new`:

- **The hash:** the resolved mod-set hash.
- **One entry per synced mod,** in resolved order: the id, the version, and the mod's digest.

The lobby's caps apply:

- **At most 16 entries.** More is an error.
- **Ids of at most 24 bytes.** A longer id becomes its first 15 bytes, then `~`, then the
  first 8 hex digits of SHA-256 of the full id.
- **Versions of at most 12 bytes.** A longer version first loses its `+build` metadata. If
  it is still too long, it is cut to 11 bytes followed by `~`.

Each entry's hash covers the full id and version, so shortening them never hides a
difference. Two ids that shorten to the same text are an error.

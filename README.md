# btm-mod

Mod tooling for Byte Tactics ([ozgb/btm](https://github.com/ozgb/btm)):

- the `btm_mod.json` v1 manifest;
- the resolver, which orders a mod set deterministically, fails fast on conflicts, and produces the resolved mod set and its hash (the hash the lobby, saves and replays compare);
- the `btm mod` CLI: `new`, `check`, `resolve` and `preview`.

The design follows sections 2.2 and 2.4 of the Byte Tactics modding plan. MIT.

## Status

Work in progress.

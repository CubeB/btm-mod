//! Byte Tactics mod tooling: the `btm_mod.json` v1 manifest, the resolver that turns a
//! set of mods into the resolved mod set and its hash, the conversion to the netplay
//! lobby's mod list. See sections 2.1 to 2.6 of the modding plan
//! and `docs/resolved-set.md`.
//!
//! ```
//! use std::collections::BTreeMap;
//! use btm_mod::{resolve, Environment, Manifest, ModPackage};
//!
//! let manifest = Manifest::parse(r#"{
//!     "manifest": 1, "id": "tiny-patch", "version": "1.0.0",
//!     "kind": ["ruleset"], "engine": ">=0.4 <0.5",
//!     "requires": { "economy": "^5" }
//! }"#).unwrap();
//! let env = Environment {
//!     engine: "0.4.2".parse().unwrap(),
//!     interfaces: BTreeMap::from([("economy".into(), "5.1.0".parse().unwrap())]),
//!     phases: vec!["wind".into(), "meteors".into()],
//! };
//! let pkg = ModPackage { manifest, files: BTreeMap::new() };
//! let set = resolve(&[pkg], &env, &BTreeMap::new()).unwrap();
//! assert_eq!(set.mods[0].id, "tiny-patch");
//! println!("{}", set.hash);
//! ```

mod canon;
pub mod digest;
mod json;
pub mod lobby;
pub mod manifest;
pub mod names;
pub mod range;
pub mod resolve;

pub use canon::{MOD_TAG, SET_TAG};
pub use digest::Digest;
pub use lobby::{LobbyError, LobbyMod, LobbyModSet};
pub use manifest::{Kind, Manifest, ManifestError, OptionSpec, Problem};
pub use range::Range;
pub use resolve::{
    check_files, resolve, Environment, ModPackage, OptionValue, OptionValues, ResolveError,
    ResolvedMod, ResolvedSet,
};

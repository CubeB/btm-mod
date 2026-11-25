//! Byte Tactics mod tooling: the `btm_mod.json` v1 manifest (modding plan, section 2.2).

pub mod digest;
mod json;
pub mod manifest;
pub mod names;
pub mod range;

pub use digest::Digest;
pub use manifest::{Kind, Manifest, ManifestError, OptionSpec, Problem};
pub use range::Range;

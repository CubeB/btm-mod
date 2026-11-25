//! Conversion to the netplay lobby's mod list (`btm_netplay::session::ModSet`).
//!
//! The lobby carries one hash over everything plus a bounded `(id, version, hash)` list,
//! used only to name which mods differ. Its caps are mirrored here, so this crate does not
//! depend on `btm-netplay`; build the lobby value with
//! `ModSet::new(set.hash, mods.map(|m| ModEntry { id: m.id, version: m.version, hash: m.hash }))`.
//!
//! Rules:
//! * Only synced mods are listed, in resolved order; presentation-only mods are optional
//!   per player and never compared.
//! * The hash is the resolved mod-set hash; each mod's hash is its digest, which covers
//!   the full id and version, so shortening below never hides a difference.
//! * An id longer than [`MAX_MOD_ID`] bytes is shortened to its first 15 bytes, `~`, and
//!   the first 8 hex digits of SHA-256 of the full id (24 bytes in all).
//! * A version longer than [`MAX_MOD_VERSION`] bytes first loses its `+build` metadata;
//!   if still too long it is cut to 11 bytes plus `~`.
//! * More than [`MAX_MODS`] synced mods, or two ids that shorten to the same text, is an
//!   error.

use crate::digest::Digest;
use crate::resolve::ResolvedSet;

/// `btm_netplay::session::MAX_MODS`.
pub const MAX_MODS: usize = 16;
/// `btm_netplay::session::MAX_MOD_ID`.
pub const MAX_MOD_ID: usize = 24;
/// `btm_netplay::session::MAX_MOD_VERSION`.
pub const MAX_MOD_VERSION: usize = 12;

/// One lobby entry; field for field `btm_netplay::session::ModEntry`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyMod {
    pub id: String,
    pub version: String,
    pub hash: [u8; 32],
}

/// The lobby's mod set; the arguments of `btm_netplay::session::ModSet::new`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyModSet {
    pub hash: [u8; 32],
    pub mods: Vec<LobbyMod>,
}

/// Why a resolved set does not fit the lobby.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LobbyError {
    #[error("{0} synced mods do not fit the lobby's limit of {MAX_MODS}")]
    TooMany(usize),
    #[error("mod ids {first} and {second} both shorten to {short} for the lobby")]
    IdCollision {
        first: String,
        second: String,
        short: String,
    },
}

/// An id that fits [`MAX_MOD_ID`].
pub fn lobby_id(id: &str) -> String {
    if id.len() <= MAX_MOD_ID {
        return id.to_owned();
    }
    // Ids are ASCII (see `names::is_mod_id`), but stay safe on any input.
    let head: String = id.chars().take_while(char::is_ascii).take(15).collect();
    let tag = Digest::of(id.as_bytes()).to_hex();
    format!("{head}~{}", tag.get(..8).unwrap_or_default())
}

/// A version that fits [`MAX_MOD_VERSION`].
pub fn lobby_version(version: &str) -> String {
    if version.len() <= MAX_MOD_VERSION {
        return version.to_owned();
    }
    let core = version.split('+').next().unwrap_or(version);
    if core.len() <= MAX_MOD_VERSION {
        return core.to_owned();
    }
    let head: String = core.chars().take_while(char::is_ascii).take(11).collect();
    format!("{head}~")
}

impl ResolvedSet {
    /// The lobby's view of this set. See the module docs for the rules.
    pub fn to_lobby(&self) -> Result<LobbyModSet, LobbyError> {
        let synced: Vec<_> = self.synced().collect();
        if synced.len() > MAX_MODS {
            return Err(LobbyError::TooMany(synced.len()));
        }
        let mut mods: Vec<LobbyMod> = Vec::new();
        let mut full: Vec<&str> = Vec::new();
        for m in synced {
            let id = lobby_id(&m.id);
            if let Some(i) = mods.iter().position(|e| e.id == id) {
                return Err(LobbyError::IdCollision {
                    first: full.get(i).copied().unwrap_or_default().to_owned(),
                    second: m.id.clone(),
                    short: id,
                });
            }
            full.push(&m.id);
            mods.push(LobbyMod {
                id,
                version: lobby_version(&m.version.to_string()),
                hash: m.digest.0,
            });
        }
        Ok(LobbyModSet {
            hash: self.hash.0,
            mods,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_fit() {
        assert_eq!(lobby_id("weather-storms"), "weather-storms");
        let exact = "a".repeat(24);
        assert_eq!(lobby_id(&exact), exact);
        let long = "io.github.someone.very-long-mod-name";
        let short = lobby_id(long);
        assert_eq!(short.len(), MAX_MOD_ID);
        assert!(short.starts_with("io.github.someo~"));
        assert_ne!(short, lobby_id("io.github.someone.very-long-mod-namf"));
    }

    #[test]
    fn versions_fit() {
        assert_eq!(lobby_version("1.2.0"), "1.2.0");
        assert_eq!(lobby_version("1.2.0+build.77"), "1.2.0");
        assert_eq!(lobby_version("1.2.0-beta.10"), "1.2.0-beta.~");
        assert_eq!(lobby_version("1.2.0-beta.10").len(), MAX_MOD_VERSION);
        assert_eq!(lobby_version("1.2.0-rc.1+build"), "1.2.0-rc.1");
    }
}

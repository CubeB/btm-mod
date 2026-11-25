//! The canonical encoding of a resolved mod set, specified in `docs/resolved-set.md`.
//! Keep the two in step: any change here is a new format version.

use crate::digest::Digest;
use crate::resolve::{FileRole, OptionValue, ResolvedMod, ResolvedSet};

/// Domain tag that starts the set encoding.
pub const SET_TAG: &str = "btm-mod/resolved-set/v1";
/// Domain tag that starts a per-mod digest's input.
pub const MOD_TAG: &str = "btm-mod/mod/v1";

struct Enc(Vec<u8>);

impl Enc {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn len(&mut self, n: usize) {
        self.u32(u32::try_from(n).unwrap_or(u32::MAX));
    }

    fn str(&mut self, s: &str) {
        self.len(s.len());
        self.0.extend_from_slice(s.as_bytes());
    }

    fn opt_str(&mut self, s: Option<&str>) {
        match s {
            None => self.u8(0),
            Some(s) => {
                self.u8(1);
                self.str(s);
            }
        }
    }

    fn digest(&mut self, d: &Digest) {
        self.0.extend_from_slice(&d.0);
    }
}

/// `mod_record` of the spec: the synced part of one mod (presentation files excluded).
fn mod_record(e: &mut Enc, m: &ResolvedMod) {
    e.str(&m.id);
    e.str(&m.version.to_string());
    let files: Vec<_> = m
        .files
        .iter()
        .filter(|f| f.role != FileRole::Presentation)
        .collect();
    e.len(files.len());
    for f in files {
        e.u8(f.role as u8);
        e.str(&f.path);
        e.digest(&f.sha256);
    }
    e.len(m.modules.len());
    for md in &m.modules {
        e.str(&md.file);
        let imp = md.implements.as_ref().map(|i| {
            let v = &i.version;
            format!("{}@{v}", i.name)
        });
        e.opt_str(imp.as_deref());
        e.len(md.wraps.len());
        for w in &md.wraps {
            e.str(&w.to_string());
        }
        e.opt_str(md.phase.as_deref());
        e.u64(md.state_bytes);
        e.len(md.publish.len());
        for p in &md.publish {
            e.str(p);
        }
        e.len(md.subscribe.len());
        for s in &md.subscribe {
            e.str(&s.name);
        }
    }
    e.len(m.options.len());
    for (name, v) in &m.options {
        e.str(name);
        match v {
            OptionValue::Int(i) => {
                e.u8(0);
                e.i64(*i);
            }
            OptionValue::Bool(b) => {
                e.u8(1);
                e.u8(u8::from(*b));
            }
            OptionValue::Enum(s) => {
                e.u8(2);
                e.str(s);
            }
        }
    }
}

/// The per-mod digest: SHA-256 of `str(MOD_TAG) || mod_record`.
pub(crate) fn mod_digest(m: &ResolvedMod) -> Digest {
    let mut e = Enc(Vec::new());
    e.str(MOD_TAG);
    mod_record(&mut e, m);
    Digest::of(&e.0)
}

/// The full set encoding; the set hash is its SHA-256.
pub(crate) fn set_bytes(s: &ResolvedSet) -> Vec<u8> {
    let mut e = Enc(Vec::new());
    e.str(SET_TAG);
    let synced: Vec<&ResolvedMod> = s.synced().collect();
    e.len(synced.len());
    for m in synced {
        mod_record(&mut e, m);
    }
    e.len(s.phases.len());
    for p in &s.phases {
        e.str(p);
    }
    e.len(s.events.len());
    for ev in &s.events {
        e.u32(ev.number);
        e.str(&ev.name);
        e.str(&ev.publisher);
    }
    e.len(s.state.len());
    for r in &s.state {
        e.str(&r.mod_id);
        e.str(&r.file);
        e.u64(r.offset);
        e.u64(r.len);
    }
    e.0
}

//! System-font fallback — egui-free.
//!
//! The bundled Noto Sans / Serif / Mono + Noto Sans Arabic cover Latin / Greek /
//! Cyrillic + Arabic. For any other script (Indic, CJK, Thai, Hebrew, …) — e.g.
//! **non-Latin text the user types or pastes** into a Text object — we fall back to
//! a font already installed on the user's machine that covers the glyph. Fonts are
//! only **read at runtime** and rasterized into the saved image; none are bundled or
//! redistributed, so offering every system font stays commercial-safe.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use ab_glyph::{Font, FontRef};
use fontdb::Database;

/// The system font database, loaded once on first use.
fn db() -> &'static Database {
    static DB: OnceLock<Database> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = Database::new();
        db.load_system_fonts();
        db
    })
}

/// Per-Unicode-block cache so each script is resolved at most once. The bytes are
/// leaked to `'static` (one font per script, alive for the app's lifetime) so they
/// slot into the renderer's existing `&'static` font path.
type Cache = Mutex<HashMap<u32, Option<(&'static [u8], u32)>>>;
fn cache() -> &'static Cache {
    static C: OnceLock<Cache> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A system font (bytes + face index) that can render `c`, or `None` if none is
/// installed. Cached per 256-codepoint block (≈ per script).
pub fn fallback_for(c: char) -> Option<(&'static [u8], u32)> {
    let key = (c as u32) & !0xFF; // 256-codepoint bucket
    if let Some(hit) = cache().lock().ok().and_then(|m| m.get(&key).copied()) {
        return hit;
    }
    let found = find_cover(c);
    if let Ok(mut m) = cache().lock() {
        m.insert(key, found);
    }
    found
}

/// Scan installed fonts for one with a glyph for `c`; leak its bytes to `'static`.
/// Blocking (reads font files) but runs at most once per script, then it's cached.
fn find_cover(c: char) -> Option<(&'static [u8], u32)> {
    for face in db().faces() {
        let covers = db()
            .with_face_data(face.id, |data, index| {
                FontRef::try_from_slice_and_index(data, index)
                    .map(|f| f.glyph_id(c).0 != 0)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if !covers {
            continue;
        }
        let Some(bytes) = db().with_face_data(face.id, |data, _| data.to_vec()) else {
            continue;
        };
        let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        return Some((leaked, face.index));
    }
    None
}

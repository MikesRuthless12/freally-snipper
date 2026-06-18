//! freally-font — data-driven glyph store for the parallel design swarm.
//!
//! Each glyph is **one `.glyph` data file per codepoint** in `glyphs/` (named by the
//! 4-hex Unicode codepoint, e.g. `0048.glyph` = 'H', `004F.glyph` = 'O'), so many
//! agents can author glyphs **in parallel with zero file conflicts**. Everything is
//! authored from scratch — no third-party glyph data.
//!
//! Units: **1000 UPEM, y-up, baseline = 0.** File format (one command per line,
//! `#` starts a comment):
//!   advance N                  — advance width
//!   rect x0 y0 x1 y1           — filled rectangle (a contour)
//!   ellipse cx cy rx ry        — filled ellipse (a contour; nest two → counter)
//!   M x y / L x y              — move / line
//!   C x1 y1 x2 y2 x y          — cubic bézier
//!   Z (or close)               — close the current contour
//! Contours fill with the **even-odd** rule, so a nested contour cuts a hole.

use std::collections::HashMap;
use std::path::{Path as FsPath, PathBuf};

use tiny_skia::{Path, PathBuilder};

// ---- Shared design spec: the consistency anchor every glyph (and agent) obeys ----
/// Units per em — the design grid.
pub const UPEM: f32 = 1000.0;
/// Capital height.
pub const CAP: f32 = 700.0;
/// Lowercase x-height (measured from Noto on our grid).
pub const XHEIGHT: f32 = 525.0;
/// Ascender height (b d f h k l) — measured from Noto 'l'.
pub const ASCENDER: f32 = 745.0;
/// Descender depth (g j p q y) — negative; measured from Noto 'p'.
pub const DESCENDER: f32 = -235.0;
/// Thick (vertical) stroke weight.
pub const STEM: f32 = 90.0;
/// Thin (horizontal / curve top-bottom) stroke — subtle humanist contrast.
pub const THIN: f32 = 68.0;
/// Round letters overshoot the cap/baseline slightly (optical correction).
pub const OVERSHOOT: f32 = 10.0;
/// Fallback advance for a glyph file that omits `advance`.
pub const DEFAULT_ADVANCE: f32 = 600.0;

/// Circle→bézier constant.
const K: f32 = 0.552_284_8;

/// A drawing command in glyph units (y-up).
#[derive(Clone, Copy)]
pub enum Cmd {
    Move(f32, f32),
    Line(f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
    Close,
}

/// One glyph: advance + path commands. If `stroke` is set, the commands are a
/// CENTERLINE stroked at that width (uniform thickness); else they're a fill outline.
pub struct Glyph {
    pub advance: f32,
    pub cmds: Vec<Cmd>,
    pub stroke: Option<f32>,
    /// Filled discs (cx, cy, r) — always filled solid regardless of `stroke` (for dots).
    pub discs: Vec<(f32, f32, f32)>,
}

fn push_ellipse(cmds: &mut Vec<Cmd>, cx: f32, cy: f32, rx: f32, ry: f32) {
    let (kx, ky) = (rx * K, ry * K);
    cmds.push(Cmd::Move(cx + rx, cy));
    cmds.push(Cmd::Cubic(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry));
    cmds.push(Cmd::Cubic(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy));
    cmds.push(Cmd::Cubic(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry));
    cmds.push(Cmd::Cubic(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy));
    cmds.push(Cmd::Close);
}

fn push_rect(cmds: &mut Vec<Cmd>, x0: f32, y0: f32, x1: f32, y1: f32) {
    cmds.push(Cmd::Move(x0, y0));
    cmds.push(Cmd::Line(x1, y0));
    cmds.push(Cmd::Line(x1, y1));
    cmds.push(Cmd::Line(x0, y1));
    cmds.push(Cmd::Close);
}

/// Parse one `.glyph` source into a [`Glyph`]. Unknown/garbled lines are ignored.
pub fn parse(src: &str) -> Glyph {
    let mut advance = DEFAULT_ADVANCE;
    let mut stroke = None;
    let mut discs = Vec::new();
    let mut cmds = Vec::new();
    for raw in src.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut tok = line.split_whitespace();
        let head = tok.next().unwrap_or("");
        let nums: Vec<f32> = tok.filter_map(|s| s.parse().ok()).collect();
        match (head, nums.as_slice()) {
            ("advance", [a]) => advance = *a,
            ("stroke", [wv]) => stroke = Some(*wv),
            ("disc", [cx, cy, r]) => discs.push((*cx, *cy, *r)),
            ("rect", [x0, y0, x1, y1]) => push_rect(&mut cmds, *x0, *y0, *x1, *y1),
            ("ellipse", [cx, cy, rx, ry]) => push_ellipse(&mut cmds, *cx, *cy, *rx, *ry),
            ("M", [x, y]) => cmds.push(Cmd::Move(*x, *y)),
            ("L", [x, y]) => cmds.push(Cmd::Line(*x, *y)),
            ("C", [a, b, c, d, e, f]) => cmds.push(Cmd::Cubic(*a, *b, *c, *d, *e, *f)),
            ("Z" | "close", _) => cmds.push(Cmd::Close),
            _ => {}
        }
    }
    Glyph {
        advance,
        cmds,
        stroke,
        discs,
    }
}

/// The glyphs directory (this crate's `glyphs/`, baked at compile time).
pub fn glyphs_dir() -> PathBuf {
    FsPath::new(env!("CARGO_MANIFEST_DIR")).join("glyphs")
}

/// The `glyphs/XXXX.glyph` path for codepoint `c` (hex-named so 'H'/'h' don't clash
/// on case-insensitive filesystems).
pub fn glyph_file(c: char) -> PathBuf {
    glyphs_dir().join(format!("{:04X}.glyph", c as u32))
}

/// Load every authored glyph into a char→[`Glyph`] map.
pub fn load_glyphs() -> HashMap<char, Glyph> {
    let mut map = HashMap::new();
    let Ok(dir) = std::fs::read_dir(glyphs_dir()) else {
        return map;
    };
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("glyph") {
            continue;
        }
        let cp = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| u32::from_str_radix(s, 16).ok());
        let Some(ch) = cp.and_then(char::from_u32) else {
            continue;
        };
        if let Ok(src) = std::fs::read_to_string(&path) {
            map.insert(ch, parse(&src));
        }
    }
    map
}

/// Build a tiny-skia path (glyph units, y-up) from a glyph's commands.
pub fn glyph_path(g: &Glyph) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for c in &g.cmds {
        match *c {
            Cmd::Move(x, y) => pb.move_to(x, y),
            Cmd::Line(x, y) => pb.line_to(x, y),
            Cmd::Cubic(a, b, c, d, e, f) => pb.cubic_to(a, b, c, d, e, f),
            Cmd::Close => pb.close(),
        }
    }
    pb.finish()
}

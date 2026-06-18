//! freally-font specimen / calibration harness — with a precise unit GRID, a Noto-vs-
//! ours OVERLAY, and printed METRICS, so glyphs are matched to the reference by the
//! numbers (width, height, advance) and by direct superposition, not just by eye.
//!
//!   cargo run -p freally-font -- 8        # one glyph
//!   freally-font.exe 89                   # several
//! Output `specimen-<text>.png`: three rows on a 100-unit grid —
//!   row 1 = ours (black), row 2 = Noto (black), row 3 = OVERLAY (ours BLUE over Noto RED).
//! Plus per-glyph metrics to stdout. Our glyphs stay 100% original; the reference is a
//! measuring stick only.

use std::collections::{HashMap, HashSet};

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use freally_font::{
    glyph_path, load_glyphs, Cmd, Glyph, ASCENDER, CAP, DEFAULT_ADVANCE, DESCENDER, XHEIGHT,
};
use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PremultipliedColorU8, Stroke,
    Transform,
};

const REFERENCE: &[u8] = include_bytes!("../../editor/fonts/NotoSans-Regular.ttf");

/// Reference cap-height as a fraction of its em (measured from 'H') for exact matching.
fn ref_cap_ratio() -> f32 {
    let Ok(font) = FontRef::try_from_slice(REFERENCE) else {
        return 0.7;
    };
    let probe = 1000.0_f32;
    let g = font
        .glyph_id('H')
        .with_scale_and_position(PxScale::from(probe), point(0.0, 0.0));
    match font.outline_glyph(g) {
        Some(o) => {
            let b = o.px_bounds();
            ((b.max.y - b.min.y).abs() / probe).max(0.1)
        }
        None => 0.7,
    }
}

#[inline]
fn blend(buf: &mut [PremultipliedColorU8], idx: usize, r: f32, g: f32, b: f32, a: f32) {
    let p = buf[idx];
    let nr = (p.red() as f32 * (1.0 - a) + r * a)
        .round()
        .clamp(0.0, 255.0) as u8;
    let ng = (p.green() as f32 * (1.0 - a) + g * a)
        .round()
        .clamp(0.0, 255.0) as u8;
    let nb = (p.blue() as f32 * (1.0 - a) + b * a)
        .round()
        .clamp(0.0, 255.0) as u8;
    buf[idx] = PremultipliedColorU8::from_rgba(nr, ng, nb, 255).unwrap();
}

/// Fine unit grid: lines every 25 units (4x denser), with bold majors every 100,
/// boldest at baseline (0) + cap (700), and the x-height tinted blue. Lets every edge
/// land on a precise line.
fn draw_grid(pixmap: &mut Pixmap, x0: f32, baseline_y: f32, scale: f32, total_units: f32) {
    let (w, h) = (pixmap.width() as i32, pixmap.height() as i32);
    let top = (baseline_y - 770.0 * scale).max(0.0) as i32;
    let bot = ((baseline_y + 250.0 * scale) as i32).min(h);
    let x1 = (x0.round() as i32).max(0);
    let x2 = ((x0 + total_units * scale).round() as i32).min(w);
    let buf = pixmap.pixels_mut();

    // verticals every 25u; bold at multiples of 100
    let mut u = 0.0;
    while u <= total_units + 1.0 {
        let x = (x0 + u * scale).round() as i32;
        if x >= 0 && x < w {
            let (r, g, b, a) = if u % 100.0 < 0.5 {
                (150.0, 150.0, 162.0, 0.55)
            } else {
                (210.0, 210.0, 218.0, 0.30)
            };
            for y in top..bot {
                blend(buf, (y * w + x) as usize, r, g, b, a);
            }
        }
        u += 25.0;
    }

    // horizontals every 25u; boldest at baseline/cap, major every 100
    let mut yu = -250.0;
    while yu <= 770.0 {
        let y = (baseline_y - yu * scale).round() as i32;
        if y >= 0 && y < h {
            let (r, g, b, a) = if yu.abs() < 0.5 || (yu - CAP).abs() < 0.5 {
                (110.0, 110.0, 120.0, 0.8)
            } else if yu % 100.0 < 0.5 {
                (150.0, 150.0, 162.0, 0.55)
            } else {
                (210.0, 210.0, 218.0, 0.30)
            };
            for x in x1..x2 {
                blend(buf, (y * w + x) as usize, r, g, b, a);
            }
        }
        yu += 25.0;
    }

    // x-height (off the 25-grid) in blue
    let y = (baseline_y - XHEIGHT * scale).round() as i32;
    if y >= 0 && y < h {
        for x in x1..x2 {
            blend(buf, (y * w + x) as usize, 150.0, 180.0, 230.0, 0.7);
        }
    }
}

/// Fill our glyphs (given colour + alpha) from the loaded data.
fn render_mine(
    pixmap: &mut Pixmap,
    glyphs: &HashMap<char, Glyph>,
    text: &str,
    scale: f32,
    x0: f32,
    baseline_y: f32,
    color: Color,
) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    let mut pen = 0.0;
    for ch in text.chars() {
        match glyphs.get(&ch) {
            Some(g) => {
                let t = Transform::from_row(scale, 0.0, 0.0, -scale, x0 + pen * scale, baseline_y);
                if let Some(path) = glyph_path(g) {
                    match g.stroke {
                        Some(wd) => {
                            let st = Stroke {
                                width: wd,
                                line_cap: LineCap::Butt,
                                line_join: LineJoin::Round,
                                ..Default::default()
                            };
                            pixmap.stroke_path(&path, &paint, &st, t, None);
                        }
                        None => pixmap.fill_path(&path, &paint, FillRule::EvenOdd, t, None),
                    }
                }
                for &(cx, cy, r) in &g.discs {
                    let mut pb = PathBuilder::new();
                    pb.push_circle(cx, cy, r);
                    if let Some(circle) = pb.finish() {
                        pixmap.fill_path(&circle, &paint, FillRule::Winding, t, None);
                    }
                }
                pen += g.advance;
            }
            None => pen += DEFAULT_ADVANCE,
        }
    }
}

/// Draw Noto coverage at `px` in the given colour, stepping origins by OUR advance so
/// each Noto glyph sits at the same origin as ours (for true overlay).
#[allow(clippy::too_many_arguments)] // a specimen render helper; bundling args into a struct adds noise
fn render_ref(
    pixmap: &mut Pixmap,
    glyphs: &HashMap<char, Glyph>,
    text: &str,
    px: f32,
    x0: f32,
    baseline_y: f32,
    rgb: (f32, f32, f32),
    amax: f32,
) {
    let Ok(font) = FontRef::try_from_slice(REFERENCE) else {
        return;
    };
    let pxs = PxScale::from(px);
    let (w, h) = (pixmap.width() as i32, pixmap.height() as i32);
    let mut pen = 0.0_f32; // font units
    for ch in text.chars() {
        let gid = font.glyph_id(ch);
        let origin_x = x0 + pen * (px / 1000.0);
        let g = gid.with_scale_and_position(pxs, point(origin_x, baseline_y));
        if let Some(o) = font.outline_glyph(g) {
            let b = o.px_bounds();
            let buf = pixmap.pixels_mut();
            o.draw(|dx, dy, cov| {
                let (x, y) = (b.min.x as i32 + dx as i32, b.min.y as i32 + dy as i32);
                if x >= 0 && y >= 0 && x < w && y < h {
                    blend(buf, (y * w + x) as usize, rgb.0, rgb.1, rgb.2, cov * amax);
                }
            });
        }
        pen += glyphs
            .get(&ch)
            .map(|g| g.advance)
            .unwrap_or(DEFAULT_ADVANCE);
    }
}

/// Our glyph's bbox in font units (on-curve + control points — exact for our ellipses).
fn ours_bbox(g: &Glyph) -> Option<(f32, f32, f32, f32)> {
    let (mut nx, mut ny, mut xx, mut xy) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    let mut any = false;
    let mut pts: Vec<(f32, f32)> = Vec::new();
    for c in &g.cmds {
        match *c {
            Cmd::Move(x, y) | Cmd::Line(x, y) => pts.push((x, y)),
            Cmd::Cubic(a, b, c, d, e, f) => {
                pts.push((a, b));
                pts.push((c, d));
                pts.push((e, f));
            }
            Cmd::Close => {}
        }
    }
    for (x, y) in pts {
        nx = nx.min(x);
        ny = ny.min(y);
        xx = xx.max(x);
        xy = xy.max(y);
        any = true;
    }
    if any {
        let pad = g.stroke.map(|w| w / 2.0).unwrap_or(0.0);
        Some((nx - pad, ny - pad, xx + pad, xy + pad))
    } else {
        None
    }
}

/// Noto glyph (width, height, advance) in font units.
fn noto_metrics(font: &FontRef, ch: char, px: f32) -> Option<(f32, f32, f32)> {
    let scale_units = px / 1000.0;
    let pxs = PxScale::from(px);
    let scaled = font.as_scaled(pxs);
    let gid = font.glyph_id(ch);
    let adv = scaled.h_advance(gid) / scale_units;
    let o = font.outline_glyph(gid.with_scale_and_position(pxs, point(0.0, 0.0)))?;
    let b = o.px_bounds();
    Some((
        (b.max.x - b.min.x) / scale_units,
        (b.max.y - b.min.y) / scale_units,
        adv,
    ))
}

/// All control/anchor points of an outline curve (font units).
fn curve_pts(c: &ab_glyph::OutlineCurve) -> Vec<(f32, f32)> {
    use ab_glyph::OutlineCurve::*;
    match c {
        Line(a, b) => vec![(a.x, a.y), (b.x, b.y)],
        Quad(a, q, b) => vec![(a.x, a.y), (q.x, q.y), (b.x, b.y)],
        Cubic(a, c1, c2, b) => vec![(a.x, a.y), (c1.x, c1.y), (c2.x, c2.y), (b.x, b.y)],
    }
}

/// Reverse-engineer the reference glyph's outline INTO OUR grid units (cap-normalized to
/// 700, y-up), so we can place our skeleton on the same gridpoints. Used for calibration
/// only — we redraw our own outline, we do not ship the reference's curve data. Scale is
/// self-calibrated so the reference 'H' (a cap) is exactly CAP tall.
fn dump_ref_outline(ch: char) {
    let Ok(font) = FontRef::try_from_slice(REFERENCE) else {
        return;
    };
    let h_ext = font
        .outline(font.glyph_id('H'))
        .map(|o| {
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            for c in &o.curves {
                for p in curve_pts(c) {
                    lo = lo.min(p.1);
                    hi = hi.max(p.1);
                }
            }
            (hi - lo).max(1.0)
        })
        .unwrap_or(1000.0);
    let g = CAP / h_ext; // reference cap-height -> our CAP (700)
    let gid = font.glyph_id(ch);
    match font.outline(gid) {
        Some(o) => {
            println!("--- reference '{ch}' outline in OUR grid units (cap=700, y-up) ---");
            for c in &o.curves {
                match c {
                    ab_glyph::OutlineCurve::Line(a, b) => {
                        println!("L  {:>4.0},{:>4.0} -> {:>4.0},{:>4.0}", a.x * g, a.y * g, b.x * g, b.y * g)
                    }
                    ab_glyph::OutlineCurve::Quad(a, q, b) => println!(
                        "Q  {:>4.0},{:>4.0}  ctrl {:>4.0},{:>4.0}  -> {:>4.0},{:>4.0}",
                        a.x * g, a.y * g, q.x * g, q.y * g, b.x * g, b.y * g
                    ),
                    ab_glyph::OutlineCurve::Cubic(a, c1, c2, b) => println!(
                        "C  {:>4.0},{:>4.0}  c1 {:>4.0},{:>4.0}  c2 {:>4.0},{:>4.0}  -> {:>4.0},{:>4.0}",
                        a.x * g, a.y * g, c1.x * g, c1.y * g, c2.x * g, c2.y * g, b.x * g, b.y * g
                    ),
                }
            }
        }
        None => println!("(no outline for '{ch}')"),
    }
}

fn sanitize(text: &str) -> String {
    let mut s = String::new();
    for c in text.chars() {
        if c.is_ascii_uppercase() {
            // mark caps so they don't collide with lowercase on case-insensitive filesystems
            s.push('U');
            s.push(c);
        } else if c.is_ascii_lowercase() || c.is_ascii_digit() {
            s.push(c);
        } else {
            s.push_str(&format!("u{:04X}", c as u32));
        }
    }
    if s.is_empty() {
        s.push_str("set");
    }
    s
}

fn main() {
    let glyphs = load_glyphs();
    let text = std::env::args().nth(1).unwrap_or_else(|| {
        let mut cs: Vec<char> = glyphs.keys().copied().collect();
        cs.sort_unstable();
        cs.into_iter().collect()
    });
    if text.is_empty() {
        println!("no glyphs authored yet — glyphs/ is empty");
        return;
    }

    let scale = std::env::args()
        .nth(2)
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|v| *v > 0.0)
        .unwrap_or(0.5);
    let margin = 70.0_f32;
    let cap_px = CAP * scale;
    let ref_px = cap_px / ref_cap_ratio();

    let advance_of = |c: char| glyphs.get(&c).map(|g| g.advance).unwrap_or(DEFAULT_ADVANCE);
    let total_units: f32 = text.chars().map(advance_of).sum();
    let w = (total_units * scale + 2.0 * margin).ceil().max(1.0) as u32;
    let asc_px = ASCENDER * scale;
    let desc_px = -DESCENDER * scale;
    let row_h = asc_px + desc_px + 64.0;
    let base1 = margin + asc_px;
    let base2 = base1 + row_h;
    let base3 = base2 + row_h;
    let h = (base3 + desc_px + margin).ceil() as u32;
    let _ = cap_px;

    let mut pixmap = Pixmap::new(w, h).expect("pixmap");
    pixmap.fill(Color::WHITE);

    draw_grid(&mut pixmap, margin, base1, scale, total_units);
    draw_grid(&mut pixmap, margin, base2, scale, total_units);
    draw_grid(&mut pixmap, margin, base3, scale, total_units);

    // row 1: ours (black). row 2: Noto (black). row 3: overlay — Noto red, then ours blue.
    render_mine(
        &mut pixmap,
        &glyphs,
        &text,
        scale,
        margin,
        base1,
        Color::BLACK,
    );
    render_ref(
        &mut pixmap,
        &glyphs,
        &text,
        ref_px,
        margin,
        base2,
        (0.0, 0.0, 0.0),
        1.0,
    );
    render_ref(
        &mut pixmap,
        &glyphs,
        &text,
        ref_px,
        margin,
        base3,
        (220.0, 40.0, 40.0),
        0.6,
    );
    render_mine(
        &mut pixmap,
        &glyphs,
        &text,
        scale,
        margin,
        base3,
        Color::from_rgba8(30, 70, 230, 130),
    );

    let fname = format!("specimen-{}.png", sanitize(&text));
    pixmap.save_png(&fname).expect("save png");

    let font = FontRef::try_from_slice(REFERENCE).ok();
    println!(
        "wrote {fname} ({w}x{h}) — row1 ours / row2 Noto / row3 OVERLAY (ours BLUE over Noto RED), 100-unit grid"
    );
    let cap_scale = CAP / (ref_cap_ratio() * 1000.0); // Noto per-mille -> our cap-matched units
    println!(
        "metrics — both normalized to OUR cap-height ({:.0}); match OURS W×H and W/H to NOTO:",
        CAP
    );
    let mut seen = HashSet::new();
    for ch in text.chars() {
        if !seen.insert(ch) {
            continue;
        }
        let os = match glyphs.get(&ch).and_then(ours_bbox) {
            Some((nx, ny, xx, xy)) => format!(
                "W×H={:.0}×{:.0} W/H={:.2} adv={:.0} x[{:.0}..{:.0}] y[{:.0}..{:.0}]",
                xx - nx,
                xy - ny,
                (xx - nx) / (xy - ny).max(1.0),
                advance_of(ch),
                nx,
                xx,
                ny,
                xy
            ),
            None => "(none)".into(),
        };
        let ns = match font.as_ref().and_then(|f| noto_metrics(f, ch, ref_px)) {
            Some((wd, ht, adv)) => {
                let (wd, ht, adv) = (wd * cap_scale, ht * cap_scale, adv * cap_scale);
                format!(
                    "W×H={:.0}×{:.0} W/H={:.2} adv={:.0}",
                    wd,
                    ht,
                    wd / ht.max(1.0),
                    adv
                )
            }
            None => "(none)".into(),
        };
        println!("  '{ch}'  OURS {os}   |   NOTO {ns}");
    }

    if text.chars().count() == 1 {
        if let Some(c) = text.chars().next() {
            dump_ref_outline(c);
        }
    }
}

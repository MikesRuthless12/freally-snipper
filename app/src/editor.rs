//! The post-capture editor hand-off (Phase 3, P3.2).
//!
//! When **Markup** is armed on the capture action bar (Toolbar 1) — or the
//! persisted "show capture editor" setting is on — a committed snip is handed to
//! this surface instead of being saved straight away. It opens **centered below
//! the selection**, shows the captured image, and offers **Save / Discard**.
//!
//! This is the *shell* of the WYSIWYG image editor (Toolbar 2): the markup tools
//! — pen, brush, highlighter, shapes, text, filters, transforms, OCR, … — arrive
//! in **Phase 4** and fill this surface in (and the editor then moves into the
//! `freally-editor` crate). It is hosted inside the full-desktop overlay window so
//! the panel can be anchored to the on-screen selection.

use eframe::egui::{self, Align2, Color32, Rect as ERect};
use freally_capture::image::RgbaImage;
use freally_capture::Rect as VRect;

/// Largest preview the editor card shows, in points (the capture is scaled to fit
/// within this, never upscaled).
const PREVIEW_MAX: egui::Vec2 = egui::vec2(440.0, 320.0);

/// What the editor surface wants the app to do this frame.
pub enum EditorOutcome {
    /// Keep the editor open.
    Active,
    /// Save the capture (folder + clipboard), then return home.
    Save,
    /// Throw the capture away and return home.
    Discard,
}

/// A live post-capture editor session (the Phase 3 hand-off shell).
pub struct EditorSession {
    /// The committed capture (handed to delivery on Save).
    image: RgbaImage,
    /// GPU texture for the live preview.
    texture: egui::TextureHandle,
    /// Selection bounds in virtual pixels, to anchor the card below it; `None`
    /// (e.g. a full-screen grab) centers the card on screen.
    region: Option<VRect>,
    /// Top-left of the host desktop in virtual pixels, to map `region` to screen.
    origin: (i32, i32),
}

impl EditorSession {
    /// Upload the capture and open the editor anchored below `region`.
    pub fn new(
        ctx: &egui::Context,
        image: RgbaImage,
        region: Option<VRect>,
        origin: (i32, i32),
    ) -> Self {
        let size = [image.width() as usize, image.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
        let texture = ctx.load_texture("freally_editor_capture", color, Default::default());
        Self {
            image,
            texture,
            region,
            origin,
        }
    }

    /// Consume the session and return the captured image (on Save).
    pub fn into_image(self) -> RgbaImage {
        self.image
    }

    /// The selection's on-screen rectangle (points), for anchoring the card.
    fn anchor(&self, ctx: &egui::Context) -> Option<ERect> {
        let r = self.region?;
        let ppp = ctx.pixels_per_point().max(0.1);
        let to_pt = |vx: i32, vy: i32| {
            egui::pos2(
                (vx - self.origin.0) as f32 / ppp,
                (vy - self.origin.1) as f32 / ppp,
            )
        };
        Some(ERect::from_min_max(
            to_pt(r.x, r.y),
            to_pt(r.right(), r.bottom()),
        ))
    }

    /// Draw the editor and process input for one frame.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> EditorOutcome {
        let ctx = ui.ctx().clone();
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            return EditorOutcome::Discard;
        }

        // Dim the rest of the desktop so the focus is on the captured region.
        let screen = ui.max_rect();
        ui.painter()
            .rect_filled(screen, 0.0, Color32::from_black_alpha(160));

        // Preview size: fit the capture within PREVIEW_MAX (never upscale).
        let fit = fit_within(
            [self.image.width() as f32, self.image.height() as f32],
            PREVIEW_MAX,
        );
        let pivot = self.card_pivot(&ctx, screen, fit);

        let mut outcome = EditorOutcome::Active;
        egui::Area::new(egui::Id::new("freally_editor_card"))
            .order(egui::Order::Foreground)
            .pivot(Align2::CENTER_TOP)
            .fixed_pos(pivot)
            .show(&ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(fit.x.max(260.0));
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "Edit capture · {} × {}",
                                self.image.width(),
                                self.image.height()
                            ))
                            .strong(),
                        );
                        ui.add_space(6.0);
                        let sized = egui::load::SizedTexture::new(self.texture.id(), fit);
                        ui.add(egui::Image::from_texture(sized));
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui
                                .button("Save")
                                .on_hover_text("Save to your folder and copy to the clipboard")
                                .clicked()
                            {
                                outcome = EditorOutcome::Save;
                            }
                            if ui
                                .button("Discard")
                                .on_hover_text("Throw this capture away (Esc)")
                                .clicked()
                            {
                                outcome = EditorOutcome::Discard;
                            }
                        });
                        ui.add_space(4.0);
                        ui.small("Markup tools (Toolbar 2) arrive in Phase 4.");
                    });
                });
            });
        outcome
    }

    /// Pivot point (center-top of the card) just below the selection, clamped so
    /// the card stays fully on-screen.
    fn card_pivot(&self, ctx: &egui::Context, screen: ERect, fit: egui::Vec2) -> egui::Pos2 {
        // Estimate the card footprint (preview + title + buttons + frame padding).
        let card_w = fit.x.max(260.0) + 24.0;
        let card_h = fit.y + 112.0;
        let (cx, top) = match self.anchor(ctx) {
            Some(a) => (a.center().x, a.max.y + 12.0),
            None => (screen.center().x, screen.center().y - card_h * 0.5),
        };
        // Clamp horizontally (card centered) and vertically (card top) on-screen.
        let half = card_w * 0.5 + 4.0;
        let (lo_x, hi_x) = (screen.min.x + half, screen.max.x - half);
        let x = if lo_x <= hi_x {
            cx.clamp(lo_x, hi_x)
        } else {
            screen.center().x
        };
        let max_top = (screen.max.y - card_h - 4.0).max(screen.min.y + 4.0);
        let y = top.clamp(screen.min.y + 4.0, max_top);
        egui::pos2(x, y)
    }
}

/// Scale `size` to fit within `max`, never upscaling.
fn fit_within(size: [f32; 2], max: egui::Vec2) -> egui::Vec2 {
    let [w, h] = size;
    if w <= 0.0 || h <= 0.0 {
        return max;
    }
    let scale = (max.x / w).min(max.y / h).min(1.0);
    egui::vec2(w * scale, h * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_within_never_upscales_and_preserves_aspect() {
        // Smaller than the box: unchanged.
        assert_eq!(
            fit_within([100.0, 50.0], egui::vec2(440.0, 320.0)),
            egui::vec2(100.0, 50.0)
        );
        // Wider than the box: clamped to width, aspect preserved.
        let fit = fit_within([880.0, 320.0], egui::vec2(440.0, 320.0));
        assert_eq!(fit, egui::vec2(440.0, 160.0));
        // Degenerate size falls back to the max box.
        assert_eq!(
            fit_within([0.0, 0.0], egui::vec2(440.0, 320.0)),
            egui::vec2(440.0, 320.0)
        );
    }
}

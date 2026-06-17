//! `freally-editor` — the WYSIWYG image editor (Toolbar 2) for Freally Snipper.
//!
//! Phase 4 lives here. The guiding rule is *"Save writes exactly what you see."*
//!
//! - **P4.1** — the surface: the capture on a **zoom/pan canvas**, the **Toolbar 2**
//!   strip, and **Undo / Redo / Save / Copy / Discard**.
//! - **P4.2** — the **raster tools**: Pen, Brush, Highlighter (free + text-aware),
//!   and a two-mode Eraser (erase-to-white / restore-original). Each has its own
//!   adjustable **size**. Strokes preview live and bake into the raster on release;
//!   every bake is a single undo step.
//! - **P4.3** — **movable overlay objects** (shapes: rectangle / oval / line /
//!   arrow): select, drag to move, drag a handle to resize, Delete to remove;
//!   drawn over the raster and flattened only on Save. The same object model
//!   carries Text / Watermark (P4.4), Emoji (P4.7) and Image (P4.8).
//!
//! Still to come on the same [`EditorSession`]: text / watermark objects (P4.4),
//! emoji (P4.7) and image (P4.8); live filters (P4.5); transforms + eyedropper +
//! OCR (P4.6); translate-as-you-type text (P4.9). Until each lands, its toolbar
//! button is present but disabled and labelled with the prompt that enables it —
//! the capture bar's convention for not-yet-built features.
//!
//! The editor is drawn into the app's single OS window (morphed to a decorated
//! editor window), matching the one-window model the capture overlay already uses.

mod filters;
mod objects;
mod raster;
mod text;
mod transforms;

use std::collections::HashMap;

use egui::{Color32, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use freally_capture::image::RgbaImage;

use objects::{Kind, Object, ShapeKind, TextData};
use raster::Paint;
use text::FontFamily;

/// Crate identifier, surfaced in version banners and logs.
pub const CRATE_NAME: &str = "freally-editor";

/// Zoom limits (points per image pixel): from a small overview to pixel-level.
const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 16.0;
/// Keep at least this many points of the image inside the canvas when panning, so
/// the picture can never be dragged completely out of view (foolproof controls).
const PAN_KEEP: f32 = 40.0;

/// Adjustable tool size range, in image pixels (the size slider's bounds).
const MIN_WIDTH: f32 = 1.0;
const MAX_WIDTH: f32 = 200.0;
/// Translucency of a highlighter stroke.
const HL_ALPHA: f32 = 0.4;
/// Undo depth. Each step is a full-image (or object-list) snapshot, bounding memory.
const MAX_UNDO: usize = 24;

/// Click tolerance for selecting an object, in screen points (scaled by zoom to
/// image pixels), so selecting stays forgiving at any zoom.
const HIT_TOL_PX: f32 = 6.0;
/// Grab tolerance for a resize handle, in screen points.
const HANDLE_TOL_PX: f32 = 9.0;
/// On-screen size of a selection handle square, in points.
const HANDLE_PX: f32 = 9.0;
/// Default object stroke width, in image pixels.
const DEFAULT_SHAPE_WIDTH: f32 = 4.0;

/// What the editor wants the host app to do after a UI frame.
pub enum EditorOutcome {
    /// Keep editing — nothing to do.
    Active,
    /// Flatten and save to the folder + copy to the clipboard, then return home.
    Save,
    /// Copy the current (flattened) image to the clipboard; keep editing.
    Copy,
    /// Throw the capture away and return home.
    Discard,
}

/// The active markup tool.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    /// Select / move / resize objects (and pan empty space) — the default.
    Select,
    Pen,
    Brush,
    Highlighter,
    Eraser,
    /// Draw a new overlay shape of this kind (P4.3).
    Shape(ShapeKind),
    /// Place a text object (P4.4).
    Text,
    /// Place a watermark (low-opacity text) object (P4.4).
    Watermark,
    /// Pick a colour from the image (P4.6).
    Eyedropper,
    /// Drag a rectangle to crop the image (P4.6).
    Crop,
}

impl Tool {
    /// Whether this tool paints freehand onto the raster (pen/brush/highlighter/eraser).
    fn is_raster(self) -> bool {
        matches!(
            self,
            Tool::Pen | Tool::Brush | Tool::Highlighter | Tool::Eraser
        )
    }
}

/// One reversible edit. Raster strokes/filters snapshot the image; object edits
/// snapshot the (small) object list — so moving an object never clones the image.
enum Edit {
    Raster(RgbaImage),
    Objects(Vec<Object>),
    /// Both layers at once — for transforms that flatten objects into the raster.
    Both(RgbaImage, Vec<Object>),
}

/// A one-shot transform action chosen from the Transform ▾ menu (P4.6).
#[derive(Clone, Copy)]
enum TxAction {
    RotateCw,
    RotateCcw,
    FlipH,
    FlipV,
    Bevel(u32),
    Crop,
}

/// What a primary-button drag on the canvas is doing (Select / Shape tools).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ObjDrag {
    None,
    /// Panning the view (Select tool, drag began on empty space).
    Pan,
    /// Moving the selected object.
    Move,
    /// Resizing the selected object via handle `usize`.
    Handle(usize),
    /// Drawing a new shape (the new object is selected).
    Create,
}

/// A live, undoable image filter (P4.5).
#[derive(Clone, Copy)]
enum Filter {
    Grayscale,
    Sepia,
    Invert,
    Blur,
    Sharpen,
    Brighten,
    Darken,
    MoreContrast,
    LessContrast,
    Posterize,
    Cartoonize,
}

impl Filter {
    /// The Filters ▾ menu, in order.
    const MENU: [(Filter, &'static str); 11] = [
        (Filter::Grayscale, "Grayscale"),
        (Filter::Sepia, "Sepia"),
        (Filter::Invert, "Invert"),
        (Filter::Blur, "Blur"),
        (Filter::Sharpen, "Sharpen"),
        (Filter::Brighten, "Brighten"),
        (Filter::Darken, "Darken"),
        (Filter::MoreContrast, "More contrast"),
        (Filter::LessContrast, "Less contrast"),
        (Filter::Posterize, "Posterize"),
        (Filter::Cartoonize, "Cartoonize"),
    ];

    fn apply(self, img: &RgbaImage) -> RgbaImage {
        match self {
            Filter::Grayscale => filters::grayscale(img),
            Filter::Sepia => filters::sepia(img),
            Filter::Invert => filters::invert(img),
            Filter::Blur => filters::box_blur(img, 2),
            Filter::Sharpen => filters::sharpen(img),
            Filter::Brighten => filters::brightness(img, 24),
            Filter::Darken => filters::brightness(img, -24),
            Filter::MoreContrast => filters::contrast(img, 1.2),
            Filter::LessContrast => filters::contrast(img, 0.82),
            Filter::Posterize => filters::posterize(img, 5),
            Filter::Cartoonize => filters::cartoonize(img),
        }
    }
}

/// A rendered text stamp + its GPU texture, cached by content (P4.4).
struct CachedText {
    /// Stamp size in image pixels (so the object's bounds stay correct).
    size: (u32, u32),
    /// `None` for empty text (nothing to draw).
    texture: Option<egui::TextureHandle>,
}

/// Highlighter behaviour.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HlMode {
    /// Translucent stroke over anything.
    Free,
    /// Highlight only detected text within the stroke band.
    TextAware,
}

/// Eraser behaviour.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EraseMode {
    /// Paint opaque white.
    White,
    /// Restore the original captured pixels (remove markup only).
    MarkupOnly,
}

/// A live editing session over one captured image.
pub struct EditorSession {
    /// The working raster — exactly what Save writes. Markup bakes here.
    image: RgbaImage,
    /// A pristine copy of the original capture, for the markup-only eraser.
    pristine: RgbaImage,
    /// GPU texture mirroring `image`, re-uploaded whenever the raster changes.
    texture: egui::TextureHandle,
    /// Zoom + pan of the canvas view.
    view: View,
    /// Active tool + its parameters.
    tool: Tool,
    /// Active markup colour (RGBA), seeded from the capture bar's colour.
    color: [u8; 4],
    /// Per-tool stroke widths (image pixels), so each tool remembers its size.
    pen_width: f32,
    brush_width: f32,
    hl_width: f32,
    eraser_width: f32,
    hl_mode: HlMode,
    erase_mode: EraseMode,
    /// Default width + fill for new shape objects.
    shape_width: f32,
    shape_fill: bool,
    /// Defaults for new text objects (P4.4).
    text_px: f32,
    text_family: FontFamily,
    /// Rendered text stamps + textures, keyed by content (string/size/family/colour).
    text_cache: HashMap<String, CachedText>,
    /// In-progress stroke points, in image-pixel coordinates (empty = idle).
    stroke: Vec<Pos2>,
    /// Movable overlay objects (P4.3), drawn over the raster, baked on Save.
    objects: Vec<Object>,
    /// Index of the selected object, if any.
    selected: Option<usize>,
    /// Current primary-drag gesture on the canvas.
    obj_drag: ObjDrag,
    /// Whether the current object gesture has already pushed its undo snapshot.
    obj_undo_pushed: bool,
    /// In-progress crop rectangle (image space): (drag start, current).
    crop_drag: Option<(Pos2, Pos2)>,
    /// Undo / redo history (raster image or object-list snapshots).
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// A short note shown in the status bar (e.g. "Copied to clipboard").
    notice: Option<String>,
}

/// The canvas view transform: how the image is placed inside the canvas rect.
struct View {
    /// Points per image pixel.
    zoom: f32,
    /// Image top-left relative to the canvas rect's min, in points.
    offset: Vec2,
    /// Cleared until the first real canvas size is known, so the opening frame
    /// fits the image to the available area before anything is drawn.
    initialized: bool,
}

impl EditorSession {
    /// Upload the capture and open a session viewing it. `color` seeds the markup
    /// colour (the capture bar's active colour).
    pub fn new(ctx: &egui::Context, image: RgbaImage, color: [u8; 4]) -> Self {
        let texture = upload(ctx, &image);
        let pristine = image.clone();
        Self {
            image,
            pristine,
            texture,
            view: View {
                zoom: 1.0,
                offset: Vec2::ZERO,
                initialized: false,
            },
            tool: Tool::Select,
            color,
            pen_width: 3.0,
            brush_width: 12.0,
            hl_width: 24.0,
            eraser_width: 16.0,
            hl_mode: HlMode::Free,
            erase_mode: EraseMode::White,
            shape_width: DEFAULT_SHAPE_WIDTH,
            shape_fill: false,
            text_px: 48.0,
            text_family: FontFamily::Sans,
            text_cache: HashMap::new(),
            stroke: Vec::new(),
            objects: Vec::new(),
            selected: None,
            obj_drag: ObjDrag::None,
            obj_undo_pushed: false,
            crop_drag: None,
            undo: Vec::new(),
            redo: Vec::new(),
            notice: None,
        }
    }

    /// Consume the session and return the flattened image (on Save).
    pub fn into_image(self) -> RgbaImage {
        self.baked()
    }

    /// A flattened copy of the current image (for Copy-to-clipboard while editing).
    pub fn flatten(&self) -> RgbaImage {
        self.baked()
    }

    /// The working raster with every overlay object baked in (Save flattening).
    fn baked(&self) -> RgbaImage {
        let mut img = self.image.clone();
        for obj in &self.objects {
            obj.bake_into(&mut img);
        }
        img
    }

    /// Image size in pixels, as a [`Vec2`].
    fn image_size(&self) -> Vec2 {
        egui::vec2(self.image.width() as f32, self.image.height() as f32)
    }

    /// Draw the editor and process input for one frame.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> EditorOutcome {
        // Keyboard: Ctrl/Cmd+Z undo, Ctrl/Cmd+Y or Ctrl/Cmd+Shift+Z redo.
        let (undo_key, redo_key) = ui.input(|i| {
            let cmd = i.modifiers.command;
            let undo = cmd && !i.modifiers.shift && i.key_pressed(egui::Key::Z);
            let redo = cmd
                && (i.key_pressed(egui::Key::Y)
                    || (i.modifiers.shift && i.key_pressed(egui::Key::Z)));
            (undo, redo)
        });
        if undo_key {
            self.undo();
        }
        if redo_key {
            self.redo();
        }

        // Esc: cancel an in-progress stroke; if nothing has been drawn yet, it
        // discards the capture (the Phase 3 behaviour). Once edits exist, Esc is
        // ignored so a stray press can't throw the work away — use Discard.
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            if !self.stroke.is_empty() {
                self.stroke.clear();
            } else if self.selected.is_some() {
                self.selected = None; // deselect first, before any discard
            } else if self.undo.is_empty() {
                return EditorOutcome::Discard;
            }
        }

        // Delete (or Backspace) removes the selected object.
        if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
            self.delete_selected();
        }

        let mut outcome = EditorOutcome::Active;
        egui::Panel::top("freally_toolbar2")
            .resizable(false)
            .show_inside(ui, |ui| self.tool_strip(ui));
        egui::Panel::bottom("freally_editor_actions")
            .resizable(false)
            .show_inside(ui, |ui| {
                if let Some(o) = self.action_bar(ui) {
                    outcome = o;
                }
            });
        egui::CentralPanel::default().show_inside(ui, |ui| self.canvas(ui));
        outcome
    }

    /// **Toolbar 2** — the markup tool strip plus the active tool's options.
    /// Pen / Brush / Highlighter / Eraser are live (P4.2); the rest are present
    /// but disabled, each labelled with the prompt that enables it.
    fn tool_strip(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        let mut chosen_filter: Option<Filter> = None;
        let mut chosen_tx: Option<TxAction> = None;
        ui.horizontal_wrapped(|ui| {
            self.tool_button(
                ui,
                Tool::Select,
                "Select",
                "Select, move and resize objects — drag empty space to pan, scroll to zoom",
            );
            ui.separator();
            self.tool_button(ui, Tool::Pen, "Pen", "Freehand pen");
            self.tool_button(ui, Tool::Brush, "Brush", "Thicker brush");
            self.tool_button(
                ui,
                Tool::Highlighter,
                "Highlighter",
                "Translucent highlighter — free or text-aware",
            );
            self.tool_button(
                ui,
                Tool::Eraser,
                "Eraser",
                "Eraser — erase to white, or remove markup only",
            );
            ui.separator();
            // Shapes ▾ — pick a kind, then drag on the image to draw it (P4.3).
            let shape_label = match self.tool {
                Tool::Shape(kind) => format!("Shape: {}", kind.label()),
                _ => "Shapes ▾".to_owned(),
            };
            ui.menu_button(shape_label, |ui| {
                for kind in ShapeKind::ALL {
                    if ui
                        .selectable_label(self.tool == Tool::Shape(kind), kind.label())
                        .clicked()
                    {
                        self.tool = Tool::Shape(kind);
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("Draw a rectangle, oval, line or arrow (drag on the image)");
            self.tool_button(ui, Tool::Text, "Text", "Add a text object (click to place)");
            self.tool_button(
                ui,
                Tool::Watermark,
                "Watermark",
                "Add a watermark — low-opacity text (click to place)",
            );
            disabled_tool(
                ui,
                "Emoji",
                "Emoji picker (colour) — arrives in Phase 4 (P4.7)",
            );
            disabled_tool(ui, "Image", "Place an image — arrives in Phase 4 (P4.8)");
            ui.separator();
            // Filters ▾ — live, undoable image filters (P4.5).
            ui.menu_button("Filters ▾", |ui| {
                for (filter, label) in Filter::MENU {
                    if ui.button(label).clicked() {
                        chosen_filter = Some(filter);
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("Apply a live, undoable filter to the image");
            // Transform ▾ — rotate / flip / bevel / crop (P4.6).
            ui.menu_button("Transform ▾", |ui| {
                if ui.button("Rotate left").clicked() {
                    chosen_tx = Some(TxAction::RotateCcw);
                    ui.close();
                }
                if ui.button("Rotate right").clicked() {
                    chosen_tx = Some(TxAction::RotateCw);
                    ui.close();
                }
                if ui.button("Flip horizontal").clicked() {
                    chosen_tx = Some(TxAction::FlipH);
                    ui.close();
                }
                if ui.button("Flip vertical").clicked() {
                    chosen_tx = Some(TxAction::FlipV);
                    ui.close();
                }
                ui.separator();
                ui.menu_button("Bevel", |ui| {
                    if ui.button("Thin").clicked() {
                        chosen_tx = Some(TxAction::Bevel(8));
                        ui.close();
                    }
                    if ui.button("Medium").clicked() {
                        chosen_tx = Some(TxAction::Bevel(16));
                        ui.close();
                    }
                    if ui.button("Thick").clicked() {
                        chosen_tx = Some(TxAction::Bevel(28));
                        ui.close();
                    }
                });
                ui.separator();
                if ui.button("Crop…").clicked() {
                    chosen_tx = Some(TxAction::Crop);
                    ui.close();
                }
            })
            .response
            .on_hover_text("Rotate, flip, bevel, or crop");
            self.tool_button(
                ui,
                Tool::Eyedropper,
                "Eyedropper",
                "Pick a colour from the image",
            );
            disabled_tool(
                ui,
                "Extract Text",
                "OCR the image to the clipboard — arrives in Phase 4 (P4.6b)",
            );
        });
        if let Some(filter) = chosen_filter {
            self.apply_filter(filter);
        }
        if let Some(action) = chosen_tx {
            self.apply_tx(action);
        }
        ui.add_space(2.0);
        ui.separator();
        self.tool_options(ui);
        ui.add_space(2.0);
    }

    /// A selectable button for a live tool.
    fn tool_button(&mut self, ui: &mut egui::Ui, tool: Tool, label: &str, hover: &str) {
        if ui
            .selectable_label(self.tool == tool, label)
            .on_hover_text(hover)
            .clicked()
        {
            self.tool = tool;
        }
    }

    /// Options for the active tool: the Select hint, a shape's size/colour/fill,
    /// or a raster tool's per-tool size + colour + mode toggles.
    fn tool_options(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        match self.tool {
            Tool::Select => {
                if self.selected_is_text() {
                    self.text_props_ui(ui);
                } else {
                    ui.label(
                        egui::RichText::new(
                            "Click an object to select · drag to move · drag a handle to resize · \
                             Delete to remove · drag empty space to pan",
                        )
                        .weak(),
                    );
                }
            }
            Tool::Shape(kind) => {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Size");
                    ui.add(
                        egui::Slider::new(&mut self.shape_width, MIN_WIDTH..=MAX_WIDTH)
                            .suffix(" px"),
                    )
                    .on_hover_text("Outline width in image pixels");
                    ui.separator();
                    ui.label("Color");
                    ui.color_edit_button_srgba_unmultiplied(&mut self.color)
                        .on_hover_text("Shape colour");
                    if kind.fillable() {
                        ui.separator();
                        ui.checkbox(&mut self.shape_fill, "Fill")
                            .on_hover_text("Fill the interior");
                    }
                });
            }
            Tool::Text | Tool::Watermark => self.text_defaults_ui(ui),
            Tool::Eyedropper => {
                ui.label(egui::RichText::new("Click a pixel to set the markup colour").weak());
            }
            Tool::Crop => {
                ui.label(egui::RichText::new("Drag a rectangle, then release to crop").weak());
            }
            _ => self.raster_options(ui),
        }
    }

    /// Whether the selected object is a text object.
    fn selected_is_text(&self) -> bool {
        self.selected
            .and_then(|i| self.objects.get(i))
            .is_some_and(|o| o.text().is_some())
    }

    /// Defaults shown while a text/watermark tool is armed (applied to new text).
    fn text_defaults_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Size");
            ui.add(egui::Slider::new(&mut self.text_px, 8.0..=240.0).suffix(" px"));
            ui.separator();
            ui.label("Font");
            family_combo(ui, "text_default_family", &mut self.text_family);
            ui.separator();
            ui.label("Color");
            ui.color_edit_button_srgba_unmultiplied(&mut self.color);
            ui.separator();
            ui.label(egui::RichText::new("Click on the image to place text").weak());
        });
    }

    /// Live editor for the selected text object's string / size / font / opacity.
    fn text_props_ui(&mut self, ui: &mut egui::Ui) {
        let Some(i) = self.selected else { return };
        if self.objects.get(i).and_then(|o| o.text()).is_none() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Text");
            let mut string = self.objects[i]
                .text()
                .map(|t| t.string.clone())
                .unwrap_or_default();
            if ui
                .add(egui::TextEdit::singleline(&mut string).desired_width(180.0))
                .changed()
            {
                if let Some(t) = self.objects[i].text_mut() {
                    t.string = string;
                }
            }
            ui.separator();
            ui.label("Size");
            let mut px = self.objects[i].text().map(|t| t.font_px).unwrap_or(48.0);
            if ui
                .add(egui::Slider::new(&mut px, 8.0..=240.0).suffix(" px"))
                .changed()
            {
                if let Some(t) = self.objects[i].text_mut() {
                    t.font_px = px;
                }
            }
            ui.separator();
            ui.label("Font");
            let mut family = self.objects[i]
                .text()
                .map(|t| t.family)
                .unwrap_or(FontFamily::Sans);
            if family_combo(ui, "text_sel_family", &mut family) {
                if let Some(t) = self.objects[i].text_mut() {
                    t.family = family;
                }
            }
            ui.separator();
            ui.label("Opacity");
            let mut alpha = self.objects[i].color[3];
            if ui.add(egui::Slider::new(&mut alpha, 0..=255)).changed() {
                self.objects[i].color[3] = alpha;
            }
            ui.separator();
            ui.label("Color");
            ui.color_edit_button_srgba_unmultiplied(&mut self.objects[i].color);
        });
    }

    /// Size + colour + mode toggles for the active raster tool (P4.2).
    fn raster_options(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            // Size — per-tool, so each tool keeps its own thickness.
            ui.label("Size");
            let mut width = self.width();
            if ui
                .add(egui::Slider::new(&mut width, MIN_WIDTH..=MAX_WIDTH).suffix(" px"))
                .on_hover_text("Stroke size in image pixels")
                .changed()
            {
                self.set_width(width);
            }

            // Colour — pen / brush / highlighter (the eraser has no colour).
            if !matches!(self.tool, Tool::Eraser) {
                ui.separator();
                ui.label("Color");
                ui.color_edit_button_srgba_unmultiplied(&mut self.color)
                    .on_hover_text("Markup colour");
            }

            // Mode toggles.
            match self.tool {
                Tool::Highlighter => {
                    ui.separator();
                    ui.selectable_value(&mut self.hl_mode, HlMode::Free, "Free")
                        .on_hover_text("Highlight anything under the stroke");
                    ui.selectable_value(&mut self.hl_mode, HlMode::TextAware, "Text-aware")
                        .on_hover_text("Highlight only detected text, sparing the background");
                }
                Tool::Eraser => {
                    ui.separator();
                    ui.selectable_value(&mut self.erase_mode, EraseMode::White, "To white")
                        .on_hover_text("Paint white");
                    ui.selectable_value(&mut self.erase_mode, EraseMode::MarkupOnly, "Markup only")
                        .on_hover_text("Restore the original captured pixels");
                }
                _ => {}
            }
        });
    }

    /// The bottom bar: zoom controls + status on the left, Undo/Redo and the file
    /// actions (Copy / Save / Discard) on the right.
    fn action_bar(&mut self, ui: &mut egui::Ui) -> Option<EditorOutcome> {
        let mut outcome = None;
        let (mut want_undo, mut want_redo) = (false, false);
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            // Zoom controls (left). ASCII "-"/"+" — the fullwidth/typographic
            // variants are tofu in egui's default fonts (see the capture bar).
            if ui.button(" - ").on_hover_text("Zoom out").clicked() {
                self.zoom_by(1.0 / 1.25, None);
            }
            ui.label(format!("{:.0}%", self.view.zoom * 100.0));
            if ui.button(" + ").on_hover_text("Zoom in").clicked() {
                self.zoom_by(1.25, None);
            }
            if ui
                .button("Fit")
                .on_hover_text("Fit the image to the window")
                .clicked()
            {
                self.view.initialized = false; // re-fit next frame
            }
            if ui
                .button("100%")
                .on_hover_text("Show the image at actual size")
                .clicked()
            {
                self.zoom_by(1.0 / self.view.zoom, None);
            }

            ui.separator();
            ui.label(
                egui::RichText::new(format!("{} × {}", self.image.width(), self.image.height()))
                    .weak(),
            );
            if let Some(notice) = &self.notice {
                ui.separator();
                ui.label(egui::RichText::new(notice).italics().weak());
            }

            // Undo/Redo + file actions (right).
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // 🗑 (U+1F5D1) renders via egui's bundled emoji-icon font, unlike
                // "✕" U+2715, which is tofu in the default fonts (see the capture bar).
                if ui
                    .button("🗑 Discard")
                    .on_hover_text("Throw this capture away")
                    .clicked()
                {
                    outcome = Some(EditorOutcome::Discard);
                }
                if ui
                    .button("Save")
                    .on_hover_text("Save to your folder and copy to the clipboard")
                    .clicked()
                {
                    outcome = Some(EditorOutcome::Save);
                }
                if ui
                    .button("Copy")
                    .on_hover_text("Copy the image to the clipboard")
                    .clicked()
                {
                    self.notice = Some("Copied to clipboard".to_owned());
                    outcome = Some(EditorOutcome::Copy);
                }
                ui.separator();
                if ui
                    .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"))
                    .on_hover_text("Redo (Ctrl+Y)")
                    .clicked()
                {
                    want_redo = true;
                }
                if ui
                    .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"))
                    .on_hover_text("Undo (Ctrl+Z)")
                    .clicked()
                {
                    want_undo = true;
                }
            });
        });
        if want_undo {
            self.undo();
        }
        if want_redo {
            self.redo();
        }
        ui.add_space(2.0);
        outcome
    }

    /// The zoom/pan canvas: a checkerboard backdrop (so transparency shows), the
    /// image, a border, and — with a drawing tool — the live stroke preview and a
    /// brush-size ring at the cursor.
    fn canvas(&mut self, ui: &mut egui::Ui) {
        let canvas = ui.max_rect();
        if canvas.width() <= 0.0 || canvas.height() <= 0.0 {
            return;
        }

        // First frame (or after "Fit"): fit the image to the canvas and center it.
        if !self.view.initialized {
            self.fit(canvas.size());
            self.view.initialized = true;
        }

        let response = ui.interact(
            canvas,
            ui.id().with("freally_editor_canvas"),
            Sense::click_and_drag(),
        );

        // Zoom: plain wheel or pinch, centered on the pointer.
        if response.hovered() {
            let (scroll_y, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let mut factor = pinch;
            if scroll_y != 0.0 {
                factor *= (scroll_y * 0.0015).exp();
            }
            if (factor - 1.0).abs() > f32::EPSILON {
                let pivot = response
                    .hover_pos()
                    .map(|p| p - canvas.min)
                    .unwrap_or_else(|| canvas.size() * 0.5);
                self.zoom_by(factor, Some(pivot));
            }
        }

        // Pan + draw (may bake a stroke and re-upload the texture this frame).
        self.handle_pointer(&response, canvas);

        self.view.offset = clamp_offset(
            self.view.offset,
            self.image_size() * self.view.zoom,
            canvas.size(),
            PAN_KEEP,
        );

        // Render/cache text stamps and keep their bounds in sync before drawing.
        let ctx = ui.ctx().clone();
        self.sync_text(&ctx);

        // Paint, clipped to the canvas.
        let painter = ui.painter_at(canvas);
        let image_rect = Rect::from_min_size(
            canvas.min + self.view.offset,
            self.image_size() * self.view.zoom,
        );
        let visible = canvas.intersect(image_rect);
        if visible.is_positive() {
            paint_checkerboard(&painter, visible, image_rect.min);
        }
        painter.image(
            self.texture.id(),
            image_rect,
            Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.rect_stroke(
            image_rect,
            0.0,
            Stroke::new(1.0, Color32::from_gray(90)),
            StrokeKind::Outside,
        );

        // Overlay objects (drawn over the raster, baked only on Save) + selection.
        let to_screen = |p: Pos2| self.image_to_screen(canvas, p);
        for obj in &self.objects {
            obj.draw(&painter, &to_screen, self.view.zoom);
            // Text objects draw from their cached texture (the editor owns the ctx).
            if let Some(t) = obj.text() {
                let key = text_key(&t.string, t.font_px, t.family, obj.color);
                if let Some(tex) = self.text_cache.get(&key).and_then(|c| c.texture.as_ref()) {
                    let rect = Rect::from_two_pos(to_screen(obj.a), to_screen(obj.b));
                    painter.image(
                        tex.id(),
                        rect,
                        Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
            }
        }
        if let Some(obj) = self.selected.and_then(|i| self.objects.get(i)) {
            objects::draw_selection(&painter, obj, &to_screen, HANDLE_PX);
        }

        // Crop rectangle preview (P4.6).
        if let Some((a, b)) = self.crop_drag {
            let r = Rect::from_two_pos(to_screen(a), to_screen(b));
            painter.rect_stroke(
                r,
                0.0,
                Stroke::new(2.0, Color32::from_rgb(40, 140, 255)),
                StrokeKind::Outside,
            );
        }

        self.draw_stroke_preview(&painter, canvas);
        self.draw_cursor_ring(&painter, canvas, &response);
    }

    /// Route the canvas pointer for one frame by tool. Middle-drag always pans.
    fn handle_pointer(&mut self, response: &egui::Response, canvas: Rect) {
        if response.dragged_by(PointerButton::Middle) {
            self.view.offset += response.drag_delta();
        }
        let pointer = response
            .interact_pointer_pos()
            .or_else(|| response.hover_pos())
            .map(|p| self.screen_to_image(canvas, p));
        match self.tool {
            Tool::Select => self.handle_select(response, pointer),
            Tool::Shape(kind) => self.handle_shape(response, kind, pointer),
            Tool::Text | Tool::Watermark => self.handle_text_tool(response, pointer),
            Tool::Eyedropper => self.handle_eyedropper(response, pointer),
            Tool::Crop => self.handle_crop(response, pointer),
            _ => self.handle_raster(response, pointer),
        }
    }

    /// Text / Watermark tool: a click places a new text object, then switches to
    /// the Select tool so it can be edited and moved.
    fn handle_text_tool(&mut self, response: &egui::Response, pointer: Option<Pos2>) {
        if !response.clicked_by(PointerButton::Primary) {
            return;
        }
        let Some(p) = pointer else { return };
        self.push_objects_undo();
        let watermark = self.tool == Tool::Watermark;
        let string = if watermark { "WATERMARK" } else { "Text" }.to_owned();
        let mut color = self.color;
        if watermark {
            color[3] = color[3].min(110); // watermarks are translucent by default
        }
        self.objects.push(Object {
            kind: Kind::Text(TextData {
                string,
                font_px: self.text_px,
                family: self.text_family,
                size: (0, 0),
            }),
            a: p,
            b: p,
            color,
            width: 0.0,
            fill: false,
        });
        self.selected = Some(self.objects.len() - 1);
        self.tool = Tool::Select;
    }

    /// Raster freehand drawing (P4.2): a primary drag paints a stroke; a click
    /// without a drag stamps a single dot.
    fn handle_raster(&mut self, response: &egui::Response, pointer: Option<Pos2>) {
        if response.drag_started_by(PointerButton::Primary) {
            self.stroke.clear();
            if let Some(p) = pointer {
                self.stroke.push(p);
            }
        } else if response.dragged_by(PointerButton::Primary) {
            if let Some(p) = pointer {
                // Thin the path: keep points ≥ 1 image px apart so the bake stays cheap.
                if self.stroke.last().is_none_or(|&l| (l - p).length() >= 1.0) {
                    self.stroke.push(p);
                }
            }
        } else if response.drag_stopped_by(PointerButton::Primary) {
            let points = std::mem::take(&mut self.stroke);
            self.commit_stroke(&points);
        } else if response.clicked_by(PointerButton::Primary) && self.stroke.is_empty() {
            if let Some(p) = pointer {
                self.commit_stroke(&[p]);
            }
        }
    }

    /// Select tool: click selects/deselects; a primary drag moves the object, drags
    /// a handle to resize, or pans when it began on empty space.
    fn handle_select(&mut self, response: &egui::Response, pointer: Option<Pos2>) {
        if response.clicked_by(PointerButton::Primary) {
            self.selected = pointer.and_then(|p| self.object_at(p));
            return;
        }
        if response.drag_started_by(PointerButton::Primary) {
            self.obj_undo_pushed = false;
            self.obj_drag = match pointer {
                Some(p) => {
                    if let Some(h) = self.selected.and_then(|i| self.handle_at(i, p)) {
                        ObjDrag::Handle(h)
                    } else if let Some(i) = self.object_at(p) {
                        self.selected = Some(i);
                        ObjDrag::Move
                    } else {
                        self.selected = None;
                        ObjDrag::Pan
                    }
                }
                None => ObjDrag::Pan,
            };
        }
        if response.dragged_by(PointerButton::Primary) {
            let delta = response.drag_delta();
            if delta != Vec2::ZERO {
                match self.obj_drag {
                    ObjDrag::Pan => self.view.offset += delta,
                    ObjDrag::Move => {
                        self.ensure_obj_undo();
                        if let Some(i) = self.selected {
                            self.objects[i].translate(delta / self.view.zoom);
                        }
                    }
                    ObjDrag::Handle(h) => {
                        self.ensure_obj_undo();
                        if let (Some(i), Some(p)) = (self.selected, pointer) {
                            self.objects[i].drag_handle(h, p);
                        }
                    }
                    _ => {}
                }
            }
        }
        if response.drag_stopped_by(PointerButton::Primary) {
            self.obj_drag = ObjDrag::None;
        }
    }

    /// Shape tool: a primary drag creates a new object; release finalizes it (and
    /// drops a too-small one), then returns to Select so it can be adjusted.
    fn handle_shape(&mut self, response: &egui::Response, kind: ShapeKind, pointer: Option<Pos2>) {
        if response.drag_started_by(PointerButton::Primary) {
            if let Some(p) = pointer {
                self.push_objects_undo();
                self.objects.push(Object {
                    kind: Kind::Shape(kind),
                    a: p,
                    b: p,
                    color: self.color,
                    width: self.shape_width,
                    fill: kind.fillable() && self.shape_fill,
                });
                self.selected = Some(self.objects.len() - 1);
                self.obj_drag = ObjDrag::Create;
            }
        } else if response.dragged_by(PointerButton::Primary) && self.obj_drag == ObjDrag::Create {
            if let (Some(i), Some(p)) = (self.selected, pointer) {
                self.objects[i].b = p;
            }
        } else if response.drag_stopped_by(PointerButton::Primary)
            && self.obj_drag == ObjDrag::Create
        {
            self.obj_drag = ObjDrag::None;
            // Drop a click-sized shape and the snapshot it pushed.
            if let Some(i) = self.selected {
                if (self.objects[i].a - self.objects[i].b).length() < 3.0 {
                    self.objects.remove(i);
                    self.selected = None;
                    self.undo.pop();
                }
            }
            self.tool = Tool::Select;
        }
    }

    /// Push an objects-undo snapshot once per gesture, on the first real change.
    fn ensure_obj_undo(&mut self) {
        if !self.obj_undo_pushed {
            self.push_objects_undo();
            self.obj_undo_pushed = true;
        }
    }

    /// Index of the top-most object hit at image point `p`.
    fn object_at(&self, p: Pos2) -> Option<usize> {
        let tol = HIT_TOL_PX / self.view.zoom;
        self.objects
            .iter()
            .enumerate()
            .rev()
            .find(|(_, o)| o.hit(p, tol))
            .map(|(i, _)| i)
    }

    /// The handle index of object `i` near image point `p`, if any.
    fn handle_at(&self, i: usize, p: Pos2) -> Option<usize> {
        let tol = HANDLE_TOL_PX / self.view.zoom;
        let obj = self.objects.get(i)?;
        obj.handles().into_iter().position(|h| h.distance(p) <= tol)
    }

    /// Remove the selected object (undoable).
    fn delete_selected(&mut self) {
        if let Some(i) = self.selected {
            if i < self.objects.len() {
                self.push_objects_undo();
                self.objects.remove(i);
                self.selected = None;
            }
        }
    }

    /// Bake the in-progress `points` (image space) into the raster as one undoable
    /// step, using the active tool's paint mode + size.
    fn commit_stroke(&mut self, points: &[Pos2]) {
        let Some(paint) = self.paint_for_tool() else {
            return;
        };
        if points.is_empty() {
            return;
        }
        self.push_raster_undo();
        let radius = self.width() / 2.0;
        let pts: Vec<(f32, f32)> = points.iter().map(|p| (p.x, p.y)).collect();
        raster::bake_stroke(&mut self.image, &self.pristine, &pts, radius, &paint);
        self.reupload();
        self.stroke.clear();
    }

    /// The active tool's paint mode (`None` for the Pan tool).
    fn paint_for_tool(&self) -> Option<Paint> {
        let rgb = [self.color[0], self.color[1], self.color[2]];
        Some(match self.tool {
            Tool::Pen | Tool::Brush => Paint::Solid(rgb),
            Tool::Highlighter => Paint::Highlight {
                color: rgb,
                alpha: HL_ALPHA,
                text_only: self.hl_mode == HlMode::TextAware,
            },
            Tool::Eraser => match self.erase_mode {
                EraseMode::White => Paint::White,
                EraseMode::MarkupOnly => Paint::Restore,
            },
            Tool::Select
            | Tool::Shape(_)
            | Tool::Text
            | Tool::Watermark
            | Tool::Eyedropper
            | Tool::Crop => return None,
        })
    }

    /// The active tool's stroke width (image pixels); 0 for non-raster tools.
    fn width(&self) -> f32 {
        match self.tool {
            Tool::Pen => self.pen_width,
            Tool::Brush => self.brush_width,
            Tool::Highlighter => self.hl_width,
            Tool::Eraser => self.eraser_width,
            Tool::Select
            | Tool::Shape(_)
            | Tool::Text
            | Tool::Watermark
            | Tool::Eyedropper
            | Tool::Crop => 0.0,
        }
    }

    /// Set the active tool's stroke width (clamped to the slider range).
    fn set_width(&mut self, width: f32) {
        let width = width.clamp(MIN_WIDTH, MAX_WIDTH);
        match self.tool {
            Tool::Pen => self.pen_width = width,
            Tool::Brush => self.brush_width = width,
            Tool::Highlighter => self.hl_width = width,
            Tool::Eraser => self.eraser_width = width,
            Tool::Select
            | Tool::Shape(_)
            | Tool::Text
            | Tool::Watermark
            | Tool::Eyedropper
            | Tool::Crop => {}
        }
    }

    /// Draw the in-progress stroke as a live overlay (committed pixels are already
    /// in the texture). Approximate — the bake on release is the source of truth.
    fn draw_stroke_preview(&self, painter: &egui::Painter, canvas: Rect) {
        if self.stroke.is_empty() {
            return;
        }
        let Some(paint) = self.paint_for_tool() else {
            return;
        };
        let color = preview_color(&paint);
        let width = (self.width() * self.view.zoom).max(1.0);
        let screen: Vec<Pos2> = self
            .stroke
            .iter()
            .map(|&p| self.image_to_screen(canvas, p))
            .collect();
        // Round caps: a dot at each end makes the polyline read as a brush stroke.
        painter.circle_filled(screen[0], width * 0.5, color);
        if screen.len() == 1 {
            return;
        }
        painter.add(egui::Shape::line(screen.clone(), Stroke::new(width, color)));
        if let Some(&last) = screen.last() {
            painter.circle_filled(last, width * 0.5, color);
        }
    }

    /// Draw a ring at the cursor showing the current brush size (raster tools).
    fn draw_cursor_ring(&self, painter: &egui::Painter, canvas: Rect, response: &egui::Response) {
        if !self.tool.is_raster() {
            return;
        }
        let Some(p) = response.hover_pos() else {
            return;
        };
        if !canvas.contains(p) {
            return;
        }
        let r = (self.width() * self.view.zoom * 0.5).max(2.0);
        // Two concentric rings (dark over light) read on any background.
        painter.circle_stroke(p, r, Stroke::new(1.5, Color32::from_white_alpha(200)));
        painter.circle_stroke(p, r, Stroke::new(0.75, Color32::from_black_alpha(200)));
    }

    /// Map an image-pixel position to a screen point.
    fn image_to_screen(&self, canvas: Rect, p: Pos2) -> Pos2 {
        canvas.min + self.view.offset + p.to_vec2() * self.view.zoom
    }

    /// Map a screen point to an image-pixel position.
    fn screen_to_image(&self, canvas: Rect, p: Pos2) -> Pos2 {
        ((p - canvas.min - self.view.offset) / self.view.zoom).to_pos2()
    }

    /// Snapshot the raster before a bake (bounded), clearing redo.
    fn push_raster_undo(&mut self) {
        self.push_undo(Edit::Raster(self.image.clone()));
    }

    /// Snapshot the object list before an object edit (bounded), clearing redo.
    fn push_objects_undo(&mut self) {
        self.push_undo(Edit::Objects(self.objects.clone()));
    }

    /// Snapshot both layers before a transform that flattens objects (P4.6).
    fn push_full_undo(&mut self) {
        self.push_undo(Edit::Both(self.image.clone(), self.objects.clone()));
    }

    fn push_undo(&mut self, edit: Edit) {
        self.undo.push(edit);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Undo the last edit (raster or object), pushing its inverse onto redo.
    fn undo(&mut self) {
        if let Some(edit) = self.undo.pop() {
            let inverse = self.apply_edit(edit);
            self.redo.push(inverse);
        }
    }

    /// Redo the last undone edit, pushing its inverse back onto undo.
    fn redo(&mut self) {
        if let Some(edit) = self.redo.pop() {
            let inverse = self.apply_edit(edit);
            self.undo.push(inverse);
        }
    }

    /// Restore the snapshot in `edit`, returning the displaced state (its inverse).
    fn apply_edit(&mut self, edit: Edit) -> Edit {
        match edit {
            Edit::Raster(image) => {
                let current = std::mem::replace(&mut self.image, image);
                self.reupload();
                Edit::Raster(current)
            }
            Edit::Objects(objects) => {
                let current = std::mem::replace(&mut self.objects, objects);
                self.selected = None; // the old index may no longer be valid
                Edit::Objects(current)
            }
            Edit::Both(image, objects) => {
                let prev_img = std::mem::replace(&mut self.image, image);
                let prev_objs = std::mem::replace(&mut self.objects, objects);
                self.selected = None;
                self.text_cache.clear();
                self.view.initialized = false; // re-fit (dimensions may have changed)
                self.reupload();
                Edit::Both(prev_img, prev_objs)
            }
        }
    }

    /// Apply a live filter to the raster as one undoable step (P4.5).
    fn apply_filter(&mut self, filter: Filter) {
        self.push_raster_undo();
        self.image = filter.apply(&self.image);
        self.reupload();
        self.notice = None;
    }

    /// Apply a transform menu action (P4.6). Crop just arms the Crop tool.
    fn apply_tx(&mut self, action: TxAction) {
        match action {
            TxAction::RotateCw => self.apply_geometry(transforms::rotate_cw),
            TxAction::RotateCcw => self.apply_geometry(transforms::rotate_ccw),
            TxAction::FlipH => self.apply_geometry(transforms::flip_h),
            TxAction::FlipV => self.apply_geometry(transforms::flip_v),
            TxAction::Bevel(width) => {
                // Bevel frames the photo under the objects — no flatten needed.
                self.push_raster_undo();
                self.image = transforms::bevel(&self.image, width);
                self.reupload();
                self.notice = None;
            }
            TxAction::Crop => self.tool = Tool::Crop,
        }
    }

    /// Flatten + geometrically transform the raster as one atomic undo step.
    /// Objects are baked in first (their coordinate space changes), so this is a
    /// `Both` edit.
    fn apply_geometry(&mut self, f: impl Fn(&RgbaImage) -> RgbaImage) {
        self.push_full_undo();
        self.flatten_objects_into_image();
        self.image = f(&self.image);
        self.selected = None;
        self.text_cache.clear();
        self.view.initialized = false; // re-fit (dimensions may have changed)
        self.reupload();
        self.notice = None;
    }

    /// Bake every overlay object into the raster and clear the object list.
    fn flatten_objects_into_image(&mut self) {
        for obj in &self.objects {
            obj.bake_into(&mut self.image);
        }
        self.objects.clear();
    }

    /// Crop the raster to the image-space rectangle (flattening objects first).
    fn apply_crop(&mut self, x: i32, y: i32, w: u32, h: u32) {
        if w < 2 || h < 2 {
            return;
        }
        self.push_full_undo();
        self.flatten_objects_into_image();
        self.image = transforms::crop(&self.image, x, y, w, h);
        self.selected = None;
        self.text_cache.clear();
        self.view.initialized = false;
        self.reupload();
        self.notice = None;
    }

    /// Eyedropper tool: a click sets the active markup colour from the pixel under
    /// the cursor (keeps the current opacity), and shows its hex.
    fn handle_eyedropper(&mut self, response: &egui::Response, pointer: Option<Pos2>) {
        if !response.clicked_by(PointerButton::Primary) {
            return;
        }
        let Some(p) = pointer else { return };
        let (x, y) = (p.x.floor() as i32, p.y.floor() as i32);
        if x < 0 || y < 0 || x as u32 >= self.image.width() || y as u32 >= self.image.height() {
            return;
        }
        let px = self.image.get_pixel(x as u32, y as u32).0;
        self.color = [px[0], px[1], px[2], self.color[3]];
        self.notice = Some(format!("Picked #{:02X}{:02X}{:02X}", px[0], px[1], px[2]));
    }

    /// Crop tool: drag a rectangle, then crop to it on release.
    fn handle_crop(&mut self, response: &egui::Response, pointer: Option<Pos2>) {
        if response.drag_started_by(PointerButton::Primary) {
            self.crop_drag = pointer.map(|p| (p, p));
        } else if response.dragged_by(PointerButton::Primary) {
            if let (Some(drag), Some(p)) = (self.crop_drag.as_mut(), pointer) {
                drag.1 = p;
            }
        } else if response.drag_stopped_by(PointerButton::Primary) {
            if let Some((a, b)) = self.crop_drag.take() {
                let r = Rect::from_two_pos(a, b);
                self.apply_crop(
                    r.min.x.round() as i32,
                    r.min.y.round() as i32,
                    r.width().round() as u32,
                    r.height().round() as u32,
                );
                self.tool = Tool::Select;
            }
        }
    }

    /// Render/cache each text object's stamp (by content) and keep its bounds in
    /// sync (`b = a + stamp size`). Called once per frame before drawing.
    fn sync_text(&mut self, ctx: &egui::Context) {
        let reqs: Vec<(usize, String)> = self
            .objects
            .iter()
            .enumerate()
            .filter_map(|(i, o)| {
                o.text()
                    .map(|t| (i, text_key(&t.string, t.font_px, t.family, o.color)))
            })
            .collect();

        for (i, key) in &reqs {
            if !self.text_cache.contains_key(key) {
                let o = &self.objects[*i];
                let t = o.text().expect("filtered to text objects");
                let entry = match text::render(&t.string, t.font_px, t.family, o.color) {
                    Some(stamp) => {
                        let size = (stamp.width(), stamp.height());
                        let img = egui::ColorImage::from_rgba_unmultiplied(
                            [size.0 as usize, size.1 as usize],
                            stamp.as_raw(),
                        );
                        let texture =
                            ctx.load_texture("freally_text", img, egui::TextureOptions::LINEAR);
                        CachedText {
                            size,
                            texture: Some(texture),
                        }
                    }
                    None => CachedText {
                        size: (0, 0),
                        texture: None,
                    },
                };
                self.text_cache.insert(key.clone(), entry);
            }
        }

        for (i, key) in &reqs {
            if let Some(size) = self.text_cache.get(key).map(|c| c.size) {
                let a = self.objects[*i].a;
                if let Some(t) = self.objects[*i].text_mut() {
                    t.size = size;
                }
                self.objects[*i].b = a + egui::vec2(size.0 as f32, size.1 as f32);
            }
        }

        // Bound memory: drop the whole cache if it grows large (re-rendered lazily).
        if self.text_cache.len() > 64 {
            self.text_cache.clear();
        }
    }

    /// Re-upload the working raster to the GPU texture after it changes.
    fn reupload(&mut self) {
        let size = [self.image.width() as usize, self.image.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, self.image.as_raw());
        self.texture.set(color, egui::TextureOptions::NEAREST);
    }

    /// Fit the image to `avail` (points) and center it.
    fn fit(&mut self, avail: Vec2) {
        self.view.zoom = fit_zoom(self.image_size(), avail);
        self.view.offset = centered_offset(self.image_size() * self.view.zoom, avail);
    }

    /// Multiply the zoom by `factor`, keeping the image point under `pivot`
    /// (canvas-local points; `None` keeps the image centered on the canvas-origin)
    /// fixed on screen. Clamped to [`MIN_ZOOM`, `MAX_ZOOM`].
    fn zoom_by(&mut self, factor: f32, pivot: Option<Vec2>) {
        let old = self.view.zoom;
        let new = (old * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        if new == old {
            return;
        }
        let pivot = pivot.unwrap_or(Vec2::ZERO);
        self.view.offset = zoom_about(self.view.offset, pivot, old, new);
        self.view.zoom = new;
    }
}

/// Upload an RGBA image as an egui texture (nearest-neighbour, so zoomed-in pixels
/// stay crisp for pixel-level editing).
fn upload(ctx: &egui::Context, image: &RgbaImage) -> egui::TextureHandle {
    let size = [image.width() as usize, image.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    ctx.load_texture("freally_editor_image", color, egui::TextureOptions::NEAREST)
}

/// A disabled Toolbar 2 tool button (present, but enabled in a later prompt).
fn disabled_tool(ui: &mut egui::Ui, label: &str, arrives: &str) {
    ui.add_enabled(false, egui::Button::new(label))
        .on_disabled_hover_text(arrives);
}

/// A font-family combo box; returns `true` if the selection changed.
fn family_combo(ui: &mut egui::Ui, id: &str, family: &mut FontFamily) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(family.label())
        .show_ui(ui, |ui| {
            for f in FontFamily::ALL {
                changed |= ui.selectable_value(family, f, f.label()).changed();
            }
        });
    changed
}

/// Content key for the text-stamp cache (string + size + family + colour), so the
/// stamp/texture are recomputed only when one of those changes.
fn text_key(string: &str, font_px: f32, family: FontFamily, color: [u8; 4]) -> String {
    format!(
        "{}\u{1}{}\u{1}{:?}\u{1}{:?}",
        string,
        font_px.to_bits(),
        family,
        color
    )
}

/// Screen colour for the live stroke preview of a given paint mode.
fn preview_color(paint: &Paint) -> Color32 {
    match paint {
        Paint::Solid([r, g, b]) => Color32::from_rgb(*r, *g, *b),
        Paint::Highlight { color, alpha, .. } => {
            Color32::from_rgba_unmultiplied(color[0], color[1], color[2], (alpha * 255.0) as u8)
        }
        // Eraser previews as a neutral swept band (the bake is the real result).
        Paint::White => Color32::from_rgba_unmultiplied(255, 255, 255, 180),
        Paint::Restore => Color32::from_rgba_unmultiplied(128, 128, 128, 140),
    }
}

/// Zoom (points per pixel) that fits an `img`-pixel image within `avail` points.
fn fit_zoom(img: Vec2, avail: Vec2) -> f32 {
    if img.x <= 0.0 || img.y <= 0.0 {
        return 1.0;
    }
    (avail.x / img.x)
        .min(avail.y / img.y)
        .clamp(MIN_ZOOM, MAX_ZOOM)
}

/// Offset that centers an already-scaled image (`scaled` points) within `avail`.
fn centered_offset(scaled: Vec2, avail: Vec2) -> Vec2 {
    (avail - scaled) * 0.5
}

/// New offset after zooming from `old` to `new` while keeping the image point
/// currently under `pivot` (canvas-local points) anchored under the cursor.
fn zoom_about(offset: Vec2, pivot: Vec2, old: f32, new: f32) -> Vec2 {
    // image_pt = (pivot - offset) / old  must stay under `pivot` after zoom:
    // offset' = pivot - image_pt * new
    pivot - (pivot - offset) * (new / old)
}

/// Clamp the pan `offset` so at least `keep` points of the `scaled`-size image
/// stay within an `avail`-size canvas on each axis.
fn clamp_offset(offset: Vec2, scaled: Vec2, avail: Vec2, keep: f32) -> Vec2 {
    let clamp_axis = |off: f32, size: f32, span: f32| {
        let lo = keep - size; // image's right/bottom edge ≥ `keep` from the left/top
        let hi = span - keep; // image's left/top edge ≤ `keep` from the right/bottom
        if lo > hi {
            (lo + hi) * 0.5
        } else {
            off.clamp(lo, hi)
        }
    };
    egui::vec2(
        clamp_axis(offset.x, scaled.x, avail.x),
        clamp_axis(offset.y, scaled.y, avail.y),
    )
}

/// Tile a checkerboard over `area`, with cells aligned to `origin` so the pattern
/// doesn't shimmer while panning. Makes transparent (e.g. freeform-masked) pixels
/// visible behind the image.
fn paint_checkerboard(painter: &egui::Painter, area: Rect, origin: Pos2) {
    const CELL: f32 = 12.0;
    let light = Color32::from_gray(210);
    let dark = Color32::from_gray(170);
    painter.rect_filled(area, 0.0, light);

    // First cell index covering the area, measured from `origin`.
    let start_i = ((area.min.x - origin.x) / CELL).floor() as i64;
    let start_j = ((area.min.y - origin.y) / CELL).floor() as i64;
    let mut j = start_j;
    loop {
        let y0 = origin.y + j as f32 * CELL;
        if y0 >= area.max.y {
            break;
        }
        let mut i = start_i;
        loop {
            let x0 = origin.x + i as f32 * CELL;
            if x0 >= area.max.x {
                break;
            }
            // Paint only the dark squares over the light base.
            if (i + j) & 1 != 0 {
                let cell = Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x0 + CELL, y0 + CELL))
                    .intersect(area);
                if cell.is_positive() {
                    painter.rect_filled(cell, 0.0, dark);
                }
            }
            i += 1;
        }
        j += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_set() {
        assert_eq!(CRATE_NAME, "freally-editor");
    }

    #[test]
    fn fit_zoom_fills_the_smaller_axis_and_clamps() {
        // 200×100 image into a 400×400 box → limited by width (×2).
        assert_eq!(
            fit_zoom(egui::vec2(200.0, 100.0), egui::vec2(400.0, 400.0)),
            2.0
        );
        // 100×200 into 400×400 → limited by height (×2).
        assert_eq!(
            fit_zoom(egui::vec2(100.0, 200.0), egui::vec2(400.0, 400.0)),
            2.0
        );
        // Degenerate sizes fall back to 1.0 and clamp to the zoom range.
        assert_eq!(
            fit_zoom(egui::vec2(0.0, 0.0), egui::vec2(400.0, 400.0)),
            1.0
        );
        assert_eq!(
            fit_zoom(egui::vec2(1.0, 1.0), egui::vec2(9999.0, 9999.0)),
            MAX_ZOOM
        );
    }

    #[test]
    fn centered_offset_centers_the_scaled_image() {
        assert_eq!(
            centered_offset(egui::vec2(100.0, 50.0), egui::vec2(300.0, 250.0)),
            egui::vec2(100.0, 100.0)
        );
    }

    #[test]
    fn zoom_about_keeps_the_pivot_point_fixed() {
        // Pivot at (50, 50); the image point under it must map back to (50, 50).
        let offset = egui::vec2(10.0, 20.0);
        let pivot = egui::vec2(50.0, 50.0);
        let (old, new) = (1.0, 2.0);
        let img_pt = (pivot - offset) / old;
        let off2 = zoom_about(offset, pivot, old, new);
        let back = off2 + img_pt * new; // screen position of that image point
        assert!((back.x - pivot.x).abs() < 1e-3 && (back.y - pivot.y).abs() < 1e-3);
    }

    #[test]
    fn clamp_offset_keeps_image_partly_in_view() {
        // Tiny image, big canvas: can't push it past the keep margin on either side.
        let scaled = egui::vec2(20.0, 20.0);
        let avail = egui::vec2(800.0, 600.0);
        let keep = 40.0;
        // Far off to the left/top is pulled back so ≥ keep stays visible.
        let c = clamp_offset(egui::vec2(-1000.0, -1000.0), scaled, avail, keep);
        assert_eq!(c, egui::vec2(keep - scaled.x, keep - scaled.y));
        // Far off to the right/bottom is pulled back to the opposite limit.
        let c = clamp_offset(egui::vec2(5000.0, 5000.0), scaled, avail, keep);
        assert_eq!(c, egui::vec2(avail.x - keep, avail.y - keep));
    }
}

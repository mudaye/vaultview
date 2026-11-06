//! Platform-neutral theme definitions, visual tokens, and style setup.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, Margin, Shadow, Stroke, Vec2, Visuals,
};

/// Deep neutral background for the canvas and input areas.
pub const BG_BASE: Color32 = Color32::from_rgb(0x12, 0x12, 0x14);
/// Surface color for panels (side, top, and bottom).
pub const BG_PANEL: Color32 = Color32::from_rgb(0x18, 0x18, 0x1B);
/// Background color for the media viewer viewport.
pub const BG_VIEWER: Color32 = Color32::from_rgb(0x14, 0x14, 0x16);
/// Surface color for dialog windows and popup frames.
pub const BG_SURFACE: Color32 = Color32::from_rgb(0x20, 0x20, 0x24);
/// Surface color for hovered interactive elements.
pub const BG_ELEVATED: Color32 = Color32::from_rgb(0x27, 0x27, 0x2A);
/// Surface color for pressed or active elements.
pub const BG_ACTIVE: Color32 = Color32::from_rgb(0x32, 0x32, 0x38);

/// Subtle borders and dividers.
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(0x2E, 0x2E, 0x33);
/// Distinct boundary borders for windows and inputs.
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x3F, 0x3F, 0x46);

/// High-contrast primary text and icons.
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xF4, 0xF4, 0xF5);
/// Secondary text for labels and metadata.
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0xA1, 0xA1, 0xAA);
/// Muted text for disabled controls, placeholders, and weak info.
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x71, 0x71, 0x7A);

/// Accent color for selection and focus indicators.
pub const ACCENT_PRIMARY: Color32 = Color32::from_rgb(0x63, 0x66, 0xF1);
/// Hovered accent color.
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x81, 0x8C, 0xF8);
/// Active accent color.
pub const ACCENT_ACTIVE: Color32 = Color32::from_rgb(0x4F, 0x46, 0xE5);

/// Semantic success color.
pub const SEMANTIC_GOOD: Color32 = Color32::from_rgb(0x22, 0xC5, 0x5E);
/// Semantic warning color.
pub const SEMANTIC_WARN: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B);
/// Semantic error or dangerous action color.
pub const SEMANTIC_BAD: Color32 = Color32::from_rgb(0xEF, 0x44, 0x44);
/// Semantic color indicating encrypted state.
pub const SEMANTIC_GOLD: Color32 = Color32::from_rgb(0xEA, 0xB3, 0x08);
/// Semantic color indicating directory entries.
pub const SEMANTIC_FOLDER: Color32 = Color32::from_rgb(0x94, 0xA3, 0xB8);

/// Standard small icon size.
pub const ICON_SIZE_SM: f32 = 14.0;
/// Standard medium icon size.
pub const ICON_SIZE_MD: f32 = 16.0;
/// Standard large icon size.
pub const ICON_SIZE_LG: f32 = 20.0;

/// Applies the platform-neutral visual theme and registers the bundled icon font.
pub fn apply(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "lucide".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(crate::icons::FONT_BYTES)),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .push("lucide".to_owned());
    fonts
        .families
        .entry(FontFamily::Name("lucide".into()))
        .or_default()
        .push("lucide".to_owned());
    ctx.set_fonts(fonts);

    let mut visuals = Visuals::dark();
    visuals.dark_mode = true;
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.panel_fill = BG_PANEL;
    visuals.window_fill = BG_SURFACE;
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER_STRONG);
    visuals.window_corner_radius = CornerRadius::same(6);
    visuals.window_shadow = Shadow::NONE;
    visuals.popup_shadow = Shadow::NONE;
    visuals.extreme_bg_color = BG_BASE;
    visuals.hyperlink_color = ACCENT_HOVER;
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(99, 102, 241, 60);
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT_PRIMARY);

    visuals.widgets.noninteractive.bg_fill = BG_PANEL;
    visuals.widgets.noninteractive.weak_bg_fill = BG_PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(4);

    visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(4);

    visuals.widgets.hovered.bg_fill = BG_ELEVATED;
    visuals.widgets.hovered.weak_bg_fill = BG_ELEVATED;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, BORDER_STRONG);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(4);

    visuals.widgets.active.bg_fill = BG_ACTIVE;
    visuals.widgets.active.weak_bg_fill = BG_ACTIVE;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT_PRIMARY);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.active.corner_radius = CornerRadius::same(4);

    visuals.widgets.open.bg_fill = BG_ACTIVE;
    visuals.widgets.open.weak_bg_fill = BG_ACTIVE;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, BORDER_STRONG);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.open.corner_radius = CornerRadius::same(4);

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(8.0, 4.0);
    style.spacing.window_margin = Margin::same(12);
    style.spacing.interact_size = Vec2::new(36.0, 22.0);
    ctx.set_style(style);
}

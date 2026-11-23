use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, RichText,
    Shadow, Stroke, TextStyle, Vec2,
};
use image::ImageFormat;
use qrcode::QrCode;
use url::{form_urlencoded, Url};
use zeroize::Zeroize;

use crate::camera::{CameraSession, CameraUpdate};
use crate::migration::decode_migration_uri;
use crate::model::{sanitize_terminal_text, OtpAccount, QrClassification};
use crate::otpauth::parse_otpauth_uri;
use crate::qrcode::read_and_classify_qr_image;

pub const APP_DISPLAY_NAME: &str = "OTP Tools";
pub const APP_AUTHOR: &str = "Glauco G. Cetara";
pub const APP_EMAIL: &str = "glaucogcetara@gmail.com";
pub const APP_REPO_URL: &str = "https://github.com/c3t4r4/otp-tools.git";
pub const APP_LICENSE: &str = "MIT License";

const MIGRATION_INPUT_ID: &str = "migration_input_text_edit";
const OTPAUTH_INPUT_ID: &str = "otpauth_input_text_edit";
const CARD_FRAME_CHROME_W: f32 = 50.0;
const APP_ICON_ICO_BYTES: &[u8] = include_bytes!("../assets/AppIcon.ico");

const PROPORTIONAL_FONT_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/HelveticaNeue.ttc",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
];

const MONOSPACE_FONT_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Menlo.ttc",
    "/System/Library/Fonts/Monaco.ttf",
    "C:\\Windows\\Fonts\\consola.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiTab {
    QrCode,
    DecodeMigration,
    ParseOtpAuth,
    About,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuiResult {
    Empty,
    QrResults {
        source_label: String,
        items: Vec<QrClassification>,
    },
    MigrationAccounts(Vec<OtpAccount>),
    OtpAuthAccount(OtpAccount),
    Error(String),
}

#[derive(Debug)]
pub struct GuiState {
    pub active_tab: GuiTab,
    pub migration_input: String,
    pub otpauth_input: String,
    pub result: GuiResult,
    pub reveal_secrets: bool,
    pub revealed_qr_ids: HashSet<usize>,
    pub dark_mode: bool,
    pub status_banner: Option<String>,
    pub result_generation: u64,
}

impl Default for GuiState {
    fn default() -> Self {
        Self {
            active_tab: GuiTab::QrCode,
            migration_input: String::new(),
            otpauth_input: String::new(),
            result: GuiResult::Empty,
            reveal_secrets: false,
            revealed_qr_ids: HashSet::new(),
            dark_mode: true,
            status_banner: None,
            result_generation: 0,
        }
    }
}

impl Drop for GuiState {
    fn drop(&mut self) {
        self.zeroize_sensitive_fields();
    }
}

impl GuiState {
    fn zeroize_account(account: &mut OtpAccount) {
        account.secret.zeroize();
        account.uri.zeroize();
        account.label.zeroize();
        if let Some(issuer) = &mut account.issuer {
            issuer.zeroize();
        }
    }

    fn zeroize_result(&mut self) {
        match &mut self.result {
            GuiResult::Empty => {}
            GuiResult::Error(msg) => msg.zeroize(),
            GuiResult::OtpAuthAccount(account) => Self::zeroize_account(account),
            GuiResult::MigrationAccounts(accounts) => {
                for account in accounts {
                    Self::zeroize_account(account);
                }
            }
            GuiResult::QrResults {
                source_label,
                items,
            } => {
                source_label.zeroize();
                for item in items {
                    match item {
                        QrClassification::OtpAuth(account) => Self::zeroize_account(account),
                        QrClassification::Migration { raw_uri, accounts } => {
                            raw_uri.zeroize();
                            for account in accounts {
                                Self::zeroize_account(account);
                            }
                        }
                        QrClassification::Invalid {
                            raw_content,
                            reason,
                        } => {
                            raw_content.zeroize();
                            reason.zeroize();
                        }
                    }
                }
            }
        }
        self.result = GuiResult::Empty;
        self.revealed_qr_ids.clear();
        self.result_generation = self.result_generation.wrapping_add(1);
    }

    fn zeroize_sensitive_fields(&mut self) {
        self.migration_input.zeroize();
        self.otpauth_input.zeroize();
        self.zeroize_result();
    }

    pub fn process_qr_path(&mut self, path: &Path) {
        self.status_banner = None;
        self.zeroize_result();

        let label = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(sanitize_terminal_text)
            .unwrap_or_else(|| sanitize_terminal_text(&path.display().to_string()));

        match read_and_classify_qr_image(path) {
            Ok(items) => {
                self.result = GuiResult::QrResults {
                    source_label: label,
                    items,
                };
            }
            Err(err) => {
                self.result = GuiResult::Error(sanitize_terminal_text(&err.to_string()));
            }
        }
    }

    pub fn process_camera_classifications(&mut self, items: Vec<QrClassification>) {
        self.status_banner = None;
        self.zeroize_result();
        self.result = GuiResult::QrResults {
            source_label: "Câmera (Ao vivo)".to_string(),
            items,
        };
        self.status_banner = Some("QR Code lido com sucesso pela câmera!".to_string());
    }

    pub fn process_camera_error(&mut self, err_msg: &str) {
        self.status_banner = None;
        self.zeroize_result();
        self.result = GuiResult::Error(sanitize_terminal_text(err_msg));
    }

    pub fn process_migration(&mut self) {
        self.status_banner = None;
        self.zeroize_result();

        let input = self.migration_input.trim();
        if input.is_empty() {
            self.result =
                GuiResult::Error("Informe um link otpauth-migration://offline?data=...".into());
            return;
        }

        match decode_migration_uri(input) {
            Ok(accounts) => {
                self.result = GuiResult::MigrationAccounts(accounts);
            }
            Err(err) => {
                self.result = GuiResult::Error(sanitize_terminal_text(&err.to_string()));
            }
        }
    }

    pub fn process_otpauth(&mut self) {
        self.status_banner = None;
        self.zeroize_result();

        let input = self.otpauth_input.trim();
        if input.is_empty() {
            self.result =
                GuiResult::Error("Informe um link otpauth://totp/... ou otpauth://hotp/...".into());
            return;
        }

        match parse_otpauth_uri(input) {
            Ok(account) => {
                self.result = GuiResult::OtpAuthAccount(account);
            }
            Err(err) => {
                self.result = GuiResult::Error(sanitize_terminal_text(&err.to_string()));
            }
        }
    }

    pub fn clear_all(&mut self) {
        self.zeroize_sensitive_fields();
        self.reveal_secrets = false;
        self.status_banner = Some("Dados, QR Codes e segredos limpos da tela.".to_string());
    }

    pub fn clear_migration_tab(&mut self) {
        self.migration_input.zeroize();
        self.zeroize_result();
        self.reveal_secrets = false;
        self.status_banner = None;
    }

    pub fn clear_otpauth_tab(&mut self) {
        self.otpauth_input.zeroize();
        self.zeroize_result();
        self.reveal_secrets = false;
        self.status_banner = None;
    }

    pub fn toggle_qr_visibility(&mut self, id_salt: usize) {
        if !self.revealed_qr_ids.remove(&id_salt) {
            self.revealed_qr_ids.insert(id_salt);
        }
    }
}

/// Remove o estado interno (incluindo histórico de undo) dos campos de texto no `egui::Context`.
pub fn clear_input_undo_history(ctx: &egui::Context) {
    ctx.data_mut(|data| {
        data.remove::<egui::widgets::text_edit::TextEditState>(egui::Id::new(MIGRATION_INPUT_ID));
        data.remove::<egui::widgets::text_edit::TextEditState>(egui::Id::new(OTPAUTH_INPUT_ID));
    });
}

/// Mascara um segredo Base32: segredos com até 8 caracteres são totalmente ocultados;
/// segredos maiores exibem apenas os 4 primeiros caracteres.
pub fn mask_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 8 {
        return "•".repeat(chars.len().max(4));
    }
    let visible: String = chars[..4].iter().collect();
    format!("{visible}{}", "•".repeat(chars.len() - 4))
}

fn mask_secret_ascii(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 8 {
        return "*".repeat(chars.len().max(4));
    }
    let visible: String = chars[..4].iter().collect();
    format!("{visible}{}", "*".repeat(chars.len() - 4))
}

/// Mascara o parâmetro `secret=` dentro de uma URI `otpauth://...` para exibição segura
/// quando `reveal_secrets == false`, preservando o restante da URI sem percent-encoding extra.
pub fn mask_otpauth_uri_secret(uri: &str) -> String {
    let Ok(parsed) = Url::parse(uri) else {
        return mask_secret_ascii(uri);
    };

    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (key, value) in parsed.query_pairs() {
        if key.eq_ignore_ascii_case("secret") {
            serializer.append_pair(&key, &mask_secret_ascii(value.trim()));
        } else {
            serializer.append_pair(&key, &value);
        }
    }

    let query = serializer.finish();
    let mut base = parsed;
    base.set_query(None);
    if query.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{query}")
    }
}

/// Gera uma `egui::ColorImage` nítida de QR Code (com quiet zone branca de 4 módulos)
/// para uma URI `otpauth://...`, pronta para leitura por aplicativos autenticadores.
pub fn generate_qr_color_image(uri: &str) -> Result<egui::ColorImage, String> {
    let code = QrCode::new(uri.as_bytes()).map_err(|err| err.to_string())?;
    let modules = code.width();
    let quiet = 4usize;
    let scale = 4usize;
    let total_modules = modules + quiet * 2;
    let pixel_size = total_modules * scale;

    let mut pixels = vec![Color32::WHITE; pixel_size * pixel_size];
    for y in 0..modules {
        for x in 0..modules {
            if code[(x, y)] == qrcode::Color::Dark {
                let start_x = (x + quiet) * scale;
                let start_y = (y + quiet) * scale;
                for dy in 0..scale {
                    let row = (start_y + dy) * pixel_size;
                    for dx in 0..scale {
                        pixels[row + start_x + dx] = Color32::BLACK;
                    }
                }
            }
        }
    }

    Ok(egui::ColorImage {
        size: [pixel_size, pixel_size],
        pixels,
    })
}

/// Carrega o ícone `assets/AppIcon.ico` embutido em tempo de compilação para uso em runtime.
pub fn load_app_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory_with_format(APP_ICON_ICO_BYTES, ImageFormat::Ico).ok()?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some(egui::IconData {
        rgba: rgba.into_raw(),
        width,
        height,
    })
}

fn load_first_system_font(candidates: &[&str]) -> Option<Vec<u8>> {
    for candidate in candidates {
        if let Ok(bytes) = std::fs::read(candidate) {
            if !bytes.is_empty() {
                return Some(bytes);
            }
        }
    }
    None
}

/// Configura fontes estáticas do sistema (sem tabela `fvar` para métricas exatas no `ab_glyph`)
/// e mantém o anti-aliasing vetorial (`feathering = true`) para curvas e textos limpos.
pub fn configure_crisp_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    if let Some(bytes) = load_first_system_font(PROPORTIONAL_FONT_CANDIDATES) {
        fonts.font_data.insert(
            "system_sans".to_owned(),
            Arc::new(FontData::from_owned(bytes)),
        );
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "system_sans".to_owned());
    }

    if let Some(bytes) = load_first_system_font(MONOSPACE_FONT_CANDIDATES) {
        fonts.font_data.insert(
            "system_mono".to_owned(),
            Arc::new(FontData::from_owned(bytes)),
        );
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "system_mono".to_owned());
    }

    ctx.set_fonts(fonts);
    ctx.tessellation_options_mut(|opts| {
        opts.feathering = true;
    });
}

#[derive(Clone, Copy)]
struct ThemePalette {
    bg_top_left: Color32,
    bg_top_right: Color32,
    bg_bottom_left: Color32,
    bg_bottom_right: Color32,
    sidebar_bg: Color32,
    card_bg: Color32,
    card_stroke: Color32,
    well_bg: Color32,
    well_stroke: Color32,
    accent: Color32,
    accent_fill: Color32,
    secondary_badge: Color32,
    accent_text: Color32,
    text_primary: Color32,
    muted_text: Color32,
    success: Color32,
    warning: Color32,
    danger: Color32,
    pill_bg: Color32,
    pill_active_bg: Color32,
    pill_active_stroke: Color32,
    card_shadow: Shadow,
}

fn palette_for(dark_mode: bool) -> ThemePalette {
    if dark_mode {
        ThemePalette {
            bg_top_left: Color32::from_rgb(23, 8, 58),
            bg_top_right: Color32::from_rgb(62, 16, 118),
            bg_bottom_left: Color32::from_rgb(44, 11, 94),
            bg_bottom_right: Color32::from_rgb(104, 24, 165),
            sidebar_bg: Color32::from_rgba_unmultiplied(18, 6, 46, 155),
            card_bg: Color32::from_rgba_unmultiplied(38, 14, 84, 175),
            card_stroke: Color32::from_rgba_unmultiplied(233, 213, 255, 48),
            well_bg: Color32::from_rgba_unmultiplied(22, 7, 52, 170),
            well_stroke: Color32::from_rgba_unmultiplied(216, 180, 254, 38),
            accent: Color32::from_rgb(139, 104, 255),
            accent_fill: Color32::from_rgb(109, 67, 246),
            secondary_badge: Color32::from_rgb(99, 102, 241),
            accent_text: Color32::WHITE,
            text_primary: Color32::WHITE,
            muted_text: Color32::from_rgb(221, 204, 245),
            success: Color32::from_rgb(16, 185, 129),
            warning: Color32::from_rgb(253, 224, 71),
            danger: Color32::from_rgb(244, 63, 94),
            pill_bg: Color32::from_rgba_unmultiplied(255, 255, 255, 26),
            pill_active_bg: Color32::from_rgba_unmultiplied(255, 255, 255, 38),
            pill_active_stroke: Color32::from_rgba_unmultiplied(255, 255, 255, 68),
            card_shadow: Shadow {
                offset: [0, 8],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(90),
            },
        }
    } else {
        ThemePalette {
            bg_top_left: Color32::from_rgb(243, 232, 255),
            bg_top_right: Color32::from_rgb(233, 229, 255),
            bg_bottom_left: Color32::from_rgb(237, 233, 254),
            bg_bottom_right: Color32::from_rgb(224, 218, 254),
            sidebar_bg: Color32::from_rgba_unmultiplied(255, 255, 255, 185),
            card_bg: Color32::from_rgba_unmultiplied(255, 255, 255, 225),
            card_stroke: Color32::from_rgba_unmultiplied(124, 92, 246, 70),
            well_bg: Color32::from_rgba_unmultiplied(245, 240, 255, 230),
            well_stroke: Color32::from_rgba_unmultiplied(124, 92, 246, 55),
            accent: Color32::from_rgb(79, 57, 218),
            accent_fill: Color32::from_rgb(99, 72, 238),
            secondary_badge: Color32::from_rgb(109, 82, 242),
            accent_text: Color32::WHITE,
            text_primary: Color32::from_rgb(30, 12, 64),
            muted_text: Color32::from_rgb(91, 68, 130),
            success: Color32::from_rgb(5, 150, 105),
            warning: Color32::from_rgb(180, 83, 9),
            danger: Color32::from_rgb(225, 29, 72),
            pill_bg: Color32::from_rgba_unmultiplied(99, 72, 238, 22),
            pill_active_bg: Color32::from_rgba_unmultiplied(99, 72, 238, 40),
            pill_active_stroke: Color32::from_rgba_unmultiplied(99, 72, 238, 95),
            card_shadow: Shadow {
                offset: [0, 6],
                blur: 18,
                spread: 0,
                color: Color32::from_black_alpha(20),
            },
        }
    }
}

fn paint_background_gradient(ctx: &egui::Context, palette: ThemePalette) {
    let rect = ctx.screen_rect();
    let mut mesh = egui::Mesh::default();
    let white_uv = egui::epaint::WHITE_UV;
    mesh.vertices.push(egui::epaint::Vertex {
        pos: rect.left_top(),
        uv: white_uv,
        color: palette.bg_top_left,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: rect.right_top(),
        uv: white_uv,
        color: palette.bg_top_right,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: rect.right_bottom(),
        uv: white_uv,
        color: palette.bg_bottom_right,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: rect.left_bottom(),
        uv: white_uv,
        color: palette.bg_bottom_left,
    });
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);

    let painter = ctx.layer_painter(egui::LayerId::background());
    painter.add(egui::Shape::mesh(mesh));
}

fn apply_modern_theme(ctx: &egui::Context, dark_mode: bool) {
    let palette = palette_for(dark_mode);
    let mut visuals = if dark_mode {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    visuals.override_text_color = Some(palette.text_primary);
    visuals.hyperlink_color = palette.accent;
    visuals.panel_fill = palette.bg_top_left;
    visuals.window_fill = palette.card_bg;
    visuals.extreme_bg_color = palette.well_bg;
    visuals.faint_bg_color = palette.well_bg;

    visuals.window_corner_radius = CornerRadius::same(16);
    visuals.menu_corner_radius = CornerRadius::same(12);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(10);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(10);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(10);
    visuals.widgets.active.corner_radius = CornerRadius::same(10);
    visuals.widgets.open.corner_radius = CornerRadius::same(10);

    visuals.widgets.inactive.bg_fill = palette.pill_bg;
    visuals.widgets.inactive.weak_bg_fill = palette.pill_bg;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, palette.card_stroke);
    visuals.widgets.hovered.bg_fill = palette.pill_active_bg;
    visuals.widgets.hovered.weak_bg_fill = palette.pill_active_bg;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.2_f32, palette.accent);
    visuals.widgets.active.bg_stroke = Stroke::new(1.5_f32, palette.accent);
    visuals.selection.bg_fill = palette.accent_fill.linear_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.2_f32, palette.accent);

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(21.0));
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(14.5));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Monospace, FontId::monospace(13.0));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(12.0));

    style.spacing.item_spacing = Vec2::new(12.0, 12.0);
    style.spacing.button_padding = Vec2::new(16.0, 9.0);
    style.spacing.interact_size.y = 36.0;
    style.spacing.window_margin = Margin::same(16);
    ctx.set_style(style);
}

enum UiAction {
    Copy { text: String, label: String },
    ToggleQr(usize),
}

#[derive(Default)]
pub struct OtpGuiApp {
    state: GuiState,
    applied_dark_mode: Option<bool>,
    fonts_configured: bool,
    qr_textures: HashMap<usize, egui::TextureHandle>,
    qr_texture_generation: u64,
    camera_session: Option<CameraSession>,
    camera_preview_texture: Option<egui::TextureHandle>,
    camera_resolution: Option<(u32, u32)>,
}

impl OtpGuiApp {
    fn sync_texture_cache(&mut self) {
        if self.qr_texture_generation != self.state.result_generation {
            self.qr_textures.clear();
            self.qr_texture_generation = self.state.result_generation;
        }
    }

    fn stop_camera(&mut self) {
        if let Some(mut session) = self.camera_session.take() {
            session.stop();
        }
        self.camera_preview_texture = None;
        self.camera_resolution = None;
    }

    fn start_camera(&mut self) {
        self.stop_camera();
        self.state.status_banner = None;
        self.state.zeroize_result();
        match CameraSession::start() {
            Ok(session) => {
                self.camera_session = Some(session);
                self.state.status_banner =
                    Some("Câmera ativada. Aponte o QR Code do celular para a lente.".to_string());
            }
            Err(err) => {
                self.state.process_camera_error(&err.to_string());
            }
        }
    }

    fn poll_camera(&mut self, ctx: &egui::Context) {
        let Some(session) = &self.camera_session else {
            return;
        };

        ctx.request_repaint_after(Duration::from_millis(33));
        let updates = session.poll();
        let mut latest_preview = None;
        let mut decoded_items = None;
        let mut error_msg = None;

        for update in updates {
            match update {
                CameraUpdate::Preview(frame) => {
                    latest_preview = Some(frame);
                }
                CameraUpdate::Decoded { items, .. } => {
                    decoded_items = Some(items);
                    break;
                }
                CameraUpdate::Error(msg) => {
                    error_msg = Some(msg);
                    break;
                }
            }
        }

        if let Some(items) = decoded_items {
            self.stop_camera();
            self.state.process_camera_classifications(items);
            return;
        }

        if let Some(msg) = error_msg {
            self.stop_camera();
            self.state.process_camera_error(&msg);
            return;
        }

        if let Some(frame) = latest_preview {
            self.camera_resolution = Some((frame.width, frame.height));
            let color_img = egui::ColorImage::from_rgba_unmultiplied(
                [frame.width as usize, frame.height as usize],
                &frame.rgba,
            );
            if let Some(tex) = &mut self.camera_preview_texture {
                tex.set(color_img, egui::TextureOptions::LINEAR);
            } else {
                self.camera_preview_texture = Some(ctx.load_texture(
                    "camera_live_preview",
                    color_img,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }

        self.stop_camera();
        let extra_count = dropped.len().saturating_sub(1);
        if let Some(path) = &dropped[0].path {
            self.state.active_tab = GuiTab::QrCode;
            self.state.process_qr_path(path);
            if extra_count > 0 {
                self.state.status_banner = Some(format!(
                    "Primeiro arquivo processado; {extra_count} arquivo(s) adicional(is) ignorado(s)."
                ));
            }
        } else {
            self.state.active_tab = GuiTab::QrCode;
            self.state.zeroize_result();
            self.state.result = GuiResult::Error(
                "Arraste um arquivo local (PNG, JPG ou JPEG) ou use o botão 'Selecionar imagem QR...'."
                    .to_string(),
            );
        }
    }

    fn render_drop_overlay(ctx: &egui::Context, palette: ThemePalette) {
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if !hovering {
            return;
        }

        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("qr_drop_overlay"),
        ));
        let screen_rect = ctx.screen_rect();
        painter.rect_filled(screen_rect, 0.0, Color32::from_black_alpha(195));
        painter.rect_stroke(
            screen_rect.shrink(24.0),
            CornerRadius::same(20),
            Stroke::new(2.5_f32, palette.accent),
            egui::StrokeKind::Inside,
        );
        painter.text(
            screen_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Solte a imagem do QR Code (PNG, JPG ou JPEG)",
            egui::FontId::proportional(24.0),
            Color32::WHITE,
        );
    }

    fn primary_button(text: &str, palette: ThemePalette) -> egui::Button<'_> {
        egui::Button::new(RichText::new(text).strong().color(palette.accent_text))
            .fill(palette.accent_fill)
            .stroke(Stroke::new(1.0_f32, palette.accent.linear_multiply(0.85)))
            .corner_radius(CornerRadius::same(10))
    }

    fn danger_button(text: &str, palette: ThemePalette) -> egui::Button<'_> {
        egui::Button::new(RichText::new(text).strong().color(Color32::WHITE))
            .fill(palette.danger)
            .stroke(Stroke::new(1.0_f32, palette.danger))
            .corner_radius(CornerRadius::same(10))
    }

    fn draw_squircle_badge(
        ui: &mut egui::Ui,
        icon_text: &str,
        size: f32,
        corner_radius: u8,
        fill_color: Color32,
    ) {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(corner_radius), fill_color);
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icon_text,
            FontId::proportional((size * 0.43).clamp(11.0, 15.0)),
            Color32::WHITE,
        );
    }

    fn feature_bullet(ui: &mut egui::Ui, icon_text: &str, title: &str, palette: ThemePalette) {
        ui.horizontal(|ui| {
            Self::draw_squircle_badge(ui, icon_text, 28.0, 8, palette.secondary_badge);
            ui.add_space(4.0);
            ui.label(
                RichText::new(title)
                    .strong()
                    .size(14.5)
                    .color(palette.text_primary),
            );
        });
    }

    fn sidebar_nav_item(
        ui: &mut egui::Ui,
        active: &mut GuiTab,
        tab: GuiTab,
        icon_glyph: &str,
        label: &str,
        palette: ThemePalette,
    ) {
        let selected = *active == tab;
        let (rect, response) = ui.allocate_exact_size(Vec2::new(198.0, 42.0), egui::Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            if selected {
                painter.rect(
                    rect,
                    CornerRadius::same(12),
                    palette.pill_active_bg,
                    Stroke::new(1.0_f32, palette.pill_active_stroke),
                    egui::StrokeKind::Inside,
                );
            } else if response.hovered() {
                painter.rect_filled(rect, CornerRadius::same(12), palette.pill_bg);
            }

            let squircle_size = 26.0;
            let squircle_rect = egui::Rect::from_min_size(
                egui::pos2(rect.min.x + 10.0, rect.center().y - squircle_size / 2.0),
                Vec2::splat(squircle_size),
            );
            let squircle_fill = if selected {
                palette.accent_fill
            } else {
                palette.secondary_badge
            };
            painter.rect_filled(squircle_rect, CornerRadius::same(8), squircle_fill);
            if tab == GuiTab::DecodeMigration {
                let c = squircle_rect.center();
                let s = Stroke::new(1.8_f32, Color32::WHITE);
                let top_y = c.y - 3.5;
                let bot_y = c.y + 3.5;
                // Seta superior (esquerda -> direita)
                painter.line_segment(
                    [egui::pos2(c.x - 6.5, top_y), egui::pos2(c.x + 6.5, top_y)],
                    s,
                );
                painter.line_segment(
                    [
                        egui::pos2(c.x + 3.0, top_y - 3.0),
                        egui::pos2(c.x + 6.5, top_y),
                    ],
                    s,
                );
                painter.line_segment(
                    [
                        egui::pos2(c.x + 3.0, top_y + 3.0),
                        egui::pos2(c.x + 6.5, top_y),
                    ],
                    s,
                );
                // Seta inferior (direita -> esquerda)
                painter.line_segment(
                    [egui::pos2(c.x - 6.5, bot_y), egui::pos2(c.x + 6.5, bot_y)],
                    s,
                );
                painter.line_segment(
                    [
                        egui::pos2(c.x - 3.0, bot_y - 3.0),
                        egui::pos2(c.x - 6.5, bot_y),
                    ],
                    s,
                );
                painter.line_segment(
                    [
                        egui::pos2(c.x - 3.0, bot_y + 3.0),
                        egui::pos2(c.x - 6.5, bot_y),
                    ],
                    s,
                );
            } else {
                painter.text(
                    squircle_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    icon_glyph,
                    FontId::proportional(12.0),
                    Color32::WHITE,
                );
            }

            let label_color = if selected {
                palette.text_primary
            } else {
                palette.muted_text
            };
            painter.text(
                egui::pos2(squircle_rect.max.x + 12.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                FontId::proportional(14.5),
                label_color,
            );
        }

        if response.clicked() {
            *active = tab;
        }
    }

    fn card_frame(palette: ThemePalette) -> egui::Frame {
        egui::Frame::NONE
            .fill(palette.card_bg)
            .stroke(Stroke::new(1.0_f32, palette.card_stroke))
            .corner_radius(CornerRadius::same(18))
            .inner_margin(Margin::symmetric(24, 22))
            .shadow(palette.card_shadow)
    }

    fn well_frame(palette: ThemePalette, corner_radius: u8, margin: Margin) -> egui::Frame {
        egui::Frame::NONE
            .fill(palette.well_bg)
            .stroke(Stroke::new(1.0_f32, palette.well_stroke))
            .corner_radius(CornerRadius::same(corner_radius))
            .inner_margin(margin)
    }

    fn badge(ui: &mut egui::Ui, text: &str, bg_color: Color32) {
        egui::Frame::NONE
            .fill(bg_color)
            .corner_radius(CornerRadius::same(99))
            .inner_margin(Margin::symmetric(11, 4))
            .show(ui, |ui| {
                ui.label(RichText::new(text).small().strong().color(Color32::WHITE));
            });
    }

    fn render_glass_field(
        ui: &mut egui::Ui,
        palette: ThemePalette,
        width: f32,
        label: &str,
        value: &str,
        value_color: Option<Color32>,
        monospace: bool,
    ) {
        let inner_w = (width - 26.0).max(60.0);
        Self::well_frame(palette, 12, Margin::symmetric(12, 8)).show(ui, |ui| {
            ui.set_width(inner_w);
            ui.set_max_width(inner_w);
            ui.label(
                RichText::new(label)
                    .small()
                    .strong()
                    .color(palette.muted_text),
            );
            ui.add_space(2.0);
            let mut text = RichText::new(value);
            if monospace {
                text = text.monospace().strong();
            }
            if let Some(color) = value_color {
                text = text.color(color);
            }
            ui.add(egui::Label::new(text).truncate());
        });
    }

    fn render_account_data_section(
        ui: &mut egui::Ui,
        palette: ThemePalette,
        account: &OtpAccount,
        reveal_secrets: bool,
        id_salt: usize,
        data_w: f32,
        pending_action: &mut Option<UiAction>,
    ) {
        ui.allocate_ui_with_layout(
            Vec2::new(data_w, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(data_w);
                ui.set_max_width(data_w);
                ui.spacing_mut().item_spacing = Vec2::new(10.0, 10.0);

                let col_gap = 12.0;
                let col_w = ((data_w - col_gap) / 2.0).floor().max(110.0);

                let issuer_display = account
                    .issuer
                    .as_deref()
                    .map(sanitize_terminal_text)
                    .unwrap_or_else(|| "-".to_string());
                let secret_display = if reveal_secrets {
                    sanitize_terminal_text(&account.secret)
                } else {
                    mask_secret(&sanitize_terminal_text(&account.secret))
                };
                let algorithm_display = account
                    .algorithm
                    .map(|a| a.as_str().to_string())
                    .unwrap_or_else(|| "SHA1 (padrão)".to_string());
                let digits_str = account
                    .digits
                    .map(|d| format!("{d} dígitos"))
                    .unwrap_or_else(|| "6 dígitos (padrão)".to_string());
                let params_display = match (account.period, account.counter) {
                    (Some(period), _) => format!("{digits_str} • {period}s"),
                    (None, Some(counter)) => format!("{digits_str} • Contador: {counter}"),
                    (None, None) => digits_str,
                };

                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.x = col_gap;
                    ui.horizontal_top(|ui| {
                        ui.set_width(data_w);
                        ui.set_max_width(data_w);

                        // Subcoluna 1 (1/3 do card)
                        ui.allocate_ui_with_layout(
                            Vec2::new(col_w, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(col_w);
                                ui.set_max_width(col_w);
                                ui.spacing_mut().item_spacing = Vec2::new(10.0, 8.0);
                                Self::render_glass_field(
                                    ui,
                                    palette,
                                    col_w,
                                    "Conta / Label",
                                    &sanitize_terminal_text(&account.label),
                                    None,
                                    false,
                                );
                                Self::render_glass_field(
                                    ui,
                                    palette,
                                    col_w,
                                    "Emissor (Issuer)",
                                    &issuer_display,
                                    None,
                                    false,
                                );
                                Self::render_glass_field(
                                    ui,
                                    palette,
                                    col_w,
                                    "Secret (Base32)",
                                    &secret_display,
                                    Some(palette.warning),
                                    true,
                                );
                            },
                        );

                        // Subcoluna 2 (1/3 do card)
                        ui.allocate_ui_with_layout(
                            Vec2::new(col_w, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(col_w);
                                ui.set_max_width(col_w);
                                ui.spacing_mut().item_spacing = Vec2::new(10.0, 8.0);
                                Self::render_glass_field(
                                    ui,
                                    palette,
                                    col_w,
                                    "Tipo OTP",
                                    account.otp_type.display_upper(),
                                    Some(palette.success),
                                    false,
                                );
                                Self::render_glass_field(
                                    ui,
                                    palette,
                                    col_w,
                                    "Algoritmo",
                                    &algorithm_display,
                                    None,
                                    false,
                                );
                                Self::render_glass_field(
                                    ui,
                                    palette,
                                    col_w,
                                    "Dígitos & Parâmetros",
                                    &params_display,
                                    None,
                                    false,
                                );
                            },
                        );
                    });
                });

                ui.add_space(2.0);

                // Link completo com largura finita (nunca f32::INFINITY)
                let link_inner_w = (data_w - 26.0).max(160.0);
                Self::well_frame(palette, 12, Margin::symmetric(12, 10)).show(ui, |ui| {
                    ui.set_width(link_inner_w);
                    ui.set_max_width(link_inner_w);
                    ui.label(
                        RichText::new("Link completo (otpauth://)")
                            .small()
                            .strong()
                            .color(palette.muted_text),
                    );
                    ui.add_space(4.0);
                    let uri_display = if reveal_secrets {
                        sanitize_terminal_text(&account.uri)
                    } else {
                        sanitize_terminal_text(&mask_otpauth_uri_secret(&account.uri))
                    };
                    let mut readonly_uri = uri_display.as_str();
                    ui.add(
                        egui::TextEdit::multiline(&mut readonly_uri)
                            .id_salt(format!("readonly_uri_{id_salt}"))
                            .desired_rows(2)
                            .desired_width(link_inner_w)
                            .frame(false)
                            .font(TextStyle::Monospace),
                    );
                });

                ui.add_space(6.0);

                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_sized([140.0, 34.0], egui::Button::new("Copiar Secret"))
                        .clicked()
                    {
                        *pending_action = Some(UiAction::Copy {
                            text: account.secret.clone(),
                            label: "Secret".to_string(),
                        });
                    }
                    if ui
                        .add_sized([175.0, 34.0], egui::Button::new("Copiar Link otpauth://"))
                        .clicked()
                    {
                        *pending_action = Some(UiAction::Copy {
                            text: account.uri.clone(),
                            label: "Link otpauth://".to_string(),
                        });
                    }
                });
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn render_account_qr_section(
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
        account: &OtpAccount,
        reveal_secrets: bool,
        revealed_qr_ids: &HashSet<usize>,
        qr_textures: &mut HashMap<usize, egui::TextureHandle>,
        id_salt: usize,
        qr_w: f32,
        pending_action: &mut Option<UiAction>,
    ) {
        let qr_visible = reveal_secrets || revealed_qr_ids.contains(&id_salt);
        let qr_inner_w = (qr_w - 26.0).max(120.0);
        let max_qr_side = (qr_inner_w - 24.0).max(96.0);
        let qr_side = max_qr_side.clamp(96.0, 172.0).min(max_qr_side);

        ui.allocate_ui_with_layout(
            Vec2::new(qr_w, 0.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.set_width(qr_w);
                ui.set_max_width(qr_w);
                ui.spacing_mut().item_spacing = Vec2::new(10.0, 10.0);

                Self::well_frame(palette, 14, Margin::same(12)).show(ui, |ui| {
                    ui.set_width(qr_inner_w);
                    ui.set_max_width(qr_inner_w);

                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("QR Code OTP")
                                .small()
                                .strong()
                                .color(palette.muted_text),
                        );
                        ui.add_space(6.0);

                        if qr_visible {
                            if let std::collections::hash_map::Entry::Vacant(entry) =
                                qr_textures.entry(id_salt)
                            {
                                if let Ok(color_img) = generate_qr_color_image(&account.uri) {
                                    let handle = ctx.load_texture(
                                        format!("otp_qr_tex_{id_salt}"),
                                        color_img,
                                        egui::TextureOptions::NEAREST,
                                    );
                                    entry.insert(handle);
                                }
                            }

                            egui::Frame::NONE
                                .fill(Color32::WHITE)
                                .corner_radius(CornerRadius::same(14))
                                .inner_margin(Margin::same(8))
                                .stroke(Stroke::new(1.0_f32, palette.card_stroke))
                                .show(ui, |ui| {
                                    if let Some(tex) = qr_textures.get(&id_salt) {
                                        ui.add(
                                            egui::Image::new(tex)
                                                .fit_to_exact_size(Vec2::splat(qr_side)),
                                        );
                                    } else {
                                        ui.label(
                                            RichText::new("Erro ao gerar QR").color(palette.danger),
                                        );
                                    }
                                });

                            ui.add_space(6.0);
                            if !reveal_secrets && ui.button("Ocultar QR").clicked() {
                                *pending_action = Some(UiAction::ToggleQr(id_salt));
                            } else {
                                ui.label(
                                    RichText::new("Escaneie no seu app OTP")
                                        .small()
                                        .color(palette.muted_text),
                                );
                            }
                        } else {
                            qr_textures.remove(&id_salt);
                            let protected_inner_w = (qr_inner_w - 30.0).max(96.0);
                            let protected_inner_h = (qr_side + 16.0).max(140.0);
                            egui::Frame::NONE
                                .fill(palette.pill_bg)
                                .corner_radius(CornerRadius::same(14))
                                .stroke(Stroke::new(1.0_f32, palette.well_stroke))
                                .inner_margin(Margin::same(14))
                                .show(ui, |ui| {
                                    ui.set_min_size(Vec2::new(
                                        protected_inner_w,
                                        protected_inner_h,
                                    ));
                                    ui.set_max_width(protected_inner_w);
                                    ui.vertical_centered(|ui| {
                                        ui.add_space(22.0);
                                        ui.label(
                                            RichText::new("QR Code Protegido")
                                                .strong()
                                                .color(palette.text_primary),
                                        );
                                        ui.add_space(2.0);
                                        ui.label(
                                            RichText::new("Oculto por privacidade")
                                                .small()
                                                .color(palette.muted_text),
                                        );
                                        ui.add_space(10.0);
                                        if ui
                                            .add(Self::primary_button("Mostrar QR", palette))
                                            .clicked()
                                        {
                                            *pending_action = Some(UiAction::ToggleQr(id_salt));
                                        }
                                    });
                                });
                        }
                    });
                });
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn render_account_card(
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
        account: &OtpAccount,
        reveal_secrets: bool,
        revealed_qr_ids: &HashSet<usize>,
        qr_textures: &mut HashMap<usize, egui::TextureHandle>,
        index: Option<(usize, usize)>,
        id_salt: usize,
        content_width: f32,
        pending_action: &mut Option<UiAction>,
    ) {
        let card_inner_w = (content_width - CARD_FRAME_CHROME_W).max(300.0);

        Self::card_frame(palette).show(ui, |ui| {
            ui.set_width(card_inner_w);
            ui.set_max_width(card_inner_w);

            // Cabeçalho com label truncada e badge de tipo sempre visível à direita
            ui.horizontal(|ui| {
                ui.set_width(card_inner_w);
                let type_badge_reserve = 72.0;
                let left_header_w = (card_inner_w - type_badge_reserve).max(140.0);

                ui.allocate_ui_with_layout(
                    Vec2::new(left_header_w, 26.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_max_width(left_header_w);
                        if let Some((idx, total)) = index {
                            Self::badge(ui, &format!("{idx}/{total}"), palette.secondary_badge);
                        }
                        ui.add(
                            egui::Label::new(
                                RichText::new(sanitize_terminal_text(&account.label))
                                    .strong()
                                    .size(16.5),
                            )
                            .truncate(),
                        );
                    },
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    Self::badge(ui, account.otp_type.display_upper(), palette.success);
                });
            });

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(8.0);

            // Layout responsivo em 3 partes (2/3 Dados + 1/3 QR Code)
            if card_inner_w >= 560.0 {
                let gap = 16.0;
                let total_cols_w = card_inner_w - gap;
                let data_w = (total_cols_w * (2.0 / 3.0)).floor();
                let qr_w = (total_cols_w - data_w).floor();

                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    ui.horizontal_top(|ui| {
                        ui.set_width(card_inner_w);
                        ui.set_max_width(card_inner_w);

                        Self::render_account_data_section(
                            ui,
                            palette,
                            account,
                            reveal_secrets,
                            id_salt,
                            data_w,
                            pending_action,
                        );

                        Self::render_account_qr_section(
                            ui,
                            ctx,
                            palette,
                            account,
                            reveal_secrets,
                            revealed_qr_ids,
                            qr_textures,
                            id_salt,
                            qr_w,
                            pending_action,
                        );
                    });
                });
            } else {
                Self::render_account_data_section(
                    ui,
                    palette,
                    account,
                    reveal_secrets,
                    id_salt,
                    card_inner_w,
                    pending_action,
                );
                ui.add_space(12.0);
                Self::render_account_qr_section(
                    ui,
                    ctx,
                    palette,
                    account,
                    reveal_secrets,
                    revealed_qr_ids,
                    qr_textures,
                    id_salt,
                    card_inner_w,
                    pending_action,
                );
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn render_migration_list(
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
        accounts: &[OtpAccount],
        reveal_secrets: bool,
        revealed_qr_ids: &HashSet<usize>,
        qr_textures: &mut HashMap<usize, egui::TextureHandle>,
        salt_offset: usize,
        content_width: f32,
        pending_action: &mut Option<UiAction>,
    ) {
        ui.allocate_ui_with_layout(
            Vec2::new(content_width, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(content_width);
                ui.horizontal(|ui| {
                    ui.set_width(content_width);
                    Self::badge(
                        ui,
                        &format!("{} conta(s) decodificada(s)", accounts.len()),
                        palette.success,
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Copiar todos os links").clicked() {
                            let all_links: Vec<&str> =
                                accounts.iter().map(|a| a.uri.as_str()).collect();
                            *pending_action = Some(UiAction::Copy {
                                text: all_links.join("\n"),
                                label: "Todos os links".to_string(),
                            });
                        }
                    });
                });
            },
        );

        ui.add_space(12.0);
        let total = accounts.len();
        for (idx, account) in accounts.iter().enumerate() {
            Self::render_account_card(
                ui,
                ctx,
                palette,
                account,
                reveal_secrets,
                revealed_qr_ids,
                qr_textures,
                Some((idx + 1, total)),
                salt_offset + idx,
                content_width,
                pending_action,
            );
            ui.add_space(14.0);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_result_area(
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
        result: &GuiResult,
        reveal_secrets: bool,
        revealed_qr_ids: &HashSet<usize>,
        qr_textures: &mut HashMap<usize, egui::TextureHandle>,
        content_width: f32,
        pending_action: &mut Option<UiAction>,
    ) {
        let card_inner_w = (content_width - CARD_FRAME_CHROME_W).max(260.0);

        match result {
            GuiResult::Empty => {}
            GuiResult::Error(msg) => {
                egui::Frame::NONE
                    .fill(palette.card_bg)
                    .corner_radius(CornerRadius::same(18))
                    .stroke(Stroke::new(1.5_f32, palette.danger))
                    .inner_margin(Margin::symmetric(24, 22))
                    .shadow(palette.card_shadow)
                    .show(ui, |ui| {
                        ui.set_width(card_inner_w);
                        ui.set_max_width(card_inner_w);
                        Self::badge(ui, "Erro", palette.danger);
                        ui.add_space(8.0);
                        ui.label(msg);
                    });
            }
            GuiResult::OtpAuthAccount(account) => {
                Self::badge(
                    ui,
                    &format!("Link otpauth://{} válido", account.otp_type.as_str()),
                    palette.success,
                );
                ui.add_space(10.0);
                Self::render_account_card(
                    ui,
                    ctx,
                    palette,
                    account,
                    reveal_secrets,
                    revealed_qr_ids,
                    qr_textures,
                    None,
                    10_000,
                    content_width,
                    pending_action,
                );
            }
            GuiResult::MigrationAccounts(accounts) => {
                Self::render_migration_list(
                    ui,
                    ctx,
                    palette,
                    accounts,
                    reveal_secrets,
                    revealed_qr_ids,
                    qr_textures,
                    20_000,
                    content_width,
                    pending_action,
                );
            }
            GuiResult::QrResults {
                source_label,
                items,
            } => {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("Origem: {source_label}")).strong());
                    Self::badge(
                        ui,
                        &format!("{} QR Code(s) detectado(s)", items.len()),
                        palette.secondary_badge,
                    );
                });
                ui.add_space(10.0);

                for (qr_idx, item) in items.iter().enumerate() {
                    if items.len() > 1 {
                        ui.heading(format!("QR Code {} de {}", qr_idx + 1, items.len()));
                    }

                    match item {
                        QrClassification::OtpAuth(account) => {
                            Self::badge(
                                ui,
                                &format!("QR Code válido: otpauth://{}", account.otp_type.as_str()),
                                palette.success,
                            );
                            ui.add_space(10.0);
                            Self::render_account_card(
                                ui,
                                ctx,
                                palette,
                                account,
                                reveal_secrets,
                                revealed_qr_ids,
                                qr_textures,
                                None,
                                30_000 + qr_idx * 100,
                                content_width,
                                pending_action,
                            );
                        }
                        QrClassification::Migration { accounts, .. } => {
                            Self::badge(
                                ui,
                                "QR Code válido: otpauth-migration://",
                                palette.secondary_badge,
                            );
                            ui.add_space(10.0);
                            Self::render_migration_list(
                                ui,
                                ctx,
                                palette,
                                accounts,
                                reveal_secrets,
                                revealed_qr_ids,
                                qr_textures,
                                40_000 + qr_idx * 1000,
                                content_width,
                                pending_action,
                            );
                        }
                        QrClassification::Invalid {
                            raw_content,
                            reason,
                        } => {
                            egui::Frame::NONE
                                .fill(palette.card_bg)
                                .corner_radius(CornerRadius::same(18))
                                .stroke(Stroke::new(1.5_f32, palette.danger))
                                .inner_margin(Margin::symmetric(24, 22))
                                .shadow(palette.card_shadow)
                                .show(ui, |ui| {
                                    ui.set_width(card_inner_w);
                                    ui.set_max_width(card_inner_w);
                                    Self::badge(ui, "QR Code inválido", palette.danger);
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new(sanitize_terminal_text(reason))
                                            .color(palette.warning),
                                    );
                                    ui.add_space(10.0);
                                    let safe_raw = sanitize_terminal_text(raw_content);
                                    let raw_inner_w = (card_inner_w - 26.0).max(160.0);
                                    Self::well_frame(palette, 12, Margin::symmetric(12, 10)).show(
                                        ui,
                                        |ui| {
                                            ui.set_width(raw_inner_w);
                                            ui.set_max_width(raw_inner_w);
                                            ui.label(
                                                RichText::new("Conteúdo encontrado no QR Code:")
                                                    .small()
                                                    .strong()
                                                    .color(palette.muted_text),
                                            );
                                            ui.add_space(4.0);
                                            let mut readonly_raw = safe_raw.as_str();
                                            ui.add(
                                                egui::TextEdit::multiline(&mut readonly_raw)
                                                    .id_salt(format!("invalid_qr_raw_{qr_idx}"))
                                                    .desired_rows(3)
                                                    .desired_width(raw_inner_w)
                                                    .frame(false)
                                                    .font(TextStyle::Monospace),
                                            );
                                        },
                                    );
                                    ui.add_space(10.0);
                                    if ui.button("Copiar conteúdo").clicked() {
                                        *pending_action = Some(UiAction::Copy {
                                            text: safe_raw,
                                            label: "Conteúdo do QR Code".to_string(),
                                        });
                                    }
                                });
                        }
                    }
                    ui.add_space(14.0);
                }
            }
        }
    }

    fn render_about_tab(
        ui: &mut egui::Ui,
        palette: ThemePalette,
        content_width: f32,
        pending_action: &mut Option<UiAction>,
    ) {
        ui.label(
            RichText::new("Sobre o OTP Tools")
                .strong()
                .size(26.0)
                .color(palette.text_primary),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Ferramenta desktop e CLI 100% offline para migração e inspeção segura de tokens OTP.",
            )
            .color(palette.muted_text),
        );
        ui.add_space(16.0);

        let card_inner_w = (content_width - CARD_FRAME_CHROME_W).max(300.0);
        Self::card_frame(palette).show(ui, |ui| {
            ui.set_width(card_inner_w);
            ui.set_max_width(card_inner_w);

            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(APP_DISPLAY_NAME).strong().size(22.0));
                Self::badge(
                    ui,
                    &format!("v{}", env!("CARGO_PKG_VERSION")),
                    palette.secondary_badge,
                );
                Self::badge(ui, APP_LICENSE, palette.success);
            });

            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "Leia QR Codes em PNG, JPG e JPEG, decodifique payloads de migração do Google Authenticator, analise links otpauth:// e gere QR Codes individuais para importar no seu aplicativo OTP preferido.",
                )
                .color(palette.muted_text),
            );

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(12.0);

            Self::feature_bullet(
                ui,
                "★",
                &format!("Criado por {APP_AUTHOR}"),
                palette,
            );
            ui.add_space(10.0);

            egui::Grid::new("about_info_grid")
                .num_columns(2)
                .spacing([20.0, 10.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("E-mail").strong().color(palette.muted_text));
                    ui.horizontal_wrapped(|ui| {
                        ui.hyperlink_to(APP_EMAIL, format!("mailto:{APP_EMAIL}"));
                        if ui.small_button("Copiar").clicked() {
                            *pending_action = Some(UiAction::Copy {
                                text: APP_EMAIL.to_string(),
                                label: "E-mail".to_string(),
                            });
                        }
                    });
                    ui.end_row();

                    ui.label(RichText::new("GitHub").strong().color(palette.muted_text));
                    ui.horizontal_wrapped(|ui| {
                        ui.hyperlink_to(APP_REPO_URL, APP_REPO_URL);
                        if ui.small_button("Copiar").clicked() {
                            *pending_action = Some(UiAction::Copy {
                                text: APP_REPO_URL.to_string(),
                                label: "Link do GitHub".to_string(),
                            });
                        }
                    });
                    ui.end_row();

                    ui.label(RichText::new("Licença").strong().color(palette.muted_text));
                    ui.label(format!("{APP_LICENSE} — Copyright (c) 2026 {APP_AUTHOR}"));
                    ui.end_row();
                });

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(10.0);
            ui.label(
                RichText::new(
                    "Privacidade: nenhum segredo OTP é enviado pela rede ou gravado em disco. Todos os dados são processados localmente em memória e zerados ao limpar a tela ou encerrar o aplicativo.",
                )
                .small()
                .color(palette.muted_text),
            );
        });
    }

    fn render_camera_viewfinder(
        &mut self,
        ui: &mut egui::Ui,
        palette: ThemePalette,
        card_inner_w: f32,
    ) {
        if self.camera_session.is_none() {
            return;
        }

        let mut stop_clicked = false;

        Self::card_frame(palette).show(ui, |ui| {
            ui.set_width(card_inner_w);
            ui.set_max_width(card_inner_w);

            ui.horizontal(|ui| {
                ui.set_width(card_inner_w);
                Self::badge(ui, "CÂMERA AO VIVO", palette.accent_fill);
                if let Some((w, h)) = self.camera_resolution {
                    Self::badge(ui, &format!("{w}x{h}"), palette.secondary_badge);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(Self::danger_button("Parar Câmera", palette))
                        .clicked()
                    {
                        stop_clicked = true;
                    }
                });
            });

            ui.add_space(10.0);
            ui.label(
                RichText::new(
                    "Aponte o QR Code da tela do celular em frente à câmera do computador.",
                )
                .color(palette.muted_text),
            );
            ui.add_space(12.0);

            let frame_inner_w = (card_inner_w - 24.0).max(200.0);
            Self::well_frame(palette, 14, Margin::same(12)).show(ui, |ui| {
                ui.set_width(frame_inner_w);
                ui.set_max_width(frame_inner_w);
                ui.vertical_centered(|ui| {
                    let max_w = (frame_inner_w - 16.0).clamp(180.0, 480.0);
                    let max_h = 300.0_f32;

                    if let Some(tex) = &self.camera_preview_texture {
                        let [tex_w, tex_h] = tex.size();
                        let (w_f, h_f) = (tex_w.max(1) as f32, tex_h.max(1) as f32);
                        let scale = (max_w / w_f).min(max_h / h_f);
                        let display_size = Vec2::new((w_f * scale).round(), (h_f * scale).round());
                        let response = ui.add(
                            egui::Image::new(tex)
                                .fit_to_exact_size(display_size)
                                .corner_radius(CornerRadius::same(10)),
                        );

                        // Cantoneiras visuais do retículo de mira para orientar o posicionamento do celular
                        let target_rect = response.rect.shrink(display_size.min_elem() * 0.16);
                        let corner_len = (target_rect.width().min(target_rect.height()) * 0.20)
                            .clamp(14.0, 28.0);
                        let stroke = Stroke::new(2.5_f32, palette.accent);
                        let p = ui.painter();
                        // Superior esquerda
                        p.line_segment(
                            [
                                target_rect.left_top(),
                                target_rect.left_top() + Vec2::new(corner_len, 0.0),
                            ],
                            stroke,
                        );
                        p.line_segment(
                            [
                                target_rect.left_top(),
                                target_rect.left_top() + Vec2::new(0.0, corner_len),
                            ],
                            stroke,
                        );
                        // Superior direita
                        p.line_segment(
                            [
                                target_rect.right_top(),
                                target_rect.right_top() - Vec2::new(corner_len, 0.0),
                            ],
                            stroke,
                        );
                        p.line_segment(
                            [
                                target_rect.right_top(),
                                target_rect.right_top() + Vec2::new(0.0, corner_len),
                            ],
                            stroke,
                        );
                        // Inferior esquerda
                        p.line_segment(
                            [
                                target_rect.left_bottom(),
                                target_rect.left_bottom() + Vec2::new(corner_len, 0.0),
                            ],
                            stroke,
                        );
                        p.line_segment(
                            [
                                target_rect.left_bottom(),
                                target_rect.left_bottom() - Vec2::new(0.0, corner_len),
                            ],
                            stroke,
                        );
                        // Inferior direita
                        p.line_segment(
                            [
                                target_rect.right_bottom(),
                                target_rect.right_bottom() - Vec2::new(corner_len, 0.0),
                            ],
                            stroke,
                        );
                        p.line_segment(
                            [
                                target_rect.right_bottom(),
                                target_rect.right_bottom() - Vec2::new(0.0, corner_len),
                            ],
                            stroke,
                        );
                    } else {
                        let placeholder_w = max_w.min(400.0);
                        let (rect, _) = ui.allocate_exact_size(
                            Vec2::new(placeholder_w, 200.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(10),
                            Color32::from_black_alpha(60),
                        );
                        ui.painter().text(
                            rect.center() - Vec2::new(0.0, 12.0),
                            egui::Align2::CENTER_CENTER,
                            "Inicializando câmera...",
                            FontId::proportional(16.0),
                            palette.text_primary,
                        );
                        ui.painter().text(
                            rect.center() + Vec2::new(0.0, 12.0),
                            egui::Align2::CENTER_CENTER,
                            "Autorize o acesso à webcam caso o sistema solicite.",
                            FontId::proportional(12.5),
                            palette.muted_text,
                        );
                    }
                });
            });
        });

        if stop_clicked {
            self.stop_camera();
            self.state.status_banner = Some("Câmera encerrada.".to_string());
        }
    }
}

impl eframe::App for OtpGuiApp {
    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop_camera();
        self.state.clear_all();
        self.qr_textures.clear();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.fonts_configured {
            configure_crisp_fonts(ctx);
            self.fonts_configured = true;
        }

        if self.applied_dark_mode != Some(self.state.dark_mode) {
            apply_modern_theme(ctx, self.state.dark_mode);
            self.applied_dark_mode = Some(self.state.dark_mode);
        }

        let palette = palette_for(self.state.dark_mode);
        paint_background_gradient(ctx, palette);
        self.handle_dropped_files(ctx);
        if self.state.active_tab != GuiTab::QrCode && self.camera_session.is_some() {
            self.stop_camera();
        }
        self.poll_camera(ctx);
        self.sync_texture_cache();

        // Sidebar lateral esquerda inspirada no CleanMyMac X
        egui::SidePanel::left("sidebar_nav")
            .exact_width(230.0)
            .resizable(false)
            .frame(
                egui::Frame::NONE
                    .fill(palette.sidebar_bg)
                    .inner_margin(Margin::symmetric(16, 20))
                    .stroke(Stroke::new(1.0_f32, palette.well_stroke)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    Self::draw_squircle_badge(ui, "OTP", 32.0, 9, palette.accent_fill);
                    ui.add_space(4.0);
                    ui.label(RichText::new(APP_DISPLAY_NAME).strong().size(18.5));
                });
                ui.add_space(6.0);
                //Self::badge(ui, "Offline", palette.success);
                ui.add_space(18.0);

                Self::sidebar_nav_item(
                    ui,
                    &mut self.state.active_tab,
                    GuiTab::QrCode,
                    "QR",
                    "Ler QR Code",
                    palette,
                );
                ui.add_space(4.0);
                Self::sidebar_nav_item(
                    ui,
                    &mut self.state.active_tab,
                    GuiTab::DecodeMigration,
                    "⇄",
                    "Migration",
                    palette,
                );
                ui.add_space(4.0);
                Self::sidebar_nav_item(
                    ui,
                    &mut self.state.active_tab,
                    GuiTab::ParseOtpAuth,
                    "🔑",
                    "Analisar OTP",
                    palette,
                );
                ui.add_space(4.0);
                Self::sidebar_nav_item(
                    ui,
                    &mut self.state.active_tab,
                    GuiTab::About,
                    "i",
                    "Sobre",
                    palette,
                );

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.label(
                        RichText::new(format!("v{} • {APP_LICENSE}", env!("CARGO_PKG_VERSION")))
                            .small()
                            .color(palette.muted_text),
                    );
                    ui.add_space(6.0);

                    if ui
                        .add_sized([198.0, 34.0], egui::Button::new("Limpar tela"))
                        .clicked()
                    {
                        self.stop_camera();
                        self.state.clear_all();
                        self.qr_textures.clear();
                        clear_input_undo_history(ctx);
                    }

                    let theme_label = if self.state.dark_mode {
                        "Tema Claro"
                    } else {
                        "Tema Escuro"
                    };
                    if ui
                        .add_sized([198.0, 34.0], egui::Button::new(theme_label))
                        .clicked()
                    {
                        self.state.dark_mode = !self.state.dark_mode;
                    }

                    let reveal_label = if self.state.reveal_secrets {
                        "Ocultar Secrets"
                    } else {
                        "Mostrar Secrets"
                    };
                    if ui
                        .add_sized([198.0, 34.0], egui::Button::new(reveal_label))
                        .clicked()
                    {
                        self.state.reveal_secrets = !self.state.reveal_secrets;
                        if !self.state.reveal_secrets {
                            self.qr_textures.clear();
                        }
                    }

                    ui.add_space(6.0);
                    ui.separator();
                });
            });

        egui::TopBottomPanel::bottom("status_footer")
            .frame(
                egui::Frame::NONE
                    .fill(palette.sidebar_bg)
                    .inner_margin(Margin::symmetric(24, 9))
                    .stroke(Stroke::new(1.0_f32, palette.well_stroke)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(status) = &self.state.status_banner {
                        ui.label(RichText::new(status).strong().color(palette.accent));
                    } else {
                        ui.label(
                            RichText::new(format!(
                                "{APP_DISPLAY_NAME} • Processamento 100% offline em memória"
                            ))
                            .small()
                            .color(palette.muted_text),
                        );
                    }
                });
            });

        let mut pending_action: Option<UiAction> = None;

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(Color32::TRANSPARENT)
                    .inner_margin(Margin::symmetric(28, 22)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let content_width = (ui.available_width() - 10.0).max(340.0);
                        let card_inner_w = (content_width - CARD_FRAME_CHROME_W).max(260.0);

                        if self.state.active_tab == GuiTab::About {
                            Self::render_about_tab(ui, palette, content_width, &mut pending_action);
                            return;
                        }

                        match self.state.active_tab {
                            GuiTab::QrCode => {
                                ui.label(
                                    RichText::new("Leitura Inteligente de QR Code")
                                        .strong()
                                        .size(26.0)
                                        .color(palette.text_primary),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(
                                        "Extraia contas TOTP, HOTP ou migrações completas do Google Authenticator em segundos.",
                                    )
                                    .color(palette.muted_text),
                                );
                                ui.add_space(12.0);
                                Self::feature_bullet(
                                    ui,
                                    "1",
                                    "Leitura instantânea de PNG, JPG, JPEG ou câmera do PC",
                                    palette,
                                );
                                ui.add_space(6.0);
                                Self::feature_bullet(
                                    ui,
                                    "2",
                                    "Decodificação automática de otpauth:// e migration://",
                                    palette,
                                );
                                ui.add_space(6.0);
                                Self::feature_bullet(
                                    ui,
                                    "3",
                                    "Geração de QR Code individual para seu app OTP",
                                    palette,
                                );
                                ui.add_space(16.0);

                                Self::card_frame(palette).show(ui, |ui| {
                                    ui.set_width(card_inner_w);
                                    ui.set_max_width(card_inner_w);
                                    ui.horizontal_wrapped(|ui| {
                                        if ui
                                            .add_sized(
                                                [210.0, 38.0],
                                                Self::primary_button(
                                                    "Selecionar imagem QR...",
                                                    palette,
                                                ),
                                            )
                                            .clicked()
                                        {
                                            if let Some(path) = rfd::FileDialog::new()
                                                .add_filter(
                                                    "Imagens QR Code",
                                                    &["png", "jpg", "jpeg", "PNG", "JPG", "JPEG"],
                                                )
                                                .pick_file()
                                            {
                                                self.stop_camera();
                                                self.state.process_qr_path(&path);
                                            }
                                        }

                                        if self.camera_session.is_some() {
                                            if ui
                                                .add_sized(
                                                    [170.0, 38.0],
                                                    Self::danger_button("Parar Câmera", palette),
                                                )
                                                .clicked()
                                            {
                                                self.stop_camera();
                                                self.state.status_banner =
                                                    Some("Câmera encerrada.".to_string());
                                            }
                                        } else if ui
                                            .add_sized(
                                                [170.0, 38.0],
                                                Self::primary_button("Usar Câmera", palette),
                                            )
                                            .clicked()
                                        {
                                            self.start_camera();
                                        }
                                    });

                                    ui.add_space(10.0);
                                    ui.label(
                                        RichText::new(
                                            "Ou arraste e solte uma imagem PNG, JPG ou JPEG nesta janela, ou aponte o QR Code do celular para a webcam.",
                                        )
                                        .color(palette.muted_text),
                                    );
                                });

                                if self.camera_session.is_some() {
                                    ui.add_space(14.0);
                                    self.render_camera_viewfinder(ui, palette, card_inner_w);
                                }
                            }
                            GuiTab::DecodeMigration => {
                                ui.label(
                                    RichText::new("Decodificar Google Authenticator")
                                        .strong()
                                        .size(24.0)
                                        .color(palette.text_primary),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(
                                        "Converta links otpauth-migration://offline?data=... em contas individuais com QR Code.",
                                    )
                                    .color(palette.muted_text),
                                );
                                ui.add_space(14.0);

                                Self::card_frame(palette).show(ui, |ui| {
                                    ui.set_width(card_inner_w);
                                    ui.set_max_width(card_inner_w);
                                    ui.label(
                                        RichText::new("Link otpauth-migration://offline?data=...")
                                            .strong(),
                                    );
                                    ui.add_space(8.0);
                                    let input_inner_w = (card_inner_w - 34.0).max(200.0);
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.state.migration_input)
                                            .id(egui::Id::new(MIGRATION_INPUT_ID))
                                            .password(!self.state.reveal_secrets)
                                            .margin(Margin::symmetric(16, 12))
                                            .vertical_align(egui::Align::Center)
                                            .horizontal_align(egui::Align::Center)
                                            .desired_width(input_inner_w)
                                            .hint_text(
                                                "Cole aqui o link otpauth-migration://offline?data=...",
                                            ),
                                    );
                                    ui.add_space(12.0);
                                    ui.horizontal(|ui| {
                                        if ui
                                            .add_sized(
                                                [150.0, 38.0],
                                                Self::primary_button("Decodificar", palette),
                                            )
                                            .clicked()
                                        {
                                            self.state.process_migration();
                                        }
                                        if ui.add_sized([110.0, 38.0], egui::Button::new("Limpar")).clicked() {
                                            self.state.clear_migration_tab();
                                            self.qr_textures.clear();
                                            clear_input_undo_history(ctx);
                                        }
                                    });
                                });
                            }
                            GuiTab::ParseOtpAuth => {
                                ui.label(
                                    RichText::new("Analisar Link OTPAuth")
                                        .strong()
                                        .size(24.0)
                                        .color(palette.text_primary),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(
                                        "Inspecione links otpauth://totp/... ou otpauth://hotp/... com campos separados e QR Code.",
                                    )
                                    .color(palette.muted_text),
                                );
                                ui.add_space(14.0);

                                Self::card_frame(palette).show(ui, |ui| {
                                    ui.set_width(card_inner_w);
                                    ui.set_max_width(card_inner_w);
                                    ui.label(
                                        RichText::new(
                                            "Link otpauth://totp/... ou otpauth://hotp/...",
                                        )
                                        .strong(),
                                    );
                                    ui.add_space(8.0);
                                    let input_inner_w = (card_inner_w - 34.0).max(200.0);
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.state.otpauth_input)
                                            .id(egui::Id::new(OTPAUTH_INPUT_ID))
                                            .password(!self.state.reveal_secrets)
                                            .margin(Margin::symmetric(16, 12))
                                            .vertical_align(egui::Align::Center)
                                            .horizontal_align(egui::Align::Center)
                                            .desired_width(input_inner_w)
                                            .hint_text(
                                                "Cole aqui o link otpauth://totp/... ou otpauth://hotp/...",
                                            ),
                                    );
                                    ui.add_space(12.0);
                                    ui.horizontal(|ui| {
                                        if ui
                                            .add_sized(
                                                [150.0, 38.0],
                                                Self::primary_button("Analisar", palette),
                                            )
                                            .clicked()
                                        {
                                            self.state.process_otpauth();
                                        }
                                        if ui.add_sized([110.0, 38.0], egui::Button::new("Limpar")).clicked() {
                                            self.state.clear_otpauth_tab();
                                            self.qr_textures.clear();
                                            clear_input_undo_history(ctx);
                                        }
                                    });
                                });
                            }
                            GuiTab::About => {}
                        }

                        self.sync_texture_cache();
                        ui.add_space(16.0);

                        Self::render_result_area(
                            ui,
                            ctx,
                            palette,
                            &self.state.result,
                            self.state.reveal_secrets,
                            &self.state.revealed_qr_ids,
                            &mut self.qr_textures,
                            content_width,
                            &mut pending_action,
                        );
                    });
            });

        if let Some(action) = pending_action {
            match action {
                UiAction::Copy { text, label } => {
                    ctx.copy_text(text);
                    self.state.status_banner =
                        Some(format!("{label} copiado para a área de transferência."));
                }
                UiAction::ToggleQr(id_salt) => {
                    self.state.toggle_qr_visibility(id_salt);
                }
            }
        }

        Self::render_drop_overlay(ctx, palette);
    }
}

/// Inicia a janela desktop do **OTP Tools** (`otp-tools-gui`).
pub fn launch_gui() -> Result<(), String> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(format!(
            "{APP_DISPLAY_NAME} — Leitor de QR Code & Decoder OTP"
        ))
        .with_inner_size([1060.0, 740.0])
        .with_min_inner_size([780.0, 540.0])
        .with_drag_and_drop(true);

    if let Some(icon) = load_app_icon() {
        viewport = viewport.with_icon(Arc::new(icon));
    }

    let options = eframe::NativeOptions {
        viewport,
        persist_window: false,
        ..Default::default()
    };

    eframe::run_native(
        APP_DISPLAY_NAME,
        options,
        Box::new(|cc| {
            configure_crisp_fonts(&cc.egui_ctx);
            Ok(Box::new(OtpGuiApp {
                fonts_configured: true,
                ..Default::default()
            }))
        }),
    )
    .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EXAMPLE_MIGRATION;
    use crate::model::OtpType;

    #[test]
    fn configure_crisp_fonts_loads_cleanly() {
        let ctx = egui::Context::default();
        configure_crisp_fonts(&ctx);
        assert!(ctx.tessellation_options(|o| o.feathering));
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("Teste de nitidez HelveticaNeue QR ⇄ 🔑");
                ui.label(RichText::new("JBSWY3DPEHPK3PXP").monospace());
            });
        });
    }

    #[test]
    fn mask_secret_handles_short_and_long_secrets() {
        assert_eq!(mask_secret("A"), "••••");
        assert_eq!(mask_secret("ABCD"), "••••");
        assert_eq!(mask_secret("ABCDE"), "•••••");
        assert_eq!(mask_secret("ABCDEFGH"), "••••••••");
        assert_eq!(mask_secret("ABCDEFGHIJKL"), "ABCD••••••••");
        assert_eq!(mask_secret("JBSWY3DPEHPK3PXP"), "JBSW••••••••••••");
    }

    #[test]
    fn mask_otpauth_uri_secret_hides_secret_param_and_preserves_rest() {
        let totp = "otpauth://totp/Example%3Aalice%40google.com?issuer=Example&secret=JBSWY3DPEHPK3PXP&digits=6";
        let masked_totp = mask_otpauth_uri_secret(totp);
        assert!(!masked_totp.contains("JBSWY3DPEHPK3PXP"));
        assert!(!masked_totp.contains("%E2%80%A2"));
        assert!(masked_totp.contains("issuer=Example"));
        assert!(masked_totp.contains("digits=6"));
        assert!(masked_totp.contains("secret=JBSW"));

        let hotp = "otpauth://hotp/Acme:bob?SECRET=ABCDEFGH&counter=5";
        let masked_hotp = mask_otpauth_uri_secret(hotp);
        assert!(!masked_hotp.contains("ABCDEFGH"));
        assert!(masked_hotp.contains("counter=5"));
    }

    #[test]
    fn generate_qr_color_image_produces_square_bw_image() {
        let uri =
            "otpauth://totp/Example%3Aalice%40google.com?issuer=Example&secret=JBSWY3DPEHPK3PXP";
        let img = generate_qr_color_image(uri).expect("gerar QR ColorImage");
        assert_eq!(img.size[0], img.size[1]);
        assert!(img.size[0] >= 100);
        assert!(img.pixels.contains(&Color32::BLACK));
        assert!(img.pixels.contains(&Color32::WHITE));
    }

    #[test]
    fn load_app_icon_decodes_embedded_ico() {
        let icon = load_app_icon().expect("carregar AppIcon.ico embutido");
        assert!(icon.width > 0 && icon.height > 0);
        assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
    }

    #[test]
    fn gui_state_decodes_migration_and_clears_sensitive_state() {
        let mut state = GuiState::default();
        state.migration_input = EXAMPLE_MIGRATION.to_string();
        state.reveal_secrets = true;

        state.process_migration();
        match &state.result {
            GuiResult::MigrationAccounts(accounts) => {
                assert_eq!(accounts.len(), 1);
                assert_eq!(accounts[0].secret, "JBSWY3DPEHPK3PXP");
            }
            other => panic!("esperava MigrationAccounts, obtive {other:?}"),
        }

        state.toggle_qr_visibility(20_000);
        assert!(state.revealed_qr_ids.contains(&20_000));

        state.clear_all();
        assert!(state.migration_input.is_empty());
        assert_eq!(state.result, GuiResult::Empty);
        assert!(!state.reveal_secrets);
        assert!(state.revealed_qr_ids.is_empty());
    }

    #[test]
    fn gui_state_rejects_empty_and_whitespace_inputs() {
        let mut state = GuiState::default();
        state.migration_input = "   \n\t ".to_string();
        state.otpauth_input = "   ".to_string();

        state.process_migration();
        assert!(matches!(state.result, GuiResult::Error(_)));

        state.process_otpauth();
        assert!(matches!(state.result, GuiResult::Error(_)));
    }

    #[test]
    fn gui_state_parses_otpauth_and_handles_invalid_input() {
        let mut state = GuiState::default();
        state.otpauth_input =
            "otpauth://hotp/Acme:bob?secret=JBSWY3DPEHPK3PXP&counter=3".to_string();

        state.process_otpauth();
        match &state.result {
            GuiResult::OtpAuthAccount(account) => {
                assert_eq!(account.otp_type, OtpType::Hotp);
                assert_eq!(account.counter, Some(3));
            }
            other => panic!("esperava OtpAuthAccount, obtive {other:?}"),
        }

        state.otpauth_input = "otpauth://totp/Test?secret=ABC&digits=%1B[31mFAKE".to_string();
        state.process_otpauth();
        match &state.result {
            GuiResult::Error(msg) => assert!(!msg.contains('\x1b')),
            other => panic!("esperava Error sanitizado, obtive {other:?}"),
        }
    }

    #[test]
    fn gui_state_handles_missing_qr_file_with_error() {
        let mut state = GuiState::default();
        state.process_qr_path(Path::new("/caminho/inexistente/qr.png"));
        assert!(matches!(state.result, GuiResult::Error(_)));
    }

    #[test]
    fn gui_state_processes_camera_classifications_and_errors() {
        let mut state = GuiState::default();
        let account = parse_otpauth_uri(
            "otpauth://totp/Cam:alice@example.com?secret=JBSWY3DPEHPK3PXP&issuer=Cam",
        )
        .expect("parse uri");
        state.process_camera_classifications(vec![QrClassification::OtpAuth(account)]);
        assert_eq!(
            state.status_banner.as_deref(),
            Some("QR Code lido com sucesso pela câmera!")
        );
        match &state.result {
            GuiResult::QrResults {
                source_label,
                items,
            } => {
                assert_eq!(source_label, "Câmera (Ao vivo)");
                assert_eq!(items.len(), 1);
            }
            other => panic!("esperava QrResults da câmera, obtive {other:?}"),
        }

        state.process_camera_error("Erro simulado\x1b[31mANSI");
        match &state.result {
            GuiResult::Error(msg) => {
                assert!(!msg.contains('\x1b'));
                assert!(msg.contains("Erro simulado"));
            }
            other => panic!("esperava Error sanitizado, obtive {other:?}"),
        }
    }

    #[test]
    fn clear_input_undo_history_removes_stored_text_edit_states() {
        let ctx = egui::Context::default();
        let mig_id = egui::Id::new(MIGRATION_INPUT_ID);
        let otp_id = egui::Id::new(OTPAUTH_INPUT_ID);

        ctx.data_mut(|data| {
            data.insert_persisted(mig_id, egui::widgets::text_edit::TextEditState::default());
            data.insert_persisted(otp_id, egui::widgets::text_edit::TextEditState::default());
        });

        clear_input_undo_history(&ctx);

        ctx.data(|data| {
            assert!(data
                .get_temp::<egui::widgets::text_edit::TextEditState>(mig_id)
                .is_none());
            assert!(data
                .get_temp::<egui::widgets::text_edit::TextEditState>(otp_id)
                .is_none());
        });
    }

    #[test]
    fn theme_palette_uses_bluish_violet_accent_for_dark_and_light() {
        for dark in [true, false] {
            let p = palette_for(dark);
            assert!(
                p.accent_fill.b() > p.accent_fill.r() && p.accent_fill.b() > p.accent_fill.g(),
                "accent_fill deve ser roxo-violeta azulado (b > r, g)"
            );
            assert!(
                p.accent.b() > p.accent.r() && p.accent.b() > p.accent.g(),
                "accent deve ser violeta azulado (b > r, g)"
            );
        }
    }

    #[test]
    fn sidebar_migration_icon_and_centered_inputs_render_cleanly() {
        let ctx = egui::Context::default();
        configure_crisp_fonts(&ctx);
        let mut active = GuiTab::DecodeMigration;
        let palette = palette_for(true);
        let mut mig = String::new();
        let mut allocated_input_height = 0.0_f32;
        let mut inner_text_height = 0.0_f32;

        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                OtpGuiApp::sidebar_nav_item(
                    ui,
                    &mut active,
                    GuiTab::DecodeMigration,
                    "⇄",
                    "Migration",
                    palette,
                );
                let scoped = ui.scope(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut mig)
                            .margin(Margin::symmetric(16, 12))
                            .vertical_align(egui::Align::Center)
                            .horizontal_align(egui::Align::Center)
                            .desired_width(400.0)
                            .hint_text("Cole aqui o link..."),
                    )
                });
                allocated_input_height = scoped.response.rect.height();
                inner_text_height = scoped.inner.rect.height();
            });
        });

        assert!(
            allocated_input_height >= inner_text_height + 24.0,
            "padding vertical Margin::symmetric(16, 12) deveria somar >= 24px (outer={allocated_input_height}, inner={inner_text_height})"
        );
    }
}

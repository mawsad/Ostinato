use crate::models::app_data_dir;
use eframe::egui::{self, Color32, Rounding, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(default)]
pub struct ThemePalette {
    pub bg_void: (u8, u8, u8),
    pub bg_bar: (u8, u8, u8),
    pub bg_main: (u8, u8, u8),
    pub bg_panel: (u8, u8, u8),
    pub bg_card: (u8, u8, u8),
    pub bg_widget: (u8, u8, u8),
    pub bg_hover: (u8, u8, u8),
    pub accent_neon: (u8, u8, u8),
    pub accent_bright: (u8, u8, u8),
    pub accent_deep: (u8, u8, u8),
    pub accent_subtle: (u8, u8, u8),
    pub border_dark: (u8, u8, u8),
    pub border_active: (u8, u8, u8),
    pub text_primary: (u8, u8, u8),
    pub text_muted: (u8, u8, u8),
    pub text_gray: (u8, u8, u8),
    pub text_dark_gray: (u8, u8, u8),
    pub glow_strength: f32,
    pub beat_reactivity: f32,
    pub bg_image_alpha: f32,
    pub mascot_alpha: f32,
    pub mascot_scale: f32,
    pub mascot_left: bool,
    pub mascot_bounce: bool,
    pub glass_alpha: f32,
    pub beat_mode: u8,
}

impl Default for ThemePalette {
    fn default() -> Self {
        Self::neverlose_blue()
    }
}

static ACTIVE_PALETTE: RwLock<ThemePalette> = RwLock::new(ThemePalette::neverlose_blue());

pub fn active_palette() -> ThemePalette {
    ACTIVE_PALETTE.read().map(|g| *g).unwrap_or_default()
}

pub fn set_active_palette(pal: ThemePalette) {
    if let Ok(mut g) = ACTIVE_PALETTE.write() {
        *g = pal;
    }
}

impl ThemePalette {
    pub const fn neverlose_blue() -> Self {
        Self {
            bg_void: (3, 5, 10), bg_bar: (5, 8, 15), bg_main: (6, 10, 18),
            bg_panel: (8, 13, 23), bg_card: (10, 16, 28), bg_widget: (12, 20, 34),
            bg_hover: (17, 29, 49), accent_neon: (0, 135, 250), accent_bright: (55, 170, 255),
            accent_deep: (0, 62, 128), accent_subtle: (11, 36, 72), border_dark: (16, 28, 48),
            border_active: (0, 110, 215), text_primary: (135, 185, 240), text_muted: (95, 130, 170),
            text_gray: (82, 96, 116), text_dark_gray: (54, 66, 84),
            glow_strength: 1.0, beat_reactivity: 1.0, bg_image_alpha: 0.25,
            mascot_alpha: 0.88, mascot_scale: 1.0, mascot_left: false,
            mascot_bounce: true, glass_alpha: 0.88, beat_mode: 0,
        }
    }

    pub const fn crimson_sakura() -> Self {
        Self {
            bg_void: (10, 4, 8), bg_bar: (15, 6, 12), bg_main: (18, 7, 14),
            bg_panel: (24, 10, 19), bg_card: (30, 13, 24), bg_widget: (38, 16, 30),
            bg_hover: (52, 22, 42), accent_neon: (255, 56, 125), accent_bright: (255, 125, 175),
            accent_deep: (135, 20, 65), accent_subtle: (72, 14, 38), border_dark: (54, 22, 40),
            border_active: (225, 45, 110), text_primary: (248, 185, 212), text_muted: (185, 125, 152),
            text_gray: (128, 92, 110), text_dark_gray: (86, 58, 72),
            glow_strength: 1.15, beat_reactivity: 1.15, bg_image_alpha: 0.28,
            mascot_alpha: 0.9, mascot_scale: 1.0, mascot_left: false,
            mascot_bounce: true, glass_alpha: 0.86, beat_mode: 0,
        }
    }
    pub const fn cyber_violet() -> Self {
        Self {
            bg_void: (6, 4, 12), bg_bar: (9, 6, 18), bg_main: (11, 8, 22),
            bg_panel: (15, 11, 29), bg_card: (19, 14, 36), bg_widget: (24, 18, 45),
            bg_hover: (34, 25, 62), accent_neon: (148, 72, 255), accent_bright: (190, 135, 255),
            accent_deep: (74, 30, 145), accent_subtle: (42, 18, 82), border_dark: (38, 26, 68),
            border_active: (130, 58, 235), text_primary: (205, 180, 250), text_muted: (148, 125, 192),
            text_gray: (102, 90, 132), text_dark_gray: (68, 58, 92),
            glow_strength: 1.1, beat_reactivity: 1.05, bg_image_alpha: 0.26,
            mascot_alpha: 0.88, mascot_scale: 1.0, mascot_left: false,
            mascot_bounce: true, glass_alpha: 0.86, beat_mode: 0,
        }
    }

    pub const fn emerald_matrix() -> Self {
        Self {
            bg_void: (3, 9, 7), bg_bar: (5, 13, 10), bg_main: (6, 16, 12),
            bg_panel: (8, 21, 16), bg_card: (11, 27, 21), bg_widget: (14, 34, 26),
            bg_hover: (20, 48, 37), accent_neon: (0, 215, 135), accent_bright: (80, 245, 178),
            accent_deep: (0, 105, 65), accent_subtle: (10, 56, 36), border_dark: (18, 46, 35),
            border_active: (0, 185, 115), text_primary: (165, 238, 208), text_muted: (110, 172, 146),
            text_gray: (82, 116, 102), text_dark_gray: (52, 78, 68),
            glow_strength: 1.0, beat_reactivity: 1.0, bg_image_alpha: 0.24,
            mascot_alpha: 0.88, mascot_scale: 1.0, mascot_left: false,
            mascot_bounce: true, glass_alpha: 0.88, beat_mode: 0,
        }
    }

    #[inline]
    pub fn c(rgb: (u8, u8, u8)) -> Color32 { Color32::from_rgb(rgb.0, rgb.1, rgb.2) }
    #[inline]
    pub fn ca(rgb: (u8, u8, u8), a: u8) -> Color32 { Color32::from_rgba_unmultiplied(rgb.0, rgb.1, rgb.2, a) }
}

pub const SKIN_ICON_KEYS: &[&str] = &[
    "play", "pause", "prev", "next", "like", "shuffle", "repeat", "lyrics", "download", "volume",
];

struct GifAnimState {
    gif_bytes: Vec<u8>,
    max_side: u32,
    delays_ms: Vec<u64>,
    total_ms: u64,
    cur_idx: usize,
    start_time: std::time::Instant,
}

#[derive(Clone)]
pub struct SkinGraphic {
    pub tex: egui::TextureHandle,
    pub aspect: f32,
    anim: Option<std::sync::Arc<std::sync::Mutex<GifAnimState>>>,
}

impl SkinGraphic {
    pub fn current_texture(&self, ctx: &egui::Context) -> &egui::TextureHandle {
        if let Some(anim_arc) = &self.anim {
            let mut next_img = None;
            if let Ok(mut st) = anim_arc.lock() {
                if st.total_ms > 0 && st.delays_ms.len() > 1 {
                    let elapsed = (st.start_time.elapsed().as_millis() as u64) % st.total_ms;
                    let mut acc = 0u64;
                    let mut target = 0usize;
                    for (i, &d) in st.delays_ms.iter().enumerate() {
                        acc += d;
                        if elapsed < acc {
                            target = i;
                            break;
                        }
                    }
                    let next_wake = acc.saturating_sub(elapsed).clamp(16, 120);
                    ctx.request_repaint_after(std::time::Duration::from_millis(next_wake));

                    if target != st.cur_idx {
                        use image::AnimationDecoder;
                        let max_s = st.max_side;
                        let decoded = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(&st.gif_bytes))
                            .ok()
                            .and_then(|dec| dec.into_frames().nth(target))
                            .and_then(|res| res.ok());
                        if let Some(frame) = decoded {
                            let dyn_img = image::DynamicImage::ImageRgba8(frame.into_buffer());
                            let scaled = if dyn_img.width() > max_s || dyn_img.height() > max_s {
                                dyn_img.resize(max_s, max_s, image::imageops::FilterType::Triangle)
                            } else {
                                dyn_img
                            };
                            let rgba = scaled.to_rgba8();
                            next_img = Some(egui::ColorImage::from_rgba_unmultiplied(
                                [rgba.width() as usize, rgba.height() as usize],
                                rgba.as_flat_samples().as_slice(),
                            ));
                            st.cur_idx = target;
                        }
                    }
                }
            }
            if let Some(cimg) = next_img {
                let mut tex_mut = self.tex.clone();
                tex_mut.set(cimg, egui::TextureOptions::LINEAR);
            }
        }
        &self.tex
    }
}


#[derive(Clone, Default)]
pub struct LoadedSkinAssets {
    pub bg_tex: Option<SkinGraphic>,
    pub bg_path: Option<PathBuf>,
    pub mascot_tex: Option<SkinGraphic>,
    pub mascot_aspect: f32,
    pub font_path: Option<PathBuf>,
    pub icons: HashMap<String, SkinGraphic>,
}

pub fn skins_dir() -> PathBuf {
    let dir = app_data_dir().join("Skins");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn active_skin_folder(active_theme: &str) -> PathBuf {
    if let Some(name) = active_theme.strip_prefix("Skin: ") {
        skins_dir().join(name)
    } else {
        let dir = skins_dir().join("StudioSkin");
        let _ = fs::create_dir_all(&dir);
        dir
    }
}

pub fn list_available_skins() -> Vec<String> {
    let mut names = vec![
        "Neverlose Blue".into(), "Crimson Sakura".into(),
        "Cyber Violet".into(), "Emerald Matrix".into(),
    ];
    if let Ok(entries) = fs::read_dir(skins_dir()) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Some(n) = p.file_name().and_then(|s| s.to_str()) {
                    let full = format!("Skin: {}", n);
                    if !names.contains(&full) { names.push(full); }
                }
            }
        }
    }
    names
}

fn load_graphic_any_size(ctx: &egui::Context, name: &str, path: &Path, max_side: u32) -> Option<SkinGraphic> {
    let is_gif = path.extension().and_then(|e| e.to_str()).map(|s| s.eq_ignore_ascii_case("gif")).unwrap_or(false);
    if is_gif {
        use image::AnimationDecoder;
        let bytes = fs::read(path).ok()?;
        let dec = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(&bytes)).ok()?;
        let mut delays_ms = Vec::new();
        let mut first_rgba = None;
        let mut aspect = 1.0;

        for (idx, fr_res) in dec.into_frames().enumerate() {
            let Ok(fr) = fr_res else { break };
            let (num, den) = fr.delay().numer_denom_ms();
            let ms = if den == 0 { 80 } else { (num / den).max(25) as u64 };
            delays_ms.push(ms);
            if idx == 0 {
                let buf = fr.into_buffer();
                aspect = buf.width().max(1) as f32 / buf.height().max(1) as f32;
                let dyn_img = image::DynamicImage::ImageRgba8(buf);
                let scaled = if dyn_img.width() > max_side || dyn_img.height() > max_side {
                    dyn_img.resize(max_side, max_side, image::imageops::FilterType::Triangle)
                } else {
                    dyn_img
                };
                first_rgba = Some(scaled.to_rgba8());
            }
        }
        let rgba = first_rgba?;
        let cimg = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_flat_samples().as_slice());
        let tex = ctx.load_texture(name.to_owned(), cimg, egui::TextureOptions::LINEAR);
        let total_ms = delays_ms.iter().sum();
        let anim = if delays_ms.len() > 1 {
            Some(std::sync::Arc::new(std::sync::Mutex::new(GifAnimState {
                gif_bytes: bytes,
                max_side,
                delays_ms,
                total_ms,
                cur_idx: 0,
                start_time: std::time::Instant::now(),
            })))
        } else {
            None
        };
        return Some(SkinGraphic { tex, aspect, anim });
    }

    let dyn_img = image::open(path).ok()?;
    let (w, h) = (dyn_img.width().max(1), dyn_img.height().max(1));
    let aspect = w as f32 / h as f32;
    let scaled = if w > max_side || h > max_side {
        dyn_img.resize(max_side, max_side, image::imageops::FilterType::Triangle)
    } else {
        dyn_img
    };
    let rgba = scaled.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let cimg = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_flat_samples().as_slice());
    let tex = ctx.load_texture(name.to_owned(), cimg, egui::TextureOptions::LINEAR);
    Some(SkinGraphic { tex, aspect, anim: None })
}

pub fn load_skin_assets_from_dir(ctx: &egui::Context, dir: &Path) -> LoadedSkinAssets {
    let bg_path = ["bg.gif", "bg.png", "bg.jpg", "bg.jpeg", "bg.webp", "background.gif", "background.png", "background.jpg"]
        .iter().map(|f| dir.join(f)).find(|p| p.exists());
    let bg_tex = bg_path.as_deref().and_then(|p| {
        let cap = if p.extension().and_then(|e| e.to_str()).map(|s| s.eq_ignore_ascii_case("gif")).unwrap_or(false) { 640 } else { 2560 };
        load_graphic_any_size(ctx, "skin_bg_tex", p, cap)
    });

    let mascot_p = ["mascot.gif", "mascot.png", "mascot.webp", "mascot.jpg", "character.gif", "character.png", "anime.gif", "anime.png"]
        .iter().map(|f| dir.join(f)).find(|p| p.exists());
    let (mascot_tex, mascot_aspect) = mascot_p.as_deref()
        .and_then(|p| {
            let cap = if p.extension().and_then(|e| e.to_str()).map(|s| s.eq_ignore_ascii_case("gif")).unwrap_or(false) { 360 } else { 1400 };
            load_graphic_any_size(ctx, "skin_mascot_tex", p, cap)
        })
        .map(|g| { let a = g.aspect; (Some(g), a) })
        .unwrap_or((None, 0.75));

    let font_path = ["font.ttf", "font.otf"].iter().map(|f| dir.join(f)).find(|p| p.exists());

    let mut icons = HashMap::new();
    for &key in SKIN_ICON_KEYS {
        let p_gif = dir.join(format!("{}.gif", key));
        let p_png = dir.join(format!("{}.png", key));
        let chosen = if p_gif.exists() { Some(p_gif) } else if p_png.exists() { Some(p_png) } else { None };
        if let Some(p) = chosen {
            if let Some(g) = load_graphic_any_size(ctx, &format!("skin_ic_{}", key), &p, 96) {
                icons.insert(key.to_string(), g);
            }
        }
    }
    LoadedSkinAssets { bg_tex, bg_path, mascot_tex, mascot_aspect, font_path, icons }
}


pub fn load_theme_and_assets(ctx: &egui::Context, selected: &str, custom_override: Option<ThemePalette>) -> (ThemePalette, LoadedSkinAssets) {
    if selected == "Custom" {
        let dir = skins_dir().join("StudioSkin");
        return (custom_override.unwrap_or_default(), load_skin_assets_from_dir(ctx, &dir));
    }
    if let Some(folder_name) = selected.strip_prefix("Skin: ") {
        let dir = skins_dir().join(folder_name);
        let pal = fs::read_to_string(dir.join("skin.json"))
            .ok().and_then(|s| serde_json::from_str::<ThemePalette>(&s).ok())
            .or(custom_override).unwrap_or_default();
        return (pal, load_skin_assets_from_dir(ctx, &dir));
    }
    let pal = match selected {
        "Crimson Sakura" => ThemePalette::crimson_sakura(),
        "Cyber Violet" => ThemePalette::cyber_violet(),
        "Emerald Matrix" => ThemePalette::emerald_matrix(),
        _ => ThemePalette::neverlose_blue(),
    };
    (pal, LoadedSkinAssets::default())
}


pub fn apply_skin_font(ctx: &egui::Context, font_path: Option<&Path>) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(p) = font_path {
        if let Ok(bytes) = fs::read(p) {
            fonts.font_data.insert("skin_custom".to_owned(), egui::FontData::from_owned(bytes).into());
            if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                fam.insert(0, "skin_custom".to_owned());
            }
        }
    }
    ctx.set_fonts(fonts);
}

pub fn auto_palette_from_image(img_path: &Path, base: ThemePalette) -> Result<ThemePalette, String> {
    let img = image::open(img_path).map_err(|e| format!("Ошибка чтения картинки: {}", e))?.to_rgb8();
    let (w, h) = img.dimensions();
    let step_x = (w / 48).max(1);
    let step_y = (h / 48).max(1);
    let (mut sr, mut sg, mut sb, mut w_sum) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);

    for y in (0..h).step_by(step_y as usize) {
        for x in (0..w).step_by(step_x as usize) {
            let p = img.get_pixel(x, y).0;
            let (r, g, b) = (p[0] as f64, p[1] as f64, p[2] as f64);
            let mx = r.max(g).max(b);
            let mn = r.min(g).min(b);
            let sat = if mx > 1.0 { (mx - mn) / mx } else { 0.0 };
            let lum = (r + g + b) / 3.0;
            if lum > 22.0 && lum < 245.0 {
                let weight = 0.15 + sat * sat * 3.5;
                sr += r * weight; sg += g * weight; sb += b * weight; w_sum += weight;
            }
        }
    }
    if w_sum < 0.01 { return Err("Картинка слишком тёмная или однотонная".into()); }
    let (mut nr, mut ng, mut nb) = ((sr / w_sum) as f32, (sg / w_sum) as f32, (sb / w_sum) as f32);
    let peak = nr.max(ng).max(nb).max(1.0);
    let boost = (235.0 / peak).clamp(1.0, 2.4);
    nr = (nr * boost).clamp(30.0, 255.0);
    ng = (ng * boost).clamp(30.0, 255.0);
    nb = (nb * boost).clamp(30.0, 255.0);

    let sc = |k: f32, add: f32| -> (u8, u8, u8) {
        ((nr * k + add).clamp(0.0, 255.0) as u8, (ng * k + add).clamp(0.0, 255.0) as u8, (nb * k + add).clamp(0.0, 255.0) as u8)
    };
    let mut out = base;
    out.accent_neon = (nr as u8, ng as u8, nb as u8);
    out.accent_bright = sc(0.75, 64.0);
    out.accent_deep = sc(0.42, 4.0);
    out.accent_subtle = sc(0.22, 6.0);
    out.border_active = sc(0.85, 0.0);
    out.border_dark = sc(0.18, 12.0);
    out.bg_void = sc(0.035, 3.0);
    out.bg_bar = sc(0.05, 5.0);
    out.bg_main = sc(0.065, 6.0);
    out.bg_panel = sc(0.085, 8.0);
    out.bg_card = sc(0.11, 10.0);
    out.bg_widget = sc(0.14, 12.0);
    out.bg_hover = sc(0.20, 16.0);
    out.text_primary = sc(0.35, 160.0);
    out.text_muted = sc(0.28, 110.0);
    Ok(out)
}
pub fn save_skin_folder(folder_name: &str, pal: &ThemePalette) -> Result<PathBuf, String> {
    let clean: String = folder_name.chars().filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ').collect();
    let name = if clean.trim().is_empty() { "MyAnimeSkin" } else { clean.trim() };
    let dir = skins_dir().join(name);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(pal).map_err(|e| e.to_string())?;
    fs::write(dir.join("skin.json"), json).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn export_skin_zip(skin_dir: &Path, pal: &ThemePalette, dest_zip: &Path) -> Result<(), String> {
    fs::create_dir_all(skin_dir).map_err(|e| e.to_string())?;
    if let Ok(json) = serde_json::to_string_pretty(pal) {
        let _ = fs::write(skin_dir.join("skin.json"), json);
    }
    let file = File::create(dest_zip).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    if let Ok(entries) = fs::read_dir(skin_dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_file() {
                if let Some(fname) = p.file_name().and_then(|s| s.to_str()) {
                    if let Ok(bytes) = fs::read(&p) {
                        zip.start_file(fname, opts).map_err(|e| e.to_string())?;
                        zip.write_all(&bytes).map_err(|e| e.to_string())?;
                    }
                }
            }
        }
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn import_skin_zip(zip_path: &Path) -> Result<String, String> {
    let stem = zip_path.file_stem().and_then(|s| s.to_str()).unwrap_or("ImportedSkin");
    let clean: String = stem.chars().filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ').collect();
    let folder_name = if clean.trim().is_empty() { "ImportedSkin".to_string() } else { clean.trim().to_string() };
    let target_dir = skins_dir().join(&folder_name);
    fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;

    let file = File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Повреждённый архив скина: {}", e))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() { continue; }
        let raw_name = Path::new(entry.name()).file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if raw_name.is_empty() || raw_name.contains("..") { continue; }
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        let _ = fs::write(target_dir.join(raw_name), buf);
    }
    Ok(format!("Skin: {}", folder_name))
}

pub fn paint_neon_glow(painter: &egui::Painter, rect: egui::Rect, rounding: Rounding, rgb: (u8, u8, u8), intensity: f32) {
    if intensity <= 0.02 { return; }
    let k = intensity.clamp(0.0, 2.4);
    for (exp, a_base) in [(18.0_f32, 13.0_f32), (10.0, 26.0), (4.2, 48.0)] {
        let a = (a_base * k).clamp(0.0, 165.0) as u8;
        let r = rect.expand(exp * (0.65 + 0.35 * k));
        let rnd = Rounding::same(rounding.nw + exp * 0.55);
        painter.rect_stroke(r, rnd, Stroke::new(2.2, ThemePalette::ca(rgb, a)));
    }
}

pub fn paint_scroll_fades(painter: &egui::Painter, rect: egui::Rect, bg: (u8, u8, u8), h: f32) {
    let steps = 5;
    let sh = h / steps as f32;
    let glass = active_palette().glass_alpha.clamp(0.2, 1.0);
    for i in 0..steps {
        let t = 1.0 - (i as f32 / steps as f32);
        let col = ThemePalette::ca(bg, (t * t * 225.0 * glass) as u8);
        painter.rect_filled(egui::Rect::from_min_size(egui::pos2(rect.left(), rect.top() + i as f32 * sh), Vec2::new(rect.width(), sh + 0.5)), Rounding::ZERO, col);
        painter.rect_filled(egui::Rect::from_min_size(egui::pos2(rect.left(), rect.bottom() - (i as f32 + 1.0) * sh), Vec2::new(rect.width(), sh + 0.5)), Rounding::ZERO, col);
    }
}

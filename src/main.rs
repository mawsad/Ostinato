#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod app;
mod audio;
mod models;
mod providers;
mod theme;
mod ui_widgets;
mod win_sys;
mod discord;
 
use app::OstinatoApp;
use eframe::egui;
use image as _;
use std::sync::Arc;

fn build_window_icon() -> egui::IconData {
    let mut rgba = vec![0u8; 64 * 64 * 4];
    for y in 0..64 {
        for x in 0..64 {
            let idx = (y * 64 + x) * 4;
            let dx = x as f32 - 31.5;
            let dy = y as f32 - 31.5;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= 30.0 {
                if (dist - 21.0).abs() <= 4.2 || dist <= 6.5 {
                    rgba[idx] = 0;
                    rgba[idx + 1] = 135;
                    rgba[idx + 2] = 250;
                    rgba[idx + 3] = 255;
                } else {
                    rgba[idx] = 4;
                    rgba[idx + 1] = 7;
                    rgba[idx + 2] = 14;
                    rgba[idx + 3] = 255;
                }
            }
        }
    }
    egui::IconData {
        rgba,
        width: 64,
        height: 64,
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Ostinato v4")
            .with_icon(Arc::new(build_window_icon()))
            .with_decorations(false)
            .with_resizable(true)
            .with_inner_size([1020.0, 650.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Ostinato",
        options,
        Box::new(|cc| {
            let mut fonts = egui::FontDefinitions::default();
            if let Ok(bold_bytes) = std::fs::read("C:\\Windows\\Fonts\\segoeuib.ttf")
                .or_else(|_| std::fs::read("C:\\Windows\\Fonts\\arialbd.ttf"))
            {
                fonts.font_data.insert(
                    "karaoke_bold".to_owned(),
                    egui::FontData::from_owned(bold_bytes).into(),
                );
                if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                    fam.insert(0, "karaoke_bold".to_owned());
                }
            }
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(OstinatoApp::new(cc)))
        }),
    )
}

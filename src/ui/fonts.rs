use eframe::egui;
use std::sync::Arc;

pub const PLEX_SANS: &[u8] = include_bytes!("../../resources/fonts/IBMPlexSans-Variable.ttf");
pub const PLEX_MONO: &[u8] = include_bytes!("../../resources/fonts/IBMPlexMono-Regular.ttf");

pub fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 1. Install IBM Plex Sans (Variable) for proportional UI text
    fonts.font_data.insert(
        "plex_sans".to_owned(),
        Arc::new(egui::FontData::from_static(PLEX_SANS)),
    );

    // 2. Install IBM Plex Mono for broadcast timecode, frame numbers, and technical telemetry
    fonts.font_data.insert(
        "plex_mono".to_owned(),
        Arc::new(egui::FontData::from_static(PLEX_MONO)),
    );

    // Make IBM Plex Sans the first priority proportional font
    fonts.families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "plex_sans".to_owned());

    // Make IBM Plex Mono the first priority monospace font
    fonts.families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "plex_mono".to_owned());

    // Named style for bold/medium if needed
    fonts.families
        .insert(egui::FontFamily::Name("mono_bold".into()), vec!["plex_mono".to_owned()]);

    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fonts_setup() {
        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        let _ = ctx.run_ui(Default::default(), |ui| {
            ui.label("Hello world 12345");
        });
    }
}

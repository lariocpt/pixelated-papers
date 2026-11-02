//! Native control window for pixelated-papers (cargo feature `gui`).
//!
//! The module is deliberately split so the window is a *client*: [`model`]
//! holds all the (headless, unit-tested) view-model logic and mutates state
//! only through `commands::*` + `cli::apply_now`; [`app`] is a thin eframe
//! paint layer that renders that model and forwards widget events into it.
//! Enabling `gui` links eframe's OpenGL/winit tree; the default build never
//! sees it, so the shipped binary stays byte-identical to the pre-GUI one.

mod app;
mod model;

use anyhow::Result;
use eframe::egui;
use pixcore::{Config, Paths};

/// Open the control window. Mirrors [`crate::tui::run`]: build the state,
/// hand it to the frame loop, block until the window closes.
pub fn run(paths: &Paths, cfg: &Config) -> Result<()> {
    let model = model::Gui::new(paths.clone(), cfg.clone())?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 620.0])
            .with_min_inner_size([680.0, 480.0])
            .with_title("Pixelated Papers"),
        ..Default::default()
    };
    eframe::run_native(
        "pixpapers",
        options,
        Box::new(
            move |_cc| -> Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>> {
                Ok(Box::new(app::GuiApp::new(model)))
            },
        ),
    )
    .map_err(|e| anyhow::anyhow!("gui window closed with an error: {e}"))
}

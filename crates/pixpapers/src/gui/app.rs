//! Thin eframe/egui paint layer for the native GUI (feature `gui`).
//!
//! Everything here is chrome: it reads [`Gui`] and forwards widget events
//! back into its methods. There is no business logic in this file — no
//! manifest/config write, no apply, no resolution happens outside the
//! view-model, so the window stays a pure client (invariant #1).

use std::path::PathBuf;

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions, Vec2};
use pixcore::{Dither, GutterStyle, PaletteMode, Pixel};

use super::model::{Gui, PLACEMENTS, View};

/// One decoded thumbnail, keyed by the cached-variant path it came from.
struct Thumbnail {
    key: Option<PathBuf>,
    tex: Option<TextureHandle>,
}

pub struct GuiApp {
    model: Gui,
    thumb: Thumbnail,
}

impl GuiApp {
    pub fn new(model: Gui) -> Self {
        Self {
            model,
            thumb: Thumbnail {
                key: None,
                tex: None,
            },
        }
    }

    /// Decode the current entry's cached variant into a texture, reusing the
    /// existing render (never renders). Placeholder if nothing is cached.
    fn sync_thumbnail(&mut self, ctx: &egui::Context) {
        let want = self.model.thumbnail_path();
        if want == self.thumb.key {
            return;
        }
        self.thumb.tex = want
            .as_ref()
            .and_then(|p| load_texture(ctx, p))
            .or_else(|| Some(placeholder(ctx)));
        self.thumb.key = want;
    }
}

/// Decode a PNG into an egui texture with the `image` crate.
fn load_texture(ctx: &egui::Context, path: &PathBuf) -> Option<TextureHandle> {
    let img = image::open(path).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let color = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
    Some(ctx.load_texture(
        path.to_string_lossy().into_owned(),
        color,
        TextureOptions::LINEAR,
    ))
}

/// A neutral placeholder texture shown when no variant is cached yet.
fn placeholder(ctx: &egui::Context) -> TextureHandle {
    let size = [128usize, 128];
    let px = vec![Color32::from_gray(40); size[0] * size[1]];
    ctx.load_texture(
        "placeholder",
        ColorImage::new(size, px),
        TextureOptions::NEAREST,
    )
}

/// A combo box over an `Option<T>` knob (offers "— inherit —" + every value).
/// Selecting rewrites `opt` in the draft; the value reaches disk only on
/// [`Gui::save`].
fn knob_combo<T: Copy + PartialEq + Clone + ToString>(
    ui: &mut egui::Ui,
    label: &str,
    opt: &mut Option<T>,
    options: &[T],
) {
    let selected = opt
        .as_ref()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "— inherit —".to_string());
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(label)
            .selected_text(selected)
            .show_ui(ui, |ui| {
                ui.selectable_value(opt, None, "— inherit —");
                for v in options {
                    ui.selectable_value(opt, Some(*v), v.to_string());
                }
            });
    });
}

impl eframe::App for GuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.sync_thumbnail(&ctx);

        // Arrow keys move the list selection (the TUI's up/down), but only
        // when no text field owns the keyboard — otherwise they'd fight an
        // in-progress edit of the title / path / interval box.
        if self.model.view == View::Library && ctx.memory(|m| m.focused().is_none()) {
            ctx.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    self.model.move_sel(-1);
                } else if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    self.model.move_sel(1);
                }
            });
        }

        // ---- top bar: view tabs + global switches ----
        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.model.view, View::Library, "Library");
                ui.selectable_value(&mut self.model.view, View::Status, "Status");
                ui.separator();

                let was = self.model.cfg.enabled;
                if ui
                    .checkbox(&mut self.model.cfg.enabled, "Pixelation")
                    .changed()
                {
                    let want = self.model.cfg.enabled;
                    if want != was {
                        let _ = self.model.set_enabled(want);
                    }
                }

                ui.separator();
                let sl = self.model.cfg.slideshow.enabled;
                let sl_btn = if sl {
                    "Slideshow: on"
                } else {
                    "Slideshow: off"
                };
                if ui.button(sl_btn).clicked() {
                    if sl {
                        let _ = self.model.slideshow_off();
                    } else {
                        let _ = self.model.slideshow_on();
                    }
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.model.interval)
                        .hint_text("interval s")
                        .desired_width(70.0),
                );
                if ui.button("Set").clicked() {
                    let _ = self.model.slideshow_interval();
                }
            });
        });

        // ---- bottom: status line ----
        egui::Panel::bottom("status").show(ui, |ui| {
            let msg = self.model.last.clone();
            ui.label(msg);
        });

        // ---- left: library list + add ----
        if self.model.view == View::Library {
            egui::Panel::left("list")
                .default_size(230.0)
                .show(ui, |ui| {
                    let current = self.model.lib.current.clone();
                    let sel = self.model.sel;
                    let empty = self.model.lib.entries.is_empty();
                    let items: Vec<(usize, String)> = self
                        .model
                        .lib
                        .entries
                        .iter()
                        .enumerate()
                        .map(|(i, e)| {
                            let star = if current.as_deref() == Some(e.id.as_str()) {
                                "● "
                            } else {
                                ""
                            };
                            (i, format!("{star}{}", e.title))
                        })
                        .collect();
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for (i, label) in &items {
                                if ui.selectable_label(*i == sel, label).clicked() {
                                    self.model.select(*i);
                                }
                            }
                            if empty {
                                ui.weak("library empty — add one below");
                            }
                        });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.model.add_path)
                                .hint_text("image path")
                                .desired_width(150.0),
                        );
                        if ui.button("Add").clicked() {
                            let _ = self.model.add();
                        }
                    });
                });
        }

        // ---- centre ----
        egui::CentralPanel::default().show(ui, |ui| match self.model.view {
            View::Library => self.paint_library(ui),
            View::Status => self.paint_status(ui),
        });
    }
}

impl GuiApp {
    fn paint_library(&mut self, ui: &mut egui::Ui) {
        if self.model.selected().is_none() {
            ui.centered_and_justified(|ui| {
                ui.heading("No entry selected");
            });
            return;
        }

        ui.horizontal_top(|ui| {
            // thumbnail column — cached variant only, never a fresh render
            ui.vertical(|ui| {
                if let Some(tex) = &self.thumb.tex {
                    ui.add(
                        egui::Image::from_texture(tex)
                            .max_size(Vec2::new(320.0, 320.0))
                            .maintain_aspect_ratio(true),
                    );
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_sized([220.0, 220.0], egui::Label::new("no cached preview yet"));
                    });
                }
            });

            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| {
                self.paint_actions(ui);
                ui.separator();
                self.paint_knobs(ui);
                ui.separator();
                self.paint_defaults(ui);
            });
        });
    }

    fn paint_actions(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Set now").clicked() {
                let _ = self.model.set_now();
            }
            if ui.button("Cycle →").clicked() {
                let _ = self.model.cycle(1);
            }
            if ui.button("← Cycle").clicked() {
                let _ = self.model.cycle(-1);
            }
            if ui.button("Remove").clicked() {
                let _ = self.model.remove_selected();
            }
        });
    }

    fn paint_knobs(&mut self, ui: &mut egui::Ui) {
        let dirty = self.model.draft_dirty();
        let editing = self.model.draft().name.clone();
        ui.group(|ui| {
            ui.heading(format!("This wallpaper — {editing}"));

            // All edits mutate the draft only; scoped so the borrow of
            // self.model ends before validate/save/revert run below.
            {
                let d = self.model.draft_mut();
                ui.horizontal(|ui| {
                    ui.label("title");
                    ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(180.0));
                });
                knob_combo(ui, "pixel", &mut d.pixel, &Pixel::ALL);
                knob_combo(ui, "placement", &mut d.placement, &PLACEMENTS);
                knob_combo(ui, "palette", &mut d.palette, &PaletteMode::ALL);
                knob_combo(ui, "dither", &mut d.dither, &Dither::ALL);
                knob_combo(ui, "gutter-style", &mut d.gutter_style, &GutterStyle::ALL);
                ui.horizontal(|ui| {
                    ui.label("bg colour");
                    ui.add(
                        egui::TextEdit::singleline(&mut d.bg)
                            .hint_text("#rrggbb / empty")
                            .desired_width(120.0),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("gutter px");
                    ui.add(
                        egui::TextEdit::singleline(&mut d.gutter)
                            .hint_text("inherit")
                            .desired_width(120.0),
                    );
                });
            }

            if let Some(err) = self.model.validate() {
                ui.colored_label(Color32::LIGHT_RED, err);
            }
            ui.horizontal(|ui| {
                ui.add_enabled_ui(dirty, |ui| {
                    if ui.button("Save").clicked() {
                        let _ = self.model.save();
                    }
                });
                if ui.button("Revert").clicked() {
                    self.model.revert();
                }
                if dirty {
                    ui.weak("unsaved changes");
                }
            });
        });
    }

    fn paint_defaults(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.heading("Library defaults");
            ui.label("changing a default applies library-wide, like `pixpapers defaults`");

            defaults_combo(
                &mut self.model,
                ui,
                "pixel",
                |m| m.cfg.default_pixel,
                |m, v| {
                    let _ = m.default_pixel(v);
                },
                &Pixel::ALL,
            );
            defaults_combo(
                &mut self.model,
                ui,
                "palette",
                |m| m.cfg.palette,
                |m, v| {
                    let _ = m.default_palette(v);
                },
                &PaletteMode::ALL,
            );
            defaults_combo(
                &mut self.model,
                ui,
                "dither",
                |m| m.cfg.dither,
                |m, v| {
                    let _ = m.default_dither(v);
                },
                &Dither::ALL,
            );
            defaults_combo(
                &mut self.model,
                ui,
                "placement",
                |m| m.cfg.default_placement,
                |m, v| {
                    let _ = m.default_placement(v);
                },
                &PLACEMENTS,
            );
            defaults_combo(
                &mut self.model,
                ui,
                "gutter-style",
                |m| m.cfg.default_gutter_style,
                |m, v| {
                    let _ = m.default_gutter_style(v);
                },
                &GutterStyle::ALL,
            );
        });
    }

    fn paint_status(&mut self, ui: &mut egui::Ui) {
        if ui.button("Refresh").clicked() {
            self.model.refresh_status();
        }
        if self.model.status_text.is_none() {
            self.model.refresh_status();
        }
        ui.separator();
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let text = self.model.status_text.clone().unwrap_or_default();
                ui.monospace(text);
            });
    }
}

/// A defaults combo box: selecting immediately calls the matching
/// `set_default_*` command (the CLI's `defaults` path), then reloads.
fn defaults_combo<T: Copy + PartialEq + Clone + ToString>(
    model: &mut Gui,
    ui: &mut egui::Ui,
    label: &str,
    get: fn(&Gui) -> T,
    set: fn(&mut Gui, T),
    options: &[T],
) {
    let cur = get(model);
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(format!("def_{label}"))
            .selected_text(cur.to_string())
            .show_ui(ui, |ui| {
                for v in options {
                    // Don't bind into cfg directly — the setter reloads it.
                    let chosen = ui.selectable_label(cur == *v, v.to_string()).clicked();
                    if chosen {
                        set(model, *v);
                    }
                }
            });
    });
}

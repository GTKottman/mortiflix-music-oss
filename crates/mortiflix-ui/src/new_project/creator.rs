//! The preset creator (doc 31b): 8 short pages in the New project window's place. Left: the steps,
//! each with what you picked under it. Right: one page. Bottom: ‹ Back to New project, Step N of 8,
//! Back / Next (Save preset / Save changes on the last page). A built-in preset is never changed:
//! saving makes your own copy. Saving is `presets.save`, deleting your own is `presets.delete`.

use egui::{Align, Color32, CornerRadius, FontId, Layout, RichText, Sense, Stroke, StrokeKind, vec2};
use serde_json::{Value, json};

use super::model::{self, Preset, PresetDraft, Voice};
use super::{CREATOR_W, Load, PICK_PICTURES, PICK_SKILL_DIR, accent_button, chip, drops, kind_glyph, pick, quiet_button};
use crate::MfxCtx;

pub const STEPS: [&str; 8] = ["Start from", "Length and shape", "Look", "Voice", "Music", "Stages", "Skills", "Name and save"];
const LIST_W: f32 = 230.0;
const PAGE_H: f32 = 600.0;

/// What the creator wants the window to do next.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    Stay,
    /// Open the Music theme page for this preset.
    Music,
    /// ‹ Back to New project without saving.
    Leave,
    Saved {
        slug: String,
        name: String,
    },
    Deleted {
        name: String,
    },
}

#[derive(Clone, Default)]
pub struct Creator {
    /// ✎: changing a preset (built in: saves as your copy).
    pub change: bool,
    /// The name of the preset being changed (title breadcrumb).
    pub change_name: String,
    pub step: usize,
    pub draft: PresetDraft,
    /// What the draft was when the creator opened (Leave asks only if something changed).
    original: PresetDraft,
    confirm_leave: bool,
    confirm_delete: bool,
    save_req: Option<u64>,
    delete_req: Option<u64>,
    result: Option<Out>,
    pub error: String,
    pictures_error: String,
    skills_error: String,
    custom_length: bool,
    custom_aspect: bool,
    voices: Vec<Voice>,
    /// A narration voice engine is set up (Settings › Narration).
    voice_set_up: bool,
    voices_load: Load,
    /// The start preset's choices (pages 2, 4, 5).
    lengths: Vec<String>,
    aspects: Vec<String>,
    narrations: Vec<String>,
    pub music_choices: Vec<String>,
    pub music_ui: super::music::MusicUi,
}

impl Creator {
    pub fn new(p: &Preset, change: bool) -> Self {
        let draft = PresetDraft::from_preset(p, change);
        let mut c = Self { change, change_name: p.name.clone(), draft: draft.clone(), original: draft, ..Self::default() };
        c.choices_from(p);
        c
    }

    fn choices_from(&mut self, p: &Preset) {
        let of = |id: &str| p.question(id).map(|q| q.choices.clone()).unwrap_or_default();
        self.lengths = of("length");
        self.aspects = of("aspect");
        self.narrations = of("voice");
        self.music_choices = of("music");
        if self.lengths.is_empty() {
            self.lengths = ["30 s", "60 s", "90 s", "2 min"].map(String::from).to_vec();
        }
        if self.aspects.is_empty() {
            self.aspects = ["16:9", "9:16", "1:1", "4:5"].map(String::from).to_vec();
        }
        if self.narrations.is_empty() {
            self.narrations = ["Narrated", "On-screen text and music only"].map(String::from).to_vec();
        }
        self.custom_length = !self.draft.length.is_empty() && !self.lengths.contains(&self.draft.length);
        self.custom_aspect = !self.draft.aspect.is_empty() && !self.aspects.contains(&self.draft.aspect);
    }

    fn dirty(&self) -> bool {
        self.draft != self.original
    }

    /// Read the replies the creator waits for.
    pub fn poll(&mut self, ctx: &mut MfxCtx) {
        if self.voices_load == Load::Idle && ctx.service.status().is_connected() {
            self.voices_load = Load::Loading(ctx.service.request("voice.list", json!({})));
        }
        if let Load::Loading(rid) = self.voices_load
            && let Some(r) = ctx.service.take_reply(rid)
        {
            (self.voices, self.voice_set_up) = if r.ok { model::parse_voices(&r.result) } else { (Vec::new(), false) };
            self.voices_load = if r.ok { Load::Loaded } else { Load::Failed(err_text(&r.error)) };
        }
        if let Some(rid) = self.save_req
            && let Some(r) = ctx.service.take_reply(rid)
        {
            self.save_req = None;
            if r.ok {
                let slug = r.result.get("slug").and_then(Value::as_str).unwrap_or_default().chars().take(80).collect::<String>();
                let name =
                    r.result.get("name").and_then(Value::as_str).map(|s| s.chars().take(80).collect()).unwrap_or_else(|| self.draft.name.trim().to_string());
                self.result = Some(Out::Saved { slug, name });
            } else {
                self.error = err_text(&r.error);
                self.step = 7;
            }
        }
        if let Some(rid) = self.delete_req
            && let Some(r) = ctx.service.take_reply(rid)
        {
            self.delete_req = None;
            if r.ok {
                self.result = Some(Out::Deleted { name: self.change_name.clone() });
            } else {
                self.error = format!("Couldn't delete it: {}", err_text(&r.error));
            }
        }
        super::music::poll(ctx, &mut self.music_ui, &mut self.draft.theme);
    }

    /// Draw the creator. Returns what the window should do.
    pub fn show(&mut self, ui: &mut egui::Ui, ctx: &mut MfxCtx, presets: &[Preset]) -> Out {
        if let Some(out) = self.result.take() {
            return out;
        }
        let t = ctx.theme;
        ui.set_width(CREATOR_W);
        crate::service_notice(ui, ctx);
        let mut out = Out::Stay;
        ui.horizontal_top(|ui| {
            self.step_list(ui, ctx);
            // a fixed-height divider (a separator in a row grows to the whole screen)
            let (line, _) = ui.allocate_exact_size(vec2(9.0, PAGE_H), Sense::hover());
            ui.painter().vline(line.center().x, line.y_range(), Stroke::new(1.0, t.separator));
            ui.vertical(|ui| {
                ui.set_width(CREATOR_W - LIST_W - 24.0);
                ui.set_min_height(PAGE_H);
                egui::ScrollArea::vertical().id_salt("mfx-np-creator-page").max_height(PAGE_H).auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    match self.step {
                        0 => self.page_start(ui, ctx, presets),
                        1 => self.page_shape(ui, ctx),
                        2 => self.page_look(ui, ctx),
                        3 => self.page_voice(ui, ctx),
                        4 => {
                            if self.page_music(ui, ctx) {
                                out = Out::Music;
                            }
                        }
                        5 => self.page_stages(ui, ctx),
                        6 => self.page_skills(ui, ctx),
                        _ => self.page_save(ui, ctx, presets),
                    }
                });
            });
        });
        ui.separator();
        if let Some(o) = self.footer(ui, ctx, presets) {
            out = o;
        }
        // Esc = ‹ Back to New project (asks first when something changed)
        if super::escape(ui) {
            if self.dirty() {
                self.confirm_leave = true;
            } else {
                out = Out::Leave;
            }
        }
        let _ = t;
        out
    }

    fn step_summary(&self, i: usize) -> String {
        let d = &self.draft;
        match i {
            0 => self.change_name.clone(),
            1 => [d.length.as_str(), d.aspect.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · "),
            2 => {
                let w = d.look.words.trim();
                if w.is_empty() { format!("{} style frames", d.look.frames) } else { w.chars().take(28).collect() }
            }
            3 if d.narrated() => {
                if d.voice.name.is_empty() {
                    "Narrated".into()
                } else {
                    format!("Narrated · {}", d.voice.name)
                }
            }
            3 => "On-screen text only".into(),
            4 => d.theme.summary(),
            5 => format!("{} of {} run", d.steps.iter().filter(|s| s.runs).count(), d.steps.len()),
            6 => format!("{} skills", d.skills.iter().filter(|s| s.on).count() + d.skill_dirs.len()),
            _ => d.name.trim().to_string(),
        }
    }

    fn step_list(&mut self, ui: &mut egui::Ui, ctx: &mut MfxCtx) {
        let t = ctx.theme;
        ui.vertical(|ui| {
            ui.set_width(LIST_W);
            ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
            for (i, name) in STEPS.iter().enumerate() {
                let (rect, r) = ui.allocate_exact_size(vec2(LIST_W, 50.0), Sense::click());
                let here = i == self.step;
                let done = self.change || i < self.step;
                let p = ui.painter_at(rect);
                if here {
                    p.rect_filled(rect, 3.0, t.hover);
                    p.rect_filled(egui::Rect::from_min_size(rect.min, vec2(3.0, rect.height())), 0.0, t.tab_text_active);
                } else if r.hovered() {
                    p.rect_filled(rect, 3.0, t.field_bg);
                }
                let c = egui::pos2(rect.min.x + 22.0, rect.min.y + 17.0);
                if done {
                    p.circle_filled(c, 10.0, t.text_dim);
                    p.text(c, egui::Align2::CENTER_CENTER, (i + 1).to_string(), FontId::proportional(11.0), t.panel_bg);
                } else {
                    p.circle_stroke(c, 10.0, Stroke::new(1.0, t.text_faint));
                    p.text(c, egui::Align2::CENTER_CENTER, (i + 1).to_string(), FontId::proportional(11.0), t.text_dim);
                }
                p.text(
                    egui::pos2(rect.min.x + 42.0, rect.min.y + 17.0),
                    egui::Align2::LEFT_CENTER,
                    *name,
                    FontId::proportional(13.0),
                    if here { t.text } else { t.text_dim },
                );
                let sum = self.step_summary(i);
                let g = p.layout_no_wrap(sum, FontId::proportional(11.0), t.text_faint);
                p.with_clip_rect(rect.shrink2(vec2(4.0, 0.0))).galley(egui::pos2(rect.min.x + 42.0, rect.min.y + 29.0), g, t.text_faint);
                if r.clicked() {
                    self.step = i;
                }
                ctx.auto(&format!("mfx.newProject.creator.step.{}", i + 1), &r, name);
            }
        });
    }

    fn heading(ui: &mut egui::Ui, t: &crate::MfxTheme, title: &str, line: &str) {
        ui.label(RichText::new(title).color(t.text).size(17.0));
        ui.label(RichText::new(line).color(t.text_dim).size(12.5));
        ui.add_space(4.0);
    }

    // 1 Start from
    fn page_start(&mut self, ui: &mut egui::Ui, ctx: &mut MfxCtx, presets: &[Preset]) {
        let t = ctx.theme;
        Self::heading(ui, &t, "Start from", "Pick a preset to copy. The one you pick stays as it is.");
        for (group, built) in [("Built in", true), ("Yours", false)] {
            let rows: Vec<&Preset> = presets.iter().filter(|p| p.built_in() == built && !p.broken()).collect();
            if rows.is_empty() {
                continue;
            }
            ui.label(RichText::new(group).color(t.text_faint).size(11.5));
            for p in rows {
                let here = p.slug == self.draft.from;
                let frame = egui::Frame::new()
                    .fill(if here { t.hover } else { t.field_bg })
                    .stroke(Stroke::new(1.0, if here { t.accent } else { t.separator }))
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(egui::Margin::symmetric(12, 8));
                let inner = frame.show(ui, |ui| {
                    ui.set_width(CREATOR_W - LIST_W - 80.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&p.name).color(t.text).size(14.0));
                        if here {
                            ui.label(RichText::new("Starting from this").color(t.ok).size(11.5));
                        }
                    });
                    let facts = [p.default_of("length"), p.default_of("aspect"), p.default_of("voice"), model::short(&p.default_of("music"))]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · ");
                    ui.label(RichText::new(facts).color(t.text_dim).size(12.0));
                    ui.label(RichText::new(p.stage_line()).color(t.text_faint).size(11.5));
                });
                let r = ui.interact(inner.response.rect, egui::Id::new(("mfx-np-start", p.slug.as_str())), Sense::click());
                if r.clicked() && !here {
                    self.start_from(p);
                }
                ctx.auto(&format!("mfx.newProject.creator.start.{}", p.slug), &r, &p.name);
            }
        }
    }

    /// Copy another preset in (page 1). In change mode the name and what it replaces stay.
    fn start_from(&mut self, p: &Preset) {
        let keep = (self.draft.replace.clone(), self.draft.name.clone(), self.draft.description.clone(), self.draft.description_touched);
        self.draft = PresetDraft::from_preset(p, false);
        if self.change {
            (self.draft.replace, self.draft.name, self.draft.description, self.draft.description_touched) = keep;
        }
        self.choices_from(p);
    }

    // 2 Length and shape
    fn page_shape(&mut self, ui: &mut egui::Ui, ctx: &mut MfxCtx) {
        let t = ctx.theme;
        Self::heading(ui, &t, "Length and shape", "How long the video runs, and the shape of the screen it's made for.");
        ui.label(RichText::new("Length").color(t.text_dim).size(13.0));
        ui.horizontal_wrapped(|ui| {
            for l in self.lengths.clone() {
                let r = chip(ui, &t, !self.custom_length && self.draft.length == l, &l);
                if r.clicked() {
                    self.draft.length = l.clone();
                    self.custom_length = false;
                }
                ctx.auto(&format!("mfx.newProject.creator.length.{l}"), &r, &l);
            }
            let r = chip(ui, &t, self.custom_length, "Custom");
            if r.clicked() {
                self.custom_length = true;
            }
            ctx.auto("mfx.newProject.creator.length.custom", &r, "Custom");
            if self.custom_length {
                let e = ui.add(egui::TextEdit::singleline(&mut self.draft.length).hint_text("45 s").desired_width(120.0));
                ctx.auto("mfx.newProject.creator.length.text", &e, "Custom length");
            }
        });
        ui.add_space(8.0);
        ui.label(RichText::new("Shape").color(t.text_dim).size(13.0));
        ui.horizontal(|ui| {
            for a in self.aspects.clone() {
                let on = !self.custom_aspect && self.draft.aspect == a;
                let r = shape_card(ui, &t, &a, on);
                if r.clicked() {
                    self.draft.aspect = a.clone();
                    self.custom_aspect = false;
                }
                ctx.auto(&format!("mfx.newProject.creator.aspect.{a}"), &r, &a);
            }
            let r = shape_card(ui, &t, "Custom", self.custom_aspect);
            if r.clicked() {
                self.custom_aspect = true;
            }
            ctx.auto("mfx.newProject.creator.aspect.custom", &r, "Custom");
        });
        if self.custom_aspect {
            let e = ui.add(egui::TextEdit::singleline(&mut self.draft.aspect).hint_text("21:9").desired_width(120.0));
            ctx.auto("mfx.newProject.creator.aspect.text", &e, "Custom shape");
        }
    }

    // 3 Look
    fn page_look(&mut self, ui: &mut egui::Ui, ctx: &mut MfxCtx) {
        let t = ctx.theme;
        Self::heading(ui, &t, "Look", "How it should look, in your words and pictures. The AI makes style frames from it for you to pick.");
        let (dropped, hovering) = drops(ui, ctx);
        let picked = ctx.state.take_paths(PICK_PICTURES);
        let mut wrong = Vec::new();
        for p in dropped.into_iter().chain(picked) {
            let ext = std::path::Path::new(&p).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
            if !model::IMAGE_EXTS.contains(&ext.as_str()) {
                wrong.push(p);
            } else if !self.draft.look.pictures.contains(&p) && self.draft.look.pictures.len() < 40 {
                self.draft.look.pictures.push(p);
            }
        }
        if !wrong.is_empty() {
            self.pictures_error = "Only pictures (JPG, PNG, WebP, SVG)".into();
        }
        ui.label(RichText::new("In your words").color(t.text_dim).size(13.0));
        let r = ui.add(
            egui::TextEdit::multiline(&mut self.draft.look.words)
                .hint_text("Dry documentary: archive paper, warm greys, one red accent, slow pushes.")
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        ctx.auto("mfx.newProject.creator.look.words", &r, "In your words");
        ui.horizontal(|ui| {
            ui.label(RichText::new("Pictures to match").color(t.text_dim).size(13.0));
            let b = ui.button("Add pictures…");
            if b.clicked() {
                self.pictures_error.clear();
                pick(ctx, PICK_PICTURES, &model::IMAGE_EXTS, false);
            }
            ctx.auto("mfx.newProject.creator.look.add", &b, "Add pictures…");
            let hint = if hovering > 0 { format!("Drop to add {hovering}") } else { "or drop them here".into() };
            ui.label(RichText::new(hint).color(if hovering > 0 { t.text } else { t.text_faint }).size(12.0));
        });
        if !self.pictures_error.is_empty() {
            ui.label(RichText::new(&self.pictures_error).color(t.error).size(12.0));
        }
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            for (i, p) in self.draft.look.pictures.iter().enumerate() {
                ui.vertical(|ui| {
                    ui.set_width(112.0);
                    kind_glyph(ui, &t, Some(model::Kind::Image), vec2(112.0, 63.0));
                    let name = std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.clone());
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(RichText::new(&name).color(t.text_dim).size(11.0)).truncate()).on_hover_text(p);
                        let x = ui.add(egui::Button::new(RichText::new("×").size(16.0)).frame(false));
                        if x.clicked() {
                            remove = Some(i);
                        }
                        ctx.auto(&format!("mfx.newProject.creator.look.pic.{i}.remove"), &x, &format!("Remove {name}"));
                    });
                });
            }
        });
        if let Some(i) = remove
            && i < self.draft.look.pictures.len()
        {
            self.draft.look.pictures.remove(i);
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Style frames to choose from").color(t.text_dim).size(13.0));
            for n in [3u8, 4, 5] {
                let r = chip(ui, &t, self.draft.look.frames == n, &n.to_string());
                if r.clicked() {
                    self.draft.look.frames = n;
                }
                ctx.auto(&format!("mfx.newProject.creator.look.frames.{n}"), &r, &n.to_string());
            }
        });
    }

    // 4 Voice
    fn page_voice(&mut self, ui: &mut egui::Ui, ctx: &mut MfxCtx) {
        let t = ctx.theme;
        Self::heading(ui, &t, "Voice", "Who speaks the script, and how.");
        ui.horizontal(|ui| {
            ui.label(RichText::new("Narration").color(t.text_dim).size(13.0));
            for (i, n) in self.narrations.clone().iter().enumerate() {
                let r = chip(ui, &t, self.draft.narration == *n, n);
                if r.clicked() {
                    self.draft.narration = n.clone();
                }
                ctx.auto(&format!("mfx.newProject.creator.narration.{i}"), &r, n);
            }
        });
        if !self.draft.narrated() {
            ui.label(RichText::new("No voice: the story is told with on-screen text and music.").color(t.text_faint).size(12.0));
            return;
        }
        ui.label(RichText::new("Voice").color(t.text_dim).size(13.0));
        if !self.voice_set_up {
            let line = match &self.voices_load {
                Load::Loading(_) => "Loading voices…".to_string(),
                _ => "No narration voice set up yet".to_string(),
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(line).color(t.text_faint).size(12.5));
                let l = ui.link("Open Settings › Narration");
                if l.clicked() {
                    ctx.command("mortiflix.setup", json!({}));
                }
                ctx.auto("mfx.newProject.creator.setupNarration", &l, "Open Settings › Narration");
            });
        }
        for v in self.voices.clone() {
            ui.horizontal(|ui| {
                let r = ui.radio(self.draft.voice.name == v.id, RichText::new(&v.name).color(t.text).size(13.0));
                if r.clicked() {
                    self.draft.voice.name = v.id.clone();
                }
                ctx.auto(&format!("mfx.newProject.creator.voice.{}", if v.id.is_empty() { &v.name } else { &v.id }), &r, &v.name);
                ui.label(RichText::new(&v.desc).color(t.text_faint).size(12.0));
                let l = ui.add_enabled(false, egui::Button::new("▶ Listen")).on_disabled_hover_text("Listening comes with the sound engine");
                ctx.auto(&format!("mfx.newProject.creator.voice.{}.listen", v.id), &l, "Listen");
            });
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("How it's spoken").color(t.text_dim).size(13.0));
            let r = ui.add(egui::TextEdit::singleline(&mut self.draft.voice.delivery).hint_text("Calm, a bit dry. Not upbeat.").desired_width(480.0));
            ctx.auto("mfx.newProject.creator.delivery", &r, "How it's spoken");
        });
    }

    // 5 Music: returns true when "Choose…" opens the Music theme page
    fn page_music(&mut self, ui: &mut egui::Ui, ctx: &mut MfxCtx) -> bool {
        let t = ctx.theme;
        Self::heading(ui, &t, "Music", "Who makes the music, and its theme. Every project made with this preset starts from it.");
        if !self.music_choices.is_empty() {
            ui.label(RichText::new("Who makes it").color(t.text_dim).size(13.0));
            for (i, c) in self.music_choices.clone().iter().enumerate() {
                let r = ui.radio(self.draft.music_choice == *c, RichText::new(c).size(12.5));
                if r.clicked() {
                    self.draft.music_choice = c.clone();
                    self.draft.theme.music = !c.to_lowercase().starts_with("no music");
                }
                ctx.auto(&format!("mfx.newProject.creator.music.{i}"), &r, c);
            }
        }
        ui.add_space(6.0);
        let mut open = false;
        egui::Frame::new()
            .fill(t.field_bg)
            .stroke(Stroke::new(1.0, t.separator))
            .corner_radius(CornerRadius::same(4))
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(CREATOR_W - LIST_W - 80.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Music theme").color(t.text_dim).size(13.0));
                    ui.label(RichText::new(self.draft.theme.summary()).color(t.text).size(13.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let r = ui.button("Choose…");
                        open = r.clicked();
                        ctx.auto("mfx.newProject.creator.music.choose", &r, "Choose…");
                    });
                });
            });
        open
    }

    // 6 Stages
    fn page_stages(&mut self, ui: &mut egui::Ui, ctx: &mut MfxCtx) {
        let t = ctx.theme;
        Self::heading(
            ui,
            &t,
            "Stages",
            "Which steps run, and where the AI waits for you. Off: the AI still decides that part, you just don't get a stop for it.",
        );
        ui.horizontal(|ui| {
            ui.add_space(CREATOR_W - LIST_W - 260.0);
            ui.label(RichText::new("Runs").color(t.text_faint).size(11.5));
            ui.add_space(40.0);
            ui.label(RichText::new("Waits for you").color(t.text_faint).size(11.5));
        });
        for i in 0..self.draft.steps.len() {
            let Some(s) = self.draft.steps.get(i).cloned() else { continue };
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(CREATOR_W - LIST_W - 280.0);
                    ui.label(RichText::new(&s.name).color(t.text).size(13.0));
                    if !s.describe.is_empty() {
                        ui.add(egui::Label::new(RichText::new(&s.describe).color(t.text_faint).size(11.5)).truncate()).on_hover_text(&s.describe);
                    }
                });
                if s.locked {
                    ui.add_sized([70.0, 20.0], egui::Label::new(RichText::new("Always").color(t.text_dim).size(12.0)));
                } else {
                    let mut runs = s.runs;
                    let r = ui.add_sized([70.0, 20.0], egui::Checkbox::new(&mut runs, ""));
                    if r.changed() {
                        self.draft.set_runs(i, runs);
                    }
                    ctx.auto(&format!("mfx.newProject.creator.stage.{}.runs", s.key), &r, &format!("{} runs", s.name));
                }
                let mut waits = s.waits;
                let r = ui.add_enabled(s.runs, egui::Checkbox::new(&mut waits, ""));
                if r.changed()
                    && let Some(row) = self.draft.steps.get_mut(i)
                {
                    row.waits = waits;
                }
                ctx.auto(&format!("mfx.newProject.creator.stage.{}.waits", s.key), &r, &format!("{} waits for you", s.name));
            });
        }
    }

    // 7 Skills
    fn page_skills(&mut self, ui: &mut egui::Ui, ctx: &mut MfxCtx) {
        let t = ctx.theme;
        Self::heading(ui, &t, "Skills", "What the AI knows how to do in this preset. A skill a running stage needs stays on.");
        let (dropped, _) = drops(ui, ctx);
        let picked = ctx.state.take_paths(PICK_SKILL_DIR);
        for d in dropped.into_iter().chain(picked) {
            if !model::is_skill_dir(&d) {
                self.skills_error = "That folder has no SKILL.md".into();
            } else if !self.draft.skill_dirs.contains(&d) && self.draft.skill_dirs.len() < 40 {
                self.skills_error.clear();
                self.draft.skill_dirs.push(d);
            }
        }
        let steps = self.draft.steps.clone();
        for (i, row) in self.draft.skills.iter_mut().enumerate() {
            let needed = model::skill_needed_by(&row.name, &steps);
            ui.horizontal(|ui| {
                let locked = !needed.is_empty();
                if locked {
                    row.on = true;
                }
                let r = ui.add_enabled(!locked, egui::Checkbox::new(&mut row.on, RichText::new(&row.name).color(t.text).size(13.0)));
                ctx.auto(&format!("mfx.newProject.creator.skill.{}", row.name), &r, &row.name);
                ui.label(RichText::new(skill_line(&row.name)).color(t.text_faint).size(12.0));
                if locked {
                    ui.label(RichText::new(format!("Needed by {}", needed.join(", "))).color(t.text_dim).size(11.5));
                }
            });
            let _ = i;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let b = ui.button("Add a skill folder…");
            if b.clicked() {
                self.skills_error.clear();
                pick(ctx, PICK_SKILL_DIR, &[], true);
            }
            ctx.auto("mfx.newProject.creator.skills.add", &b, "Add a skill folder…");
            ui.label(RichText::new("A folder with a SKILL.md is copied into the preset.").color(t.text_faint).size(11.5));
        });
        if !self.skills_error.is_empty() {
            ui.label(RichText::new(&self.skills_error).color(t.error).size(12.0));
        }
        let mut remove = None;
        for (i, d) in self.draft.skill_dirs.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("✓ {}", std::path::Path::new(d).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()))
                        .color(t.text)
                        .size(13.0),
                );
                ui.label(RichText::new(d).color(t.text_faint).size(11.0));
                let x = ui.add(egui::Button::new(RichText::new("×").size(16.0)).frame(false));
                if x.clicked() {
                    remove = Some(i);
                }
                ctx.auto(&format!("mfx.newProject.creator.skilldir.{i}.remove"), &x, "Remove");
            });
        }
        if let Some(i) = remove
            && i < self.draft.skill_dirs.len()
        {
            self.draft.skill_dirs.remove(i);
        }
    }

    // 8 Name and save
    fn page_save(&mut self, ui: &mut egui::Ui, ctx: &mut MfxCtx, presets: &[Preset]) {
        let t = ctx.theme;
        Self::heading(ui, &t, "Name and save", "Give it a name. Check what you picked; Change jumps back to that page.");
        if self.change && self.draft.from_built_in {
            ui.label(RichText::new(format!("{} is built in, so this saves as your own preset.", self.change_name)).color(t.warn).size(12.5));
        }
        ui.horizontal(|ui| {
            ui.add_sized([96.0, 20.0], egui::Label::new(RichText::new("Name").color(t.text_dim).size(13.0)));
            let r = ui.add(egui::TextEdit::singleline(&mut self.draft.name).hint_text("My explainer video").desired_width(480.0));
            if r.changed() {
                self.error.clear();
            }
            ctx.auto("mfx.newProject.creator.name", &r, "Name");
        });
        if !self.draft.description_touched {
            self.draft.description = self.draft.auto_description();
        }
        ui.horizontal_top(|ui| {
            ui.add_sized([96.0, 20.0], egui::Label::new(RichText::new("Description").color(t.text_dim).size(13.0)));
            let r = ui.add(egui::TextEdit::multiline(&mut self.draft.description).desired_rows(3).desired_width(CREATOR_W - LIST_W - 160.0));
            if r.changed() {
                self.draft.description_touched = true;
            }
            ctx.auto("mfx.newProject.creator.description", &r, "Description");
        });
        ui.add_space(6.0);
        egui::Grid::new("mfx-np-creator-summary").num_columns(3).spacing(vec2(16.0, 6.0)).show(ui, |ui| {
            for i in 0..7 {
                ui.label(RichText::new(STEPS.get(i).copied().unwrap_or_default()).color(t.text_dim).size(12.5));
                ui.add_sized([CREATOR_W - LIST_W - 340.0, 18.0], egui::Label::new(RichText::new(self.step_summary(i)).color(t.text).size(12.5)).truncate());
                let r = ui.link("Change");
                if r.clicked() {
                    self.step = i;
                }
                ctx.auto(&format!("mfx.newProject.creator.change.{}", i + 1), &r, "Change");
                ui.end_row();
            }
        });
        if self.change && !self.draft.from_built_in && !self.draft.replace.is_empty() {
            ui.add_space(10.0);
            if self.confirm_delete {
                egui::Frame::new()
                    .fill(t.field_bg)
                    .stroke(Stroke::new(1.0, t.error))
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("Delete \"{}\"? Projects made with it keep working; you can't pick it for new ones.", self.change_name))
                                .color(t.text)
                                .size(12.5),
                        );
                        ui.horizontal(|ui| {
                            let c = quiet_button(ui, &t, "Cancel");
                            if c.clicked() {
                                self.confirm_delete = false;
                            }
                            ctx.auto("mfx.newProject.creator.delete.cancel", &c, "Cancel");
                            let d = ui.add(egui::Button::new(RichText::new("Delete").color(Color32::WHITE)).fill(t.error).min_size(vec2(80.0, 28.0)));
                            if d.clicked() {
                                self.confirm_delete = false;
                                self.delete_req = Some(ctx.service.request("presets.delete", json!({"slug": self.draft.replace})));
                            }
                            ctx.auto("mfx.newProject.creator.delete.confirm", &d, "Delete");
                        });
                    });
            } else {
                let d = ui.add(egui::Button::new(RichText::new("Delete preset…").color(t.error).size(12.5)).frame(false));
                if d.clicked() {
                    self.confirm_delete = true;
                }
                ctx.auto("mfx.newProject.creator.delete", &d, "Delete preset…");
            }
        }
        let _ = presets;
    }

    fn footer(&mut self, ui: &mut egui::Ui, ctx: &mut MfxCtx, presets: &[Preset]) -> Option<Out> {
        let t = ctx.theme;
        let mut out = None;
        if self.confirm_leave {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Leave without saving? Your changes to this preset are lost.").color(t.warn).size(12.5));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let l = quiet_button(ui, &t, "Leave");
                    if l.clicked() {
                        out = Some(Out::Leave);
                    }
                    ctx.auto("mfx.newProject.creator.leave.confirm", &l, "Leave");
                    let k = accent_button(ui, &t, "Keep editing", true);
                    if k.clicked() {
                        self.confirm_leave = false;
                    }
                    ctx.auto("mfx.newProject.creator.leave.keep", &k, "Keep editing");
                });
            });
            return out;
        }
        let busy = self.save_req.is_some() || self.delete_req.is_some();
        ui.horizontal(|ui| {
            ui.set_width(CREATOR_W);
            let back = ui.link("‹ Back to New project");
            if back.clicked() {
                if self.dirty() {
                    self.confirm_leave = true;
                } else {
                    out = Some(Out::Leave);
                }
            }
            ctx.auto("mfx.newProject.creator.leave", &back, "‹ Back to New project");
            if !self.error.is_empty() {
                ui.label(RichText::new(&self.error).color(t.error).size(12.5));
            } else if busy {
                ui.label(RichText::new("Saving…").color(t.text_dim).size(12.5));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let last = self.step + 1 >= STEPS.len();
                let label = if !last {
                    "Next"
                } else if self.change && !self.draft.replace.is_empty() {
                    "Save changes"
                } else {
                    "Save preset"
                };
                let n = accent_button(ui, &t, label, !busy);
                if n.clicked() {
                    if last {
                        self.save(ctx, presets);
                    } else {
                        self.step += 1;
                    }
                }
                ctx.auto("mfx.newProject.creator.next", &n, label);
                if self.step > 0 {
                    let b = quiet_button(ui, &t, "Back");
                    if b.clicked() {
                        self.step = self.step.saturating_sub(1);
                    }
                    ctx.auto("mfx.newProject.creator.back", &b, "Back");
                }
                ui.label(RichText::new(format!("Step {} of {}", self.step + 1, STEPS.len())).color(t.text_faint).size(12.0));
            });
        });
        out
    }

    fn save(&mut self, ctx: &mut MfxCtx, presets: &[Preset]) {
        if let Some(problem) = self.draft.problem(presets) {
            self.error = problem;
            self.step = 7;
            return;
        }
        self.error.clear();
        if !self.draft.theme.music
            && let Some(no) = self.music_choices.iter().find(|c| model::is_no_music(c))
        {
            self.draft.music_choice = no.clone();
        }
        self.save_req = Some(ctx.service.request("presets.save", self.draft.save_params()));
    }
}

fn err_text(e: &Value) -> String {
    e.get("message").and_then(Value::as_str).unwrap_or("the service didn't answer").chars().take(300).collect()
}

/// One plain line per shared skill.
fn skill_line(name: &str) -> &'static str {
    match name {
        "motion-design" => "Composition, timing and easing for motion design.",
        "remotion-motion" => "Builds the shots in Remotion.",
        "transition-board" => "Plans how each frame becomes the next.",
        "voiceover" => "Writes and times the narration.",
        "music" => "Writes the original score to the cuts.",
        "final-pass" => "Checks the finished video before it's handed over.",
        _ => "A skill from the start preset.",
    }
}

/// A shape drawn to scale (page 2): a 64 × 64 box with the frame inside, the label under it.
fn shape_card(ui: &mut egui::Ui, t: &crate::MfxTheme, label: &str, on: bool) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(vec2(86.0, 96.0), Sense::click());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, if on { t.hover } else { t.field_bg });
    p.rect_stroke(rect, 4.0, Stroke::new(1.0, if on { t.accent } else { t.separator }), StrokeKind::Inside);
    let (w, h) = label
        .split_once(':')
        .and_then(|(a, b)| Some((a.trim().parse::<f32>().ok()?, b.trim().parse::<f32>().ok()?)))
        .filter(|(a, b)| *a > 0.0 && *b > 0.0 && a.is_finite() && b.is_finite())
        .unwrap_or((1.0, 1.0));
    let k = 56.0 / w.max(h);
    let frame = egui::Rect::from_center_size(egui::pos2(rect.center().x, rect.min.y + 38.0), vec2((w * k).max(4.0), (h * k).max(4.0)));
    if label == "Custom" {
        p.rect_stroke(frame.shrink(8.0), 2.0, Stroke::new(1.0, t.text_faint), StrokeKind::Inside);
        p.text(frame.center(), egui::Align2::CENTER_CENTER, "?", FontId::proportional(16.0), t.text_faint);
    } else {
        p.rect_stroke(frame, 2.0, Stroke::new(1.5, if on { t.text } else { t.text_dim }), StrokeKind::Inside);
    }
    let text = if on { format!("✓ {label}") } else { label.to_string() };
    p.text(egui::pos2(rect.center().x, rect.max.y - 12.0), egui::Align2::CENTER_CENTER, text, FontId::proportional(12.0), if on { t.text } else { t.text_dim });
    r
}

#[cfg(test)]
mod tests {
    use super::model::SkillRow;
    use super::*;
    use crate::{MfxState, MfxTheme, Service};

    fn preset(slug: &str, source: &str) -> Preset {
        model::parse_presets(&json!([{"slug": slug, "name": slug, "source": source,
            "intake": [{"id": "length", "type": "choice", "choices": ["60 s"], "default": "60 s"}, {"id": "aspect", "type": "choice", "choices": ["16:9"], "default": "16:9"}],
            "steps": [{"key": "brief", "name": "Brief"}, {"key": "final", "name": "Final"}], "shared_skills": ["final-pass"]}]))
        .remove(0)
    }

    #[test]
    fn save_checks_then_asks_the_service() {
        let mut state = MfxState::default();
        let mut service = Service::default();
        let mut ctx = MfxCtx::new(&mut state, &mut service, MfxTheme::default());
        let p = preset("explainer", "built-in");
        let mut c = Creator::new(&p, false);
        c.step = 2;
        c.save(&mut ctx, std::slice::from_ref(&p));
        assert_eq!(c.error, "Give the preset a name.");
        assert_eq!(c.step, 7, "the name problem jumps to page 8");
        c.draft.name = "Dry docs".into();
        c.save(&mut ctx, std::slice::from_ref(&p));
        assert!(c.error.is_empty());
        assert!(c.save_req.is_some());
        // no service: the error comes back on page 8, never a wait forever
        c.poll(&mut ctx);
        assert!(c.save_req.is_none());
        assert!(!c.error.is_empty());
    }

    #[test]
    fn leaving_asks_only_after_a_change_and_start_from_keeps_change_mode() {
        let mine = preset("dry", "studio");
        let other = preset("explainer", "built-in");
        let mut c = Creator::new(&mine, true);
        assert!(!c.dirty());
        c.start_from(&other);
        assert_eq!((c.draft.from.as_str(), c.draft.replace.as_str(), c.draft.name.as_str()), ("explainer", "dry", "dry"));
        assert!(c.dirty());
        assert_eq!(c.draft.skills, vec![SkillRow { name: "final-pass".into(), on: true }]);
    }

    #[test]
    fn shape_cards_survive_hostile_labels() {
        let ectx = egui::Context::default();
        let t = MfxTheme::default();
        ectx.run_ui(egui::RawInput::default(), |ui| {
            for l in ["16:9", "0:0", "-1:5", "inf:1", "1e30:1", "abc", ":", "Custom"] {
                let _ = shape_card(ui, &t, l, true);
            }
        })
        .textures_delta
        .clear();
    }
}

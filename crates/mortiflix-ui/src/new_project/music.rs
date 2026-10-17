//! The Music theme page (doc 33a-2): swapped into the New project window (or the preset creator's
//! Music step) in place of the form; Back / Done return. Who picks, mood (up to two), genre, tempo,
//! instruments, "like this song", three 15 s samples to hear first (`music.theme.samples`, never made on
//! its own: it's an AI call), and More (leave out, exact tempo, key, who makes it, music on/off).

use egui::{Align, Key, Layout, RichText, vec2};
use serde_json::{Value, json};

use super::model::{self, Asset, INSTRUMENTS, MOODS, MusicTheme, Reference, Sample, TempoFeel};
use super::{accent_button, chip, drops, pick, quiet_button, row_label};
use crate::MfxCtx;

const W: f32 = super::MAIN_W;

/// The page's own state (per place it's opened from).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MusicUi {
    pub more_open: bool,
    /// The "+ Other" instrument being typed.
    pub other: String,
    /// The "leave out" instrument being typed.
    pub avoid: String,
    pub samples_req: Option<u64>,
    pub samples_error: String,
    /// The service's word when it makes them later (no project yet, a real backend).
    pub samples_note: String,
}

/// Read the `music.theme.samples` reply into the theme. The one in use stays as a fourth row until you
/// pick again (doc 33).
pub fn poll(ctx: &mut MfxCtx, mu: &mut MusicUi, theme: &mut MusicTheme) {
    let Some(rid) = mu.samples_req else { return };
    let Some(r) = ctx.service.take_reply(rid) else { return };
    mu.samples_req = None;
    if !r.ok {
        let msg = r.error.get("message").and_then(Value::as_str).unwrap_or("the service didn't answer");
        mu.samples_error = format!("The samples couldn't be made: {}", msg.chars().take(200).collect::<String>());
        return;
    }
    let mut fresh = model::parse_samples(&r.result);
    for (i, s) in fresh.iter_mut().enumerate() {
        if s.id.is_empty() {
            s.id = format!("s{}", i + 1);
        }
    }
    if let Some(used) = theme.used_sample().cloned()
        && !fresh.iter().any(|s| s.id == used.id)
    {
        fresh.push(used);
    }
    mu.samples_error.clear();
    mu.samples_note = if fresh.is_empty() {
        r.result.get("note").and_then(Value::as_str).map_or("The samples come later, as a card in the chat.".to_string(), |n| n.chars().take(300).collect())
    } else {
        String::new()
    };
    if !fresh.is_empty() || theme.samples.is_empty() {
        theme.samples = fresh;
    }
}

/// Draw the page. Returns true when Back or Done was pressed (choices are kept either way).
pub fn page(
    ui: &mut egui::Ui,
    ctx: &mut MfxCtx,
    mu: &mut MusicUi,
    theme: &mut MusicTheme,
    assets: &[Asset],
    choices: &[String],
    choice: &mut String,
    pick_key: &str,
) -> bool {
    let t = ctx.theme;
    ui.set_width(W);
    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
    // a song dropped anywhere on the window, or chosen with Browse…, becomes the reference
    let (dropped, _) = drops(ui, ctx);
    let picked = ctx.state.take_paths(pick_key);
    if let Some(path) = dropped.into_iter().chain(picked).next() {
        let a = model::inspect(&path);
        if a.error.is_empty() && a.kind.is_audio() {
            theme.reference = Some(reference_of(&a));
            mu.samples_error.clear();
        } else {
            mu.samples_error = format!("Can't use {} as a song: try a WAV, FLAC, MP3 or OGG.", a.name);
        }
    }

    // Who picks
    ui.horizontal(|ui| {
        row_label(ui, &t, "Who picks", None);
        for (label, key, id) in [("Let the AI pick", "ai", "mfx.newProject.music.ai"), ("I'll choose", "you", "mfx.newProject.music.you")] {
            let on = (key == "ai") == theme.ai_picks();
            let r = chip(ui, &t, on, label);
            if r.clicked() {
                theme.picked_by = key.into();
            }
            ctx.auto(id, &r, label);
        }
    });
    if theme.ai_picks() {
        ui.horizontal(|ui| {
            ui.add_space(super::LABEL_W + 8.0);
            ui.label(
                RichText::new("The AI's pick: it reads the preset and your idea, then picks the mood, the instruments and a tempo fitted to the cuts.")
                    .color(t.text_dim)
                    .size(12.0),
            );
        });
    } else {
        ui.horizontal(|ui| {
            ui.add_space(super::LABEL_W + 8.0);
            ui.label(RichText::new("Anything you leave empty, the AI picks.").color(t.text_faint).size(11.5));
        });
        choose_fields(ui, ctx, mu, theme, assets, pick_key);
    }

    // Hear it first
    ui.separator();
    ui.horizontal(|ui| {
        row_label(ui, &t, "Hear it first", None);
        ui.label(RichText::new("3 samples, 15 s each").color(t.text_dim).size(12.5));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = if theme.samples.is_empty() { "Make 3 samples" } else { "Make 3 new samples" };
            let busy = mu.samples_req.is_some();
            let r = ui.add_enabled(!busy, egui::Button::new(label));
            if r.clicked() {
                mu.samples_error.clear();
                mu.samples_note.clear();
                mu.samples_req = Some(ctx.service.request("music.theme.samples", json!({"theme": theme})));
            }
            ctx.auto("mfx.newProject.music.samples.make", &r, label);
        });
    });
    if mu.samples_req.is_some() {
        for i in 1..=3 {
            sample_line(ui, &t, &format!("Sample {i}"), "Making… about 40 s", false);
        }
    } else if theme.samples.is_empty() && mu.samples_note.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(super::LABEL_W + 8.0);
            ui.label(RichText::new("Optional. If you don't make them here, the AI makes three during the Look stage.").color(t.text_faint).size(11.5));
        });
    }
    let mut use_id = None;
    for (i, s) in theme.samples.iter().enumerate() {
        let using = s.id == theme.using;
        if let Some(r) = sample_row(ui, ctx, i, s, using) {
            use_id = Some(r);
        }
    }
    if let Some(id) = use_id {
        theme.using = id;
    }
    if !theme.samples.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(super::LABEL_W + 8.0);
            ui.label(RichText::new("The one you use becomes the music's main idea.").color(t.text_faint).size(11.5));
        });
    }
    if !mu.samples_note.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(super::LABEL_W + 8.0);
            ui.label(RichText::new(&mu.samples_note).color(t.text_dim).size(12.0));
        });
    }
    if !mu.samples_error.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(super::LABEL_W + 8.0);
            ui.label(RichText::new(&mu.samples_error).color(t.error).size(12.0));
        });
    }

    // More (folded)
    ui.separator();
    more(ui, ctx, mu, theme, choices, choice);

    // footer
    ui.separator();
    let mut back = false;
    ui.horizontal(|ui| {
        ui.set_width(W);
        ui.label(RichText::new("Esc = Back · Ctrl+Enter = Done").color(t.text_faint).size(12.0));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let d = accent_button(ui, &t, "Done", true);
            back |= d.clicked();
            ctx.auto("mfx.newProject.music.done", &d, "Done");
            let b = quiet_button(ui, &t, "Back");
            back |= b.clicked();
            ctx.auto("mfx.newProject.music.back", &b, "Back");
        });
    });
    if super::escape(ui) || ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::Enter)) {
        back = true;
    }
    back
}

fn reference_of(a: &Asset) -> Reference {
    let length = a.facts.rsplit(" · ").next().filter(|s| s.contains(':')).unwrap_or_default().to_string();
    Reference { path: a.path.clone(), name: a.name.clone(), length }
}

fn choose_fields(ui: &mut egui::Ui, ctx: &mut MfxCtx, mu: &mut MusicUi, theme: &mut MusicTheme, assets: &[Asset], pick_key: &str) {
    let t = ctx.theme;
    ui.horizontal_wrapped(|ui| {
        row_label(ui, &t, "Mood", None);
        for m in MOODS {
            let on = theme.mood.iter().any(|x| x == m);
            let r = chip(ui, &t, on, m);
            if r.clicked() {
                theme.toggle_mood(m);
            }
            ctx.auto(&format!("mfx.newProject.music.mood.{m}"), &r, m);
        }
        ui.label(RichText::new("up to 2").color(t.text_faint).size(11.0));
    });
    ui.horizontal(|ui| {
        row_label(ui, &t, "Genre", None);
        let r = ui.add(
            egui::TextEdit::singleline(&mut theme.genre).hint_text("Ambient documentary, lo-fi, small orchestra…").desired_width(W - super::LABEL_W - 16.0),
        );
        ctx.auto("mfx.newProject.music.genre", &r, "Genre");
    });
    ui.horizontal(|ui| {
        row_label(ui, &t, "Tempo", None);
        for f in TempoFeel::ALL {
            let r = chip(ui, &t, theme.tempo.feel == f, f.label());
            if r.clicked() {
                theme.set_tempo(f);
            }
            ctx.auto(&format!("mfx.newProject.music.tempo.{}", f.label().to_lowercase()), &r, f.label());
        }
        let line = match theme.tempo.feel.bpm() {
            Some((lo, hi)) => format!("About {lo}–{hi} BPM. Fitted to the cuts."),
            None => "Fitted to the cuts.".into(),
        };
        ui.label(RichText::new(line).color(t.text_faint).size(11.5));
    });
    ui.horizontal_wrapped(|ui| {
        row_label(ui, &t, "Instruments", None);
        let mut names: Vec<String> = INSTRUMENTS.iter().map(|s| (*s).to_string()).collect();
        for own in &theme.instruments.use_ {
            if !names.iter().any(|n| n.eq_ignore_ascii_case(own)) {
                names.push(own.clone());
            }
        }
        for n in names {
            let on = theme.instruments.use_.iter().any(|x| x.eq_ignore_ascii_case(&n));
            let r = chip(ui, &t, on, &n);
            if r.clicked() {
                theme.toggle_instrument(&n);
            }
            ctx.auto(&format!("mfx.newProject.music.instrument.{n}"), &r, &n);
        }
        let o = ui.add(egui::TextEdit::singleline(&mut mu.other).hint_text("+ Other").desired_width(110.0));
        ctx.auto("mfx.newProject.music.other", &o, "+ Other");
        if o.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !mu.other.trim().is_empty() {
            theme.toggle_instrument(&mu.other);
            mu.other.clear();
        }
    });
    ui.horizontal_top(|ui| {
        row_label(ui, &t, "Like this song", None);
        ui.vertical(|ui| {
            match theme.reference.clone() {
                Some(r) => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&r.name).color(t.text).size(13.0));
                        if !r.length.is_empty() {
                            ui.label(RichText::new(&r.length).color(t.text_faint).size(12.0));
                        }
                        let x = ui.add(egui::Button::new(RichText::new("×").size(16.0)).frame(false)).on_hover_text("Remove");
                        if x.clicked() {
                            theme.reference = None;
                        }
                        ctx.auto("mfx.newProject.music.ref.remove", &x, "Remove the song");
                    });
                }
                None => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Drop a song here, or").color(t.text_dim).size(12.5));
                        let b = ui.add(egui::Button::new("Browse…"));
                        if b.clicked() {
                            pick(ctx, pick_key, &model::audio_exts(), false);
                        }
                        ctx.auto("mfx.newProject.music.ref.browse", &b, "Browse…");
                        let audio: Vec<&Asset> = assets.iter().filter(|a| a.error.is_empty() && a.kind.is_audio()).collect();
                        if !audio.is_empty() {
                            let mut chosen = None;
                            let c = egui::ComboBox::from_id_salt("mfx-np-music-ref").selected_text("From your assets").width(200.0).show_ui(ui, |ui| {
                                for (i, a) in audio.iter().enumerate() {
                                    let r = ui.selectable_label(false, &a.name);
                                    if r.clicked() {
                                        chosen = Some((*a).clone());
                                    }
                                    ctx.auto(&format!("mfx.newProject.music.ref.asset.{i}"), &r, &a.name);
                                }
                            });
                            ctx.auto("mfx.newProject.music.ref.asset", &c.response, "From your assets");
                            if let Some(a) = chosen {
                                theme.reference = Some(reference_of(&a));
                            }
                        }
                    });
                }
            }
            ui.label(RichText::new("Studied for its feel, tempo and instruments, never copied.").color(t.text_faint).size(11.5));
        });
    });
}

fn sample_line(ui: &mut egui::Ui, t: &crate::MfxTheme, name: &str, line: &str, _using: bool) {
    ui.horizontal(|ui| {
        ui.add_space(super::LABEL_W + 8.0);
        ui.label(RichText::new(name).color(t.text_dim).size(12.5));
        ui.label(RichText::new(line).color(t.text_faint).size(12.0));
    });
}

/// One sample row: the radio says "Using this" in words, never colour alone. Returns the id when
/// it was picked.
fn sample_row(ui: &mut egui::Ui, ctx: &mut MfxCtx, i: usize, s: &Sample, using: bool) -> Option<String> {
    let t = ctx.theme;
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.add_space(super::LABEL_W + 8.0);
        if !s.error.is_empty() {
            ui.label(RichText::new(format!("Sample {} couldn't be made: {}", i + 1, s.error)).color(t.error).size(12.0));
            return;
        }
        let play = ui.add_enabled(!s.wav.is_empty(), egui::Button::new("▶")).on_disabled_hover_text("Playing samples comes with the sound engine");
        ctx.auto(&format!("mfx.newProject.music.sample.{i}.play"), &play, "Play");
        let r = ui.radio(using, RichText::new(&s.name).color(t.text).size(13.0));
        if r.clicked() {
            picked = Some(s.id.clone());
        }
        ctx.auto(&format!("mfx.newProject.music.sample.{i}"), &r, &s.name);
        let mut facts = s.desc.clone();
        if let Some(b) = s.bpm {
            facts = if facts.is_empty() { format!("{b} BPM") } else { format!("{facts} · {b} BPM") };
        }
        ui.label(RichText::new(facts).color(t.text_faint).size(12.0));
        if using {
            ui.label(RichText::new("Using this").color(t.ok).size(12.0));
        }
    });
    picked
}

fn more(ui: &mut egui::Ui, ctx: &mut MfxCtx, mu: &mut MusicUi, theme: &mut MusicTheme, finish: &[String], choice: &mut String) {
    let t = ctx.theme;
    let summary = {
        let leave = if theme.instruments.avoid.is_empty() { "none".to_string() } else { theme.instruments.avoid.join(", ") };
        let tempo = theme.tempo.bpm_fixed.map_or("fitted".to_string(), |b| format!("{b} BPM"));
        let key = if theme.key.trim().is_empty() { "any".to_string() } else { theme.key.trim().to_string() };
        let on = if theme.music { String::new() } else { " · Music off".to_string() };
        format!("Leave out: {leave} · Exact tempo: {tempo} · Key: {key}{on}")
    };
    ui.horizontal(|ui| {
        let r = super::fold(ui, &t, mu.more_open, "More");
        if r.clicked() {
            mu.more_open = !mu.more_open;
        }
        ctx.auto("mfx.newProject.music.more", &r, "More");
        ui.label(RichText::new(summary).color(t.text_faint).size(12.0));
    });
    if !mu.more_open {
        return;
    }
    ui.horizontal(|ui| {
        row_label(ui, &t, "Music", None);
        let r = ui.checkbox(&mut theme.music, "Music in this video");
        if r.changed() {
            let pick = if theme.music { finish.iter().find(|c| !model::is_no_music(c)) } else { finish.iter().find(|c| model::is_no_music(c)) };
            if let Some(c) = pick {
                *choice = c.clone();
            }
        }
        ctx.auto("mfx.newProject.music.on", &r, "Music in this video");
    });
    ui.add_enabled_ui(theme.music, |ui| {
        ui.horizontal_wrapped(|ui| {
            row_label(ui, &t, "Leave out", None);
            let mut drop = None;
            for (i, n) in theme.instruments.avoid.iter().enumerate() {
                let r = chip(ui, &t, true, n).on_hover_text("Click to remove");
                if r.clicked() {
                    drop = Some(i);
                }
                ctx.auto(&format!("mfx.newProject.music.avoid.{i}"), &r, n);
            }
            if let Some(i) = drop
                && i < theme.instruments.avoid.len()
            {
                theme.instruments.avoid.remove(i);
            }
            let a = ui.add(egui::TextEdit::singleline(&mut mu.avoid).hint_text("An instrument to leave out").desired_width(200.0));
            ctx.auto("mfx.newProject.music.avoid", &a, "Leave out");
            if a.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !mu.avoid.trim().is_empty() && theme.instruments.avoid.len() < 20 {
                theme.instruments.avoid.push(mu.avoid.trim().chars().take(40).collect());
                mu.avoid.clear();
            }
        });
        ui.horizontal(|ui| {
            row_label(ui, &t, "Exact tempo", None);
            let mut fixed = theme.tempo.bpm_fixed.is_some();
            let r = ui.checkbox(&mut fixed, "Fixed BPM");
            ctx.auto("mfx.newProject.music.bpm.fixed", &r, "Fixed BPM");
            if fixed {
                let mut bpm = theme.tempo.bpm_fixed.unwrap_or(96);
                let d = ui.add(egui::DragValue::new(&mut bpm).range(40..=220).suffix(" BPM"));
                ctx.auto("mfx.newProject.music.bpm", &d, "BPM");
                theme.tempo.bpm_fixed = Some(bpm.clamp(40, 220));
            } else {
                theme.tempo.bpm_fixed = None;
                ui.label(RichText::new("Fitted to the cuts").color(t.text_faint).size(11.5));
            }
        });
        ui.horizontal(|ui| {
            row_label(ui, &t, "Key", None);
            let r = ui.add(egui::TextEdit::singleline(&mut theme.key).hint_text("Any (D minor, C major…)").desired_width(200.0));
            ctx.auto("mfx.newProject.music.key", &r, "Key");
        });
        if !finish.is_empty() {
            ui.horizontal_top(|ui| {
                row_label(ui, &t, "Who makes it", None);
                ui.vertical(|ui| {
                    for (i, c) in finish.iter().enumerate() {
                        if c.to_lowercase().starts_with("no music") {
                            continue;
                        }
                        let r = ui.radio(*choice == *c, RichText::new(c).size(12.5));
                        if r.clicked() {
                            *choice = c.clone();
                            theme.finish = model::finish_of(c).into();
                        }
                        ctx.auto(&format!("mfx.newProject.music.finish.{i}"), &r, c);
                    }
                });
            });
        }
    });
}

//! The stage documents the service writes (brief.md, genre.md, songmap.md…), drawn readably: `#`/`##`
//! headings, paragraphs, `- ` lists, `**bold**`, inline `code` in monospace, tables as aligned grids, and
//! check lines (`- ✓` in the ok colour, `- ✗` in the warn colour). Shared by the app studio's brief and the
//! MIDI studio's stages; a doc with only headings, paragraphs and lists looks as the brief always did.

use egui::text::{LayoutJob, TextFormat};
use egui::{FontId, RichText};

use crate::MfxTheme;

/// One block of a document, in the order it appears.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block<'a> {
    /// `# ` line.
    Title(&'a str),
    /// `## ` (or deeper) line.
    Head(&'a str),
    /// `- ` line.
    Item(&'a str),
    /// `- ✓ ` (true) or `- ✗ ` (false) line, the mark taken off.
    Check(bool, &'a str),
    /// Any other line with words on it.
    Para(&'a str),
    /// Consecutive `|` rows: the header row (when a `|---|` separator follows it), then the others.
    Table { head: Vec<&'a str>, rows: Vec<Vec<&'a str>> },
}

/// A piece of a line: plain, `**bold**` or `` `code` ``.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span<'a> {
    Plain(&'a str),
    Bold(&'a str),
    Code(&'a str),
}

/// The blocks of `md`. Blank lines only separate.
pub fn parse(md: &str) -> Vec<Block<'_>> {
    let mut out = Vec::new();
    let mut table: Vec<&str> = Vec::new();
    for line in md.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('|') {
            table.push(trimmed);
            continue;
        }
        if !table.is_empty() {
            out.push(parse_table(&std::mem::take(&mut table)));
        }
        if let Some(title) = line.strip_prefix("# ") {
            out.push(Block::Title(title));
        } else if line.starts_with("##") {
            out.push(Block::Head(line.trim_start_matches('#').trim_start()));
        } else if let Some(item) = line.strip_prefix("- ") {
            out.push(match (item.strip_prefix("✓"), item.strip_prefix("✗")) {
                (Some(rest), _) => Block::Check(true, rest.trim_start()),
                (_, Some(rest)) => Block::Check(false, rest.trim_start()),
                _ => Block::Item(item),
            });
        } else if !trimmed.is_empty() {
            out.push(Block::Para(line));
        }
    }
    if !table.is_empty() {
        out.push(parse_table(&table));
    }
    out
}

/// `| a | b |` rows into cells; a `|---|` separator after the first row makes it the header.
fn parse_table<'a>(lines: &[&'a str]) -> Block<'a> {
    let separator = |l: &str| cells(l).all(|c| !c.is_empty() && c.trim_matches(':').chars().all(|ch| ch == '-'));
    let mut rows: Vec<Vec<&str>> = Vec::new();
    let mut head = Vec::new();
    for (i, &l) in lines.iter().enumerate() {
        if separator(l) {
            if i == 1 && head.is_empty() {
                head = rows.pop().unwrap_or_default();
            }
            continue;
        }
        rows.push(cells(l).collect());
    }
    Block::Table { head, rows }
}

/// The trimmed cells of a row.
fn cells(l: &str) -> impl Iterator<Item = &str> {
    let inner = l.strip_prefix('|').unwrap_or(l);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    inner.split('|').map(str::trim)
}

/// The bold and code pieces of a line. A marker without its closing one stays as it is.
pub fn spans(line: &str) -> Vec<Span<'_>> {
    let mut out = Vec::new();
    let mut rest = line;
    loop {
        let code = rest.find('`');
        let bold = rest.find("**");
        let (at, mark) = match (code, bold) {
            (Some(c), Some(b)) if b < c => (b, "**"),
            (Some(c), _) => (c, "`"),
            (None, Some(b)) => (b, "**"),
            (None, None) => break,
        };
        let after = &rest[at + mark.len()..];
        let Some(end) = after.find(mark).filter(|&e| e > 0) else {
            // unclosed (or empty): keep the marker as text and look past it
            out.push(Span::Plain(&rest[..at + mark.len()]));
            rest = after;
            continue;
        };
        if at > 0 {
            out.push(Span::Plain(&rest[..at]));
        }
        out.push(if mark == "`" { Span::Code(&after[..end]) } else { Span::Bold(&after[..end]) });
        rest = &after[end + mark.len()..];
    }
    if !rest.is_empty() {
        out.push(Span::Plain(rest));
    }
    out
}

/// `text` as one wrapped line of `size` in `color`, its bold in the text colour and its code in monospace.
fn job(t: &MfxTheme, prefix: &str, text: &str, size: f32, color: egui::Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let plain = TextFormat { font_id: FontId::proportional(size), color, ..TextFormat::default() };
    if !prefix.is_empty() {
        job.append(prefix, 0.0, plain.clone());
    }
    for s in spans(text) {
        match s {
            Span::Plain(p) => job.append(p, 0.0, plain.clone()),
            Span::Bold(b) => job.append(b, 0.0, TextFormat { color: t.text, ..plain.clone() }),
            Span::Code(c) => job.append(c, 0.0, TextFormat { font_id: FontId::monospace(size - 1.0), color: t.text, background: t.field_bg, ..plain.clone() }),
        }
    }
    job
}

/// Draw `md` top to bottom in the current layout.
pub fn show(ui: &mut egui::Ui, t: &MfxTheme, md: &str) {
    for (i, block) in parse(md).into_iter().enumerate() {
        match block {
            Block::Title(title) => {
                ui.label(RichText::new(title).font(FontId::proportional(22.0)).color(t.text));
                ui.add_space(6.0);
            }
            Block::Head(head) => {
                ui.add_space(12.0);
                ui.label(RichText::new(head).size(15.0).color(t.text));
                ui.add_space(2.0);
            }
            Block::Item(item) => {
                ui.add(egui::Label::new(job(t, "•  ", item, 13.0, t.text_dim)).wrap());
            }
            Block::Check(ok, what) => {
                let (mark, color) = if ok { ("✓  ", t.ok) } else { ("✗  ", t.warn) };
                ui.add(egui::Label::new(job(t, mark, what, 13.0, color)).wrap());
            }
            Block::Para(line) => {
                ui.add(egui::Label::new(job(t, "", line, 13.0, t.text_dim)).wrap());
            }
            Block::Table { head, rows } => {
                ui.add_space(4.0);
                // wide tables (a song map with many parts) scroll sideways instead of squeezing
                egui::ScrollArea::horizontal().id_salt(("mfx-md-table-scroll", i)).show(ui, |ui| {
                    egui::Grid::new(("mfx-md-table", i)).spacing([16.0, 4.0]).show(ui, |ui| {
                        if !head.is_empty() {
                            for cell in &head {
                                ui.label(job(t, "", cell, 13.0, t.text));
                            }
                            ui.end_row();
                        }
                        for row in &rows {
                            for cell in row {
                                ui.label(job(t, "", cell, 13.0, t.text_dim));
                            }
                            ui.end_row();
                        }
                    });
                });
                ui.add_space(4.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_has_its_header_and_rows_without_the_separator() {
        let md = "## Sections\n\n| Section | Bars | Energy |\n|---|:---:|---|\n| Intro | 8 | ● |\n| Verse | 16 | ●● |\n\nAfter.";
        let blocks = parse(md);
        assert_eq!(blocks[0], Block::Head("Sections"));
        assert_eq!(blocks[1], Block::Table { head: vec!["Section", "Bars", "Energy"], rows: vec![vec!["Intro", "8", "●"], vec!["Verse", "16", "●●"]] });
        assert_eq!(blocks[2], Block::Para("After."));
    }

    #[test]
    fn a_table_without_a_separator_has_no_header_and_empty_cells_stay() {
        let blocks = parse("| a | | c |\n| d | e | f |");
        assert_eq!(blocks, vec![Block::Table { head: vec![], rows: vec![vec!["a", "", "c"], vec!["d", "e", "f"]] }]);
    }

    #[test]
    fn check_lines_items_and_headings() {
        let blocks = parse("# Song · v1\n## Checks\n- ✓ 4 parts\n- ✗ no shapes for bass\n- a list item\n### Deeper");
        assert_eq!(
            blocks,
            vec![
                Block::Title("Song · v1"),
                Block::Head("Checks"),
                Block::Check(true, "4 parts"),
                Block::Check(false, "no shapes for bass"),
                Block::Item("a list item"),
                Block::Head("Deeper"),
            ]
        );
    }

    #[test]
    fn bold_and_code_pieces() {
        assert_eq!(spans("★ **Four on the floor**: steady"), vec![Span::Plain("★ "), Span::Bold("Four on the floor"), Span::Plain(": steady")]);
        assert_eq!(spans("`s(\"bd*4\")`"), vec![Span::Code("s(\"bd*4\")")]);
        // inside code, ** is just text
        assert_eq!(spans("a `x**y` b"), vec![Span::Plain("a "), Span::Code("x**y"), Span::Plain(" b")]);
        // unclosed markers stay
        assert_eq!(spans("2 * 3 ** 4"), vec![Span::Plain("2 * 3 **"), Span::Plain(" 4")]);
        assert_eq!(spans("plain"), vec![Span::Plain("plain")]);
    }
}

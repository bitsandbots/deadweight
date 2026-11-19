//! A shareable summary card: totals only, never a plugin, skill or server name.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use resvg::{tiny_skia, usvg};
use serde::Serialize;

use crate::analyze::Analysis;
use crate::fix::Plan;
use crate::util::{fmt_tokens, plural};

const W: f64 = 1200.0;
const H: f64 = 675.0;
const PAD: f64 = 64.0;
/// PNGs are rendered at twice the layout size so they stay sharp once a site recompresses them.
const PNG_SCALE: f32 = 2.0;

const BG: &str = "#0d1117";
const PANEL: &str = "#161b22";
const LINE: &str = "#30363d";
const TEXT: &str = "#c9d1d9";
const BRIGHT: &str = "#f0f6fc";
const DIM: &str = "#8b949e";
const RED: &str = "#ff7b72";
const GREEN: &str = "#3fb950";
const CYAN: &str = "#39c5cf";

const SANS: &str = "DejaVu Sans";
const MONO: &str = "DejaVu Sans Mono";

/// Everything the card shows. Built from counts and totals only, so the card
/// can be posted without revealing what someone has installed.
#[derive(Debug, Clone, Serialize)]
pub struct Card {
    /// Measured tokens of the first request, when a measurement exists.
    pub measured: Option<f64>,
    pub setup: f64,
    pub builtin: f64,
    pub skills_listed: usize,
    pub skills_unused: usize,
    /// Biggest category: generic label, item count, tokens.
    pub biggest: Option<(String, usize, f64)>,
    /// Tokens `deadweight fix` would save per request.
    pub saves: f64,
    pub window_days: u64,
    pub sessions: usize,
}

impl Card {
    pub fn new(a: &Analysis, plan: &Plan) -> Self {
        Card {
            measured: a.measured_tokens.map(|m| m as f64),
            setup: a.setup_tokens,
            builtin: a.builtin_tokens,
            skills_listed: a.skills_listed,
            skills_unused: a.skills_unused,
            biggest: a
                .categories
                .first()
                .filter(|c| c.tokens >= 1.0)
                .map(|c| (c.kind.label().to_string(), c.count, c.tokens)),
            saves: plan.tokens,
            window_days: a.window_days,
            sessions: a.sessions,
        }
    }

    pub fn svg(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" font-family="{SANS}">"#
        ));
        s.push_str(&format!(
            r#"<rect width="{W}" height="{H}" fill="{BG}"/><rect x="0" y="0" width="{W}" height="6" fill="{RED}"/>"#
        ));

        // Header.
        s.push_str(&text(
            PAD,
            84.0,
            22.0,
            DIM,
            "bold",
            "start",
            "MY CLAUDE CODE SETUP",
        ));
        s.push_str(&text(
            W - PAD,
            84.0,
            22.0,
            BRIGHT,
            "bold",
            "end",
            "deadweight",
        ));

        // Headline number.
        let (number, line) = match self.measured {
            Some(m) => (m, "on every request, before I type a word"),
            None => (self.setup, "added to every request by my setup"),
        };
        s.push_str(&format!(
            r#"<text x="{PAD}" y="214" fill="{RED}" font-size="128" font-weight="bold">{}<tspan fill="{BRIGHT}" font-size="48" dx="18">tokens</tspan></text>"#,
            esc(&fmt_tokens(number))
        ));
        s.push_str(&text(PAD, 264.0, 30.0, TEXT, "normal", "start", line));

        // Split of the measured total: my setup vs Claude Code itself.
        if let Some(total) = self.measured.filter(|t| *t > 0.0) {
            let width = W - 2.0 * PAD;
            let mine = (self.setup / total).clamp(0.0, 1.0) * width;
            s.push_str(&format!(
                r#"<rect x="{PAD}" y="296" width="{width}" height="18" rx="9" fill="{LINE}"/>"#
            ));
            if mine >= 1.0 {
                s.push_str(&format!(
                    r#"<rect x="{PAD}" y="296" width="{mine:.1}" height="18" rx="9" fill="{RED}"/>"#
                ));
            }
            s.push_str(&format!(
                r#"<text x="{PAD}" y="348" font-size="20" fill="{TEXT}"><tspan fill="{RED}">●</tspan> ~{} from my setup<tspan fill="{DIM}" dx="28">●</tspan> ~{} from Claude Code itself</text>"#,
                esc(&fmt_tokens(self.setup)),
                esc(&fmt_tokens(self.builtin))
            ));
        }

        // Three tiles.
        let gap = 24.0;
        let tile_w = (W - 2.0 * PAD - 2.0 * gap) / 3.0;
        let tiles = self.tiles();
        for (i, (value, color, lines)) in tiles.iter().enumerate() {
            let x = PAD + i as f64 * (tile_w + gap);
            s.push_str(&format!(
                r#"<rect x="{x:.1}" y="384" width="{tile_w:.1}" height="168" rx="14" fill="{PANEL}" stroke="{LINE}"/>"#
            ));
            s.push_str(&text(x + 28.0, 450.0, 52.0, color, "bold", "start", value));
            for (j, l) in lines.iter().enumerate() {
                let y = 492.0 + j as f64 * 28.0;
                s.push_str(&text(x + 28.0, y, 21.0, TEXT, "normal", "start", l));
            }
        }

        // Footer.
        s.push_str(&format!(
            r#"<text x="{PAD}" y="620" font-family="{MONO}" font-size="24"><tspan fill="{GREEN}">$</tspan><tspan fill="{BRIGHT}"> cargo install deadweight</tspan></text>"#
        ));
        s.push_str(&text(
            W - PAD,
            620.0,
            22.0,
            DIM,
            "normal",
            "end",
            "github.com/msertdev/deadweight",
        ));
        s.push_str("</svg>\n");
        s
    }

    fn tiles(&self) -> Vec<(String, &'static str, Vec<String>)> {
        let mut tiles = Vec::new();
        if self.skills_listed > 0 {
            tiles.push((
                format!("{} / {}", self.skills_unused, self.skills_listed),
                RED,
                vec![
                    format!(
                        "{} never used",
                        plural(self.skills_listed, "skill", "skills")
                    ),
                    format!("in the last {} days", self.window_days),
                ],
            ));
        } else {
            tiles.push((
                self.sessions.to_string(),
                CYAN,
                vec![
                    format!("{} analyzed", plural(self.sessions, "session", "sessions")),
                    format!("from the last {} days", self.window_days),
                ],
            ));
        }
        match &self.biggest {
            Some((label, count, tokens)) => tiles.push((
                format!("~{}", fmt_tokens(*tokens)),
                CYAN,
                vec![label.to_lowercase(), format!("({count}), the biggest part")],
            )),
            None => tiles.push((
                format!("~{}", fmt_tokens(self.setup)),
                CYAN,
                vec!["from my setup".into(), "on every request".into()],
            )),
        }
        if self.saves >= 1.0 {
            tiles.push((
                format!("−{}", fmt_tokens(self.saves)),
                GREEN,
                vec!["per request after".into(), "deadweight fix".into()],
            ));
        } else {
            tiles.push((
                "0".into(),
                GREEN,
                vec!["unused plugins".into(), "or skills left".into()],
            ));
        }
        tiles
    }

    /// Write the card; `.svg` paths get the SVG, anything else a PNG.
    pub fn write(&self, path: &Path) -> Result<()> {
        let svg = self.svg();
        let is_svg = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
        let bytes = if is_svg {
            svg.into_bytes()
        } else {
            render_png(&svg)?
        };
        std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
    }
}

fn render_png(svg: &str) -> Result<Vec<u8>> {
    let mut opt = usvg::Options {
        font_family: SANS.into(),
        ..usvg::Options::default()
    };
    let db = opt.fontdb_mut();
    db.load_font_data(dejavu::sans::regular().to_vec());
    db.load_font_data(dejavu::sans::bold().to_vec());
    db.load_font_data(dejavu::sans_mono::regular().to_vec());
    let tree = usvg::Tree::from_str(svg, &opt).context("building the card")?;
    let (w, h) = ((W as f32 * PNG_SCALE) as u32, (H as f32 * PNG_SCALE) as u32);
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or_else(|| anyhow!("card too large"))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(PNG_SCALE, PNG_SCALE),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().context("encoding the card")
}

fn text(x: f64, y: f64, size: f64, fill: &str, weight: &str, anchor: &str, body: &str) -> String {
    format!(
        r#"<text x="{x:.1}" y="{y:.1}" font-size="{size}" fill="{fill}" font-weight="{weight}" text-anchor="{anchor}">{}</text>"#,
        esc(body)
    )
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers from the README's example run.
    fn example() -> Card {
        Card {
            measured: Some(61_200.0),
            setup: 19_400.0,
            builtin: 41_800.0,
            skills_listed: 214,
            skills_unused: 198,
            biggest: Some(("Deferred tool names".into(), 412, 7_100.0)),
            saves: 6_800.0,
            window_days: 30,
            sessions: 38,
        }
    }

    #[test]
    fn svg_has_the_numbers() {
        let svg = example().svg();
        assert!(svg.contains("61.2k"));
        assert!(svg.contains("198 / 214"));
        assert!(svg.contains("−6.8k"));
    }

    #[test]
    fn png_renders() {
        let png = render_png(&example().svg()).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }

    /// `DEADWEIGHT_CARD_OUT=assets/card.png cargo test example_card -- --ignored`
    #[test]
    #[ignore]
    fn example_card() {
        let out = std::env::var("DEADWEIGHT_CARD_OUT").unwrap_or("card-example.png".into());
        example().write(Path::new(&out)).unwrap();
    }
}

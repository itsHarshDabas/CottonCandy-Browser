//! The installer's window: a frameless card in cotton candy colours, drawn by hand with egui.

use crate::work::{self, Options, Shared};
use crate::{platform, Args};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontFamily, FontId, Mesh, Pos2, Rect, Sense,
    Shape, Stroke, StrokeKind,
};
use std::path::PathBuf;
use std::time::Instant;

const W: f32 = 820.0;
const H: f32 = 520.0;
const SIDE: f32 = 300.0;
const RADIUS: u8 = 20;

// Cotton candy: pink, lilac and sky, on a warm white.
const PINK: Color32 = Color32::from_rgb(0xff, 0x8f, 0xc7);
const PINK_SOFT: Color32 = Color32::from_rgb(0xff, 0xd6, 0xec);
const LILAC: Color32 = Color32::from_rgb(0xc3, 0xa6, 0xff);
const SKY: Color32 = Color32::from_rgb(0x8f, 0xd3, 0xfe);
const CREAM: Color32 = Color32::from_rgb(0xff, 0xf9, 0xfc);
const INK: Color32 = Color32::from_rgb(0x3b, 0x23, 0x40);
const INK_2: Color32 = Color32::from_rgb(0x7a, 0x66, 0x80);
const INK_3: Color32 = Color32::from_rgb(0xb3, 0xa3, 0xb8);
const TRACK: Color32 = Color32::from_rgb(0xf6, 0xe8, 0xf2);

/// Little lines shown while the bar fills.
const QUOTES: &[&str] = &[
    "Spinning sugar into silk…",
    "Fluffing up the clouds…",
    "Blocking trackers, keeping the sweetness.",
    "Light as air. Quick as a sugar rush.",
    "Grace is warming up the whisk…",
    "Every tab, a little treat.",
    "Your web, minus the crumbs.",
    "Twirling the last few strands…",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Welcome,
    Working,
    Done,
    Failed,
    UninstallAsk,
}

pub fn run(args: Args) {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([W, H])
            .with_resizable(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_title(if args.uninstall {
                "Uninstall CottonCandy"
            } else {
                "Install CottonCandy"
            }),
        centered: true,
        ..Default::default()
    };
    let _ = eframe::run_native(
        "CottonCandy Setup",
        options,
        Box::new(move |cc| Ok(Box::new(Setup::new(cc, args)))),
    );
}

struct Setup {
    args: Args,
    page: Page,
    dir: PathBuf,
    /// The install folder as typed in the field.
    dir_text: String,
    desktop: bool,
    start_menu: bool,
    launch: bool,
    remove_data: bool,
    size: u64,
    progress: Shared,
    started: Instant,
    error: String,
    demo: bool,
}

impl Setup {
    fn new(cc: &eframe::CreationContext<'_>, args: Args) -> Setup {
        load_fonts(&cc.egui_ctx);
        let mut style = (*cc.egui_ctx.global_style()).clone();
        style.visuals = egui::Visuals::light();
        style.visuals.selection.bg_fill = PINK;
        style.visuals.extreme_bg_color = Color32::WHITE;
        cc.egui_ctx.set_global_style(style);
        let dir = args
            .dir
            .as_deref()
            .map(work::install_dir_in)
            .unwrap_or_else(work::default_dir);
        let demo = args.demo.is_some();
        let mut s = Setup {
            page: if args.uninstall {
                Page::UninstallAsk
            } else {
                Page::Welcome
            },
            dir_text: dir.display().to_string(),
            dir,
            desktop: !args.no_desktop,
            start_menu: !args.no_start_menu,
            launch: true,
            remove_data: false,
            size: if demo {
                384 << 20
            } else {
                work::payload_size()
            },
            progress: Default::default(),
            started: Instant::now(),
            error: String::new(),
            demo,
            args,
        };
        match s.args.demo.as_deref() {
            Some("installing") => s.page = Page::Working,
            Some("done") => s.page = Page::Done,
            Some("uninstall") => {
                s.args.uninstall = true;
                s.page = Page::UninstallAsk;
            }
            Some("error") => {
                s.page = Page::Failed;
                s.error = "CottonCandy is open. Close it, then try again.".into();
            }
            _ => {}
        }
        if s.args.go {
            s.start();
        }
        s
    }

    fn start(&mut self) {
        self.page = Page::Working;
        self.started = Instant::now();
        if self.demo {
            return;
        }
        let progress = self.progress.clone();
        if self.args.uninstall {
            let target = self.args.target.clone().unwrap_or_default();
            let remove = self.remove_data;
            let from_temp = self.args.from_temp;
            #[cfg(windows)]
            if !work::writable(&target) {
                // Installed for everyone (say, in Program Files): removing it needs an administrator.
                let mut args = format!(
                    "--uninstall --from-temp --go --target \"{}\"",
                    target.display()
                );
                if remove {
                    args += " --remove-data";
                }
                if platform::relaunch_elevated(&args) {
                    std::process::exit(0);
                }
                self.page = Page::Failed;
                self.error = "Removing CottonCandy from this folder needs an administrator.".into();
                return;
            }
            std::thread::spawn(move || {
                let r = work::uninstall(&target, remove, &progress);
                if from_temp {
                    platform::delete_self_later();
                }
                if let Ok(mut p) = progress.lock() {
                    p.finished = Some(r);
                }
            });
            return;
        }
        if !self.dir.is_absolute() {
            self.page = Page::Failed;
            self.error =
                "Type a full folder path, like the one we suggested, or use Browse.".into();
            return;
        }
        let opts = Options {
            dir: self.dir.clone(),
            desktop: self.desktop,
            start_menu: self.start_menu,
        };
        #[cfg(windows)]
        if !work::writable(&opts.dir) {
            // A folder like Program Files needs an administrator: ask Windows, and hand over.
            let mut args = format!("--go --dir \"{}\"", opts.dir.display());
            if !opts.desktop {
                args += " --no-desktop";
            }
            if !opts.start_menu {
                args += " --no-start-menu";
            }
            if platform::relaunch_elevated(&args) {
                std::process::exit(0);
            }
            self.page = Page::Failed;
            self.error = format!(
                "CottonCandy can't write to {}. Pick another folder.",
                opts.dir.display()
            );
            return;
        }
        std::thread::spawn(move || {
            let r = work::install(&opts, &progress);
            if let Ok(mut p) = progress.lock() {
                p.finished = Some(r);
            }
        });
    }

    /// 0..1, and the file being worked on.
    fn fraction(&mut self) -> (f32, String) {
        if self.demo {
            let t = self.started.elapsed().as_secs_f32();
            return (((t * 0.18) % 1.0).max(0.62), "libcef.dll".into());
        }
        let Ok(mut p) = self.progress.lock() else {
            return (0.0, String::new());
        };
        if let Some(result) = p.finished.take() {
            match result {
                Ok(()) => self.page = Page::Done,
                Err(e) => {
                    self.page = Page::Failed;
                    self.error = e;
                }
            }
        }
        let f = if p.total > 0 {
            p.done as f32 / p.total as f32
        } else {
            0.0
        };
        (f.clamp(0.0, 1.0), p.file.clone())
    }
}

fn load_fonts(ctx: &egui::Context) {
    // The system's own UI font where there is one (Segoe UI on Windows), so text looks native and crisp.
    let candidates: &[(&str, &str)] = &[
        (
            r"C:\Windows\Fonts\segoeui.ttf",
            r"C:\Windows\Fonts\segoeuib.ttf",
        ),
        (
            "/usr/share/fonts/opentype/inter/Inter-Regular.otf",
            "/usr/share/fonts/opentype/inter/Inter-Bold.otf",
        ),
    ];
    let mut fonts = egui::FontDefinitions::default();
    for (regular, bold) in candidates {
        let (Ok(r), Ok(b)) = (std::fs::read(regular), std::fs::read(bold)) else {
            continue;
        };
        fonts
            .font_data
            .insert("ui".into(), egui::FontData::from_owned(r).into());
        fonts
            .font_data
            .insert("ui-bold".into(), egui::FontData::from_owned(b).into());
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "ui".into());
        fonts.families.insert(
            FontFamily::Name("bold".into()),
            vec!["ui-bold".into(), "ui".into()],
        );
        ctx.set_fonts(fonts);
        return;
    }
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    fonts
        .families
        .insert(FontFamily::Name("bold".into()), fallback);
    ctx.set_fonts(fonts);
}

fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

/// A rectangle filled with a gradient through `stops` from top-left to bottom-right.
fn gradient(rect: Rect, radius: f32, stops: &[Color32]) -> Shape {
    // Rows top to bottom; the left end of each row follows the rounded outer corners, so the rows
    // are dense where the corners curve and sparse in between.
    let mut ys = Vec::new();
    let mut y = 0.0;
    while y < rect.height() {
        ys.push(y);
        y += if y < radius || y > rect.height() - radius - 1.0 {
            1.0
        } else {
            12.0
        };
    }
    ys.push(rect.height());
    let mut mesh = Mesh::default();
    for (i, &dy) in ys.iter().enumerate() {
        let t = dy / rect.height();
        let seg = t * (stops.len() - 1) as f32;
        let k = (seg.floor() as usize).min(stops.len() - 2);
        let c = mix(stops[k], stops[k + 1], seg - k as f32);
        // How far the corner curve sits in from the left at this height.
        let from_edge = dy.min(rect.height() - dy);
        let inset = if from_edge < radius {
            let d = radius - from_edge;
            radius - (radius * radius - d * d).max(0.0).sqrt()
        } else {
            0.0
        };
        mesh.colored_vertex(pos2(rect.left() + inset, rect.top() + dy), c);
        mesh.colored_vertex(
            pos2(rect.right(), rect.top() + dy),
            mix(c, stops[stops.len() - 1], 0.25),
        );
        if i > 0 {
            let b = (i as u32) * 2;
            mesh.add_triangle(b - 2, b - 1, b);
            mesh.add_triangle(b - 1, b + 1, b);
        }
    }
    Shape::mesh(mesh)
}

/// The CottonCandy logo (the same shapes as the app icon), centred at `c`, `s` pixels across.
fn logo(p: &egui::Painter, c: Pos2, s: f32) {
    let k = s / 128.0;
    let at = |x: f32, y: f32| pos2(c.x + (x - 64.0) * k, c.y + (y - 64.0) * k);
    p.rect_filled(
        Rect::from_center_size(c + vec2(0.0, 6.0 * k), vec2(120.0 * k, 120.0 * k)),
        CornerRadius::same((28.0 * k) as u8),
        Color32::from_black_alpha(28),
    );
    p.rect_filled(
        Rect::from_center_size(c, vec2(120.0 * k, 120.0 * k)),
        CornerRadius::same((28.0 * k) as u8),
        Color32::from_rgb(0x1c, 0x1e, 0x25),
    );
    p.line_segment(
        [at(64.0, 70.0), at(64.0, 112.0)],
        Stroke::new(7.0 * k, Color32::from_rgb(0xe6, 0xbe, 0x6e)),
    );
    for (x, y, r, col) in [
        (46.0, 54.0, 20.0, Color32::from_rgb(0xd8, 0x6a, 0x9f)),
        (82.0, 54.0, 20.0, Color32::from_rgb(0xd8, 0x6a, 0x9f)),
        (64.0, 38.0, 22.0, Color32::from_rgb(0xec, 0x62, 0x71)),
        (64.0, 64.0, 20.0, Color32::from_rgb(0xec, 0x62, 0x71)),
        (
            56.0,
            32.0,
            7.0,
            Color32::from_rgba_unmultiplied(0xf5, 0xb3, 0xc4, 180),
        ),
    ] {
        p.circle_filled(at(x, y), r * k, col);
    }
}

/// The candy-striped progress bar: pink and white stripes drifting along, with a glossy top.
fn candy_bar(p: &egui::Painter, rect: Rect, f: f32, t: f32) {
    let r = CornerRadius::same((rect.height() / 2.0) as u8);
    p.rect_filled(rect, r, TRACK);
    let w = (rect.width() * f).max(rect.height());
    let fill = Rect::from_min_size(rect.min, vec2(w, rect.height()));
    p.rect_filled(fill, r, PINK);
    // Stripes, clipped to the filled part (inset so the round ends stay clean).
    let clip = p.with_clip_rect(fill.shrink2(vec2(rect.height() * 0.3, 0.0)));
    let stripe = rect.height() * 0.9;
    let shift = (t * 34.0) % (stripe * 2.0);
    let mut x = fill.left() - stripe * 2.0 + shift;
    while x < fill.right() + stripe {
        let pts = vec![
            pos2(x, fill.bottom()),
            pos2(x + stripe, fill.bottom()),
            pos2(x + stripe * 2.0, fill.top()),
            pos2(x + stripe, fill.top()),
        ];
        clip.add(Shape::convex_polygon(pts, PINK_SOFT, Stroke::NONE));
        x += stripe * 2.0;
    }
    // Gloss and a soft rim.
    let gloss = Rect::from_min_max(
        fill.min + vec2(rect.height() * 0.35, 2.0),
        pos2(fill.right() - rect.height() * 0.35, fill.center().y - 1.0),
    );
    if gloss.width() > 0.0 {
        p.rect_filled(gloss, CornerRadius::same(4), Color32::from_white_alpha(70));
    }
    p.rect_stroke(
        rect,
        r,
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xd9, 0x6c, 0xa8, 60)),
        StrokeKind::Inside,
    );
    // A sparkle at the leading edge.
    let tip = pos2(fill.right() - rect.height() * 0.5, fill.center().y);
    let pulse = 0.6 + 0.4 * (t * 5.0).sin();
    p.circle_filled(tip, rect.height() * 0.22 * pulse, Color32::WHITE);
}

/// A rounded pill button. Returns true when clicked.
fn pill(ui: &mut egui::Ui, rect: Rect, label: &str, primary: bool) -> bool {
    let resp = ui.interact(rect, ui.scope_id().with(label), Sense::click());
    let p = ui.painter();
    let r = CornerRadius::same((rect.height() / 2.0) as u8);
    if primary {
        let lift = if resp.hovered() { 2.0 } else { 0.0 };
        p.rect_filled(
            rect.translate(vec2(0.0, 4.0)),
            r,
            Color32::from_rgba_unmultiplied(0xe0, 0x5a, 0xa8, 50),
        );
        let body = rect.translate(vec2(0.0, -lift));
        p.add(gradient_pill(body, &[PINK, LILAC]));
        p.text(
            body.center(),
            Align2::CENTER_CENTER,
            label,
            bold(15.0),
            Color32::WHITE,
        );
    } else {
        let fill = if resp.hovered() {
            PINK_SOFT
        } else {
            Color32::WHITE
        };
        p.rect(
            rect,
            r,
            fill,
            Stroke::new(1.0, Color32::from_rgb(0xf0, 0xd5, 0xe6)),
            StrokeKind::Inside,
        );
        p.text(rect.center(), Align2::CENTER_CENTER, label, bold(14.0), INK);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// A horizontal gradient inside a pill shape.
fn gradient_pill(rect: Rect, stops: &[Color32; 2]) -> Shape {
    let mut mesh = Mesh::default();
    let r = rect.height() / 2.0;
    let steps = 16;
    // Build the outline (left cap, top, right cap, bottom) and fan it from the centre line.
    let mut outline = Vec::new();
    for i in 0..=steps {
        let a = std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / steps as f32;
        outline.push(pos2(
            rect.left() + r + r * a.cos(),
            rect.center().y - r * a.sin(),
        ));
    }
    for i in 0..=steps {
        let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / steps as f32;
        outline.push(pos2(
            rect.right() - r + r * a.cos(),
            rect.center().y - r * a.sin(),
        ));
    }
    let colour = |x: f32| {
        mix(
            stops[0],
            stops[1],
            ((x - rect.left()) / rect.width()).clamp(0.0, 1.0),
        )
    };
    mesh.colored_vertex(rect.center(), colour(rect.center().x));
    for pt in &outline {
        mesh.colored_vertex(*pt, colour(pt.x));
    }
    let n = outline.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    Shape::mesh(mesh)
}

/// A long path with its middle cut out, so its start and its end both show.
fn short_path(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let head = max / 3;
    let tail = max - head - 1;
    let start: String = s.chars().take(head).collect();
    let end: String = s.chars().skip(n - tail).collect();
    format!("{start}…{end}")
}

/// A four-pointed sparkle, `r` pixels from the centre to each tip.
fn sparkle(p: &egui::Painter, c: Pos2, r: f32, col: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(c, col);
    for i in 0..8 {
        let a = std::f32::consts::FRAC_PI_4 * i as f32;
        let len = if i % 2 == 0 { r } else { r * 0.32 };
        mesh.colored_vertex(c + vec2(a.cos(), a.sin()) * len, col);
    }
    for i in 0..8u32 {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % 8);
    }
    p.add(Shape::mesh(mesh));
}

/// A toggle switch with a label. Returns the new value.
fn toggle(ui: &mut egui::Ui, at: Pos2, label: &str, on: bool) -> bool {
    let sw = Rect::from_min_size(at, vec2(38.0, 22.0));
    let hit = Rect::from_min_size(at, vec2(300.0, 22.0));
    let resp = ui.interact(hit, ui.scope_id().with(label), Sense::click());
    let on = if resp.clicked() { !on } else { on };
    let p = ui.painter();
    let t = ui
        .ctx()
        .animate_bool(ui.scope_id().with((label, "anim")), on);
    p.rect_filled(
        sw,
        CornerRadius::same(11),
        mix(Color32::from_rgb(0xe9, 0xdc, 0xe6), PINK, t),
    );
    let knob = pos2(sw.left() + 11.0 + t * 16.0, sw.center().y);
    p.circle_filled(knob + vec2(0.0, 1.0), 9.0, Color32::from_black_alpha(25));
    p.circle_filled(knob, 9.0, Color32::WHITE);
    p.text(
        pos2(sw.right() + 12.0, sw.center().y),
        Align2::LEFT_CENTER,
        label,
        regular(14.0),
        INK,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    on
}

fn size_text(bytes: u64) -> String {
    if bytes == 0 {
        return String::new();
    }
    format!("Needs about {} MB of space", (bytes + (1 << 20) - 1) >> 20)
}

impl eframe::App for Setup {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // The uninstaller's temporary copy removes itself however the window is closed.
        if self.args.from_temp {
            platform::delete_self_later();
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let t = self.started.elapsed().as_secs_f32();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(root, |ui| {
                let full = Rect::from_min_size(Pos2::ZERO, vec2(W, H));
                let card = full.shrink(1.0);
                let p = ui.painter().clone();
                // The card, and dragging the window by it.
                let drag = ui.interact(card, ui.scope_id().with("drag"), Sense::click_and_drag());
                if drag.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                p.rect_filled(card, CornerRadius::same(RADIUS), CREAM);
                // The candy side.
                let side = Rect::from_min_size(card.min, vec2(SIDE, card.height()));
                let side_clip = p.with_clip_rect(side);
                p.add(gradient(side, RADIUS as f32, &[PINK, LILAC, SKY]));
                // Soft cotton puffs drifting very slowly.
                for (i, (x, y, r)) in [
                    (70.0, 90.0, 70.0),
                    (240.0, 60.0, 50.0),
                    (60.0, 430.0, 80.0),
                    (250.0, 380.0, 60.0),
                    (160.0, 250.0, 110.0),
                ]
                .iter()
                .enumerate()
                {
                    let dx = (t * 0.3 + i as f32).sin() * 6.0;
                    let dy = (t * 0.25 + i as f32 * 1.7).cos() * 5.0;
                    side_clip.circle_filled(
                        card.min + vec2(x + dx, y + dy),
                        *r,
                        Color32::from_white_alpha(34),
                    );
                }
                logo(&p, pos2(side.center().x, side.top() + 170.0), 108.0);
                p.text(
                    pos2(side.center().x, side.top() + 262.0),
                    Align2::CENTER_CENTER,
                    "CottonCandy",
                    bold(30.0),
                    Color32::WHITE,
                );
                p.text(
                    pos2(side.center().x, side.top() + 294.0),
                    Align2::CENTER_CENTER,
                    "The sweet side of the web",
                    regular(14.0),
                    Color32::from_white_alpha(230),
                );
                let chip = Rect::from_center_size(
                    pos2(side.center().x, side.top() + 330.0),
                    vec2(96.0, 24.0),
                );
                p.rect_filled(chip, CornerRadius::same(12), Color32::from_white_alpha(60));
                p.text(
                    chip.center(),
                    Align2::CENTER_CENTER,
                    format!("v{}", env!("CARGO_PKG_VERSION")),
                    bold(12.0),
                    Color32::WHITE,
                );
                p.text(
                    pos2(side.center().x, side.bottom() - 28.0),
                    Align2::CENTER_CENTER,
                    "Free software · GPL-3.0 or later",
                    regular(11.5),
                    Color32::from_white_alpha(200),
                );

                // Window buttons.
                let close = Rect::from_center_size(
                    pos2(card.right() - 26.0, card.top() + 24.0),
                    vec2(28.0, 28.0),
                );
                let min = close.translate(vec2(-34.0, 0.0));
                let working = self.page == Page::Working;
                for (r, is_close) in [(min, false), (close, true)] {
                    let resp =
                        ui.interact(r, ui.scope_id().with(("win", is_close)), Sense::click());
                    if resp.hovered() {
                        p.circle_filled(r.center(), 14.0, if is_close { PINK_SOFT } else { TRACK });
                    }
                    let c = r.center();
                    if is_close {
                        let col = if working { INK_3 } else { INK_2 };
                        p.line_segment(
                            [c + vec2(-5.0, -5.0), c + vec2(5.0, 5.0)],
                            Stroke::new(1.6, col),
                        );
                        p.line_segment(
                            [c + vec2(-5.0, 5.0), c + vec2(5.0, -5.0)],
                            Stroke::new(1.6, col),
                        );
                        if resp.clicked() && !working {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    } else {
                        p.line_segment(
                            [c + vec2(-5.0, 0.0), c + vec2(5.0, 0.0)],
                            Stroke::new(1.6, INK_2),
                        );
                        if resp.clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                    }
                }

                let body = Rect::from_min_max(
                    pos2(side.right() + 44.0, card.top() + 58.0),
                    pos2(card.right() - 44.0, card.bottom() - 36.0),
                );
                match self.page {
                    Page::Welcome => self.welcome(ui, body),
                    Page::UninstallAsk => self.uninstall_ask(ui, body),
                    Page::Working => self.working(ui, body, t),
                    Page::Done => self.done(ui, body, t),
                    Page::Failed => self.failed(ui, body),
                }
                p.rect_stroke(
                    card,
                    CornerRadius::same(RADIUS),
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xd9, 0x6c, 0xa8, 40)),
                    StrokeKind::Outside,
                );
            });
        if matches!(self.page, Page::Working | Page::Done) || self.demo {
            ctx.request_repaint();
        }
    }
}

/// Paints transparent-looking rounded corners on the left side, where the gradient would otherwise
/// show square: everything outside the card's rounded outline is cleared to nothing.
impl Setup {
    fn heading(&self, ui: &egui::Ui, body: Rect, title: &str, lead: &str) {
        let p = ui.painter();
        p.text(body.left_top(), Align2::LEFT_TOP, title, bold(28.0), INK);
        p.text(
            body.left_top() + vec2(0.0, 44.0),
            Align2::LEFT_TOP,
            lead,
            regular(15.0),
            INK_2,
        );
    }

    fn welcome(&mut self, ui: &mut egui::Ui, body: Rect) {
        self.heading(
            ui,
            body,
            "Let's get you set up",
            "A fast, private browser with Grace built in.",
        );
        let p = ui.painter().clone();
        let y = body.top() + 104.0;
        p.text(
            pos2(body.left(), y),
            Align2::LEFT_TOP,
            "INSTALL TO",
            bold(11.5),
            INK_3,
        );
        let has_picker = cfg!(windows);
        let field_w = if has_picker {
            body.width() - 108.0
        } else {
            body.width()
        };
        let field = Rect::from_min_size(pos2(body.left(), y + 22.0), vec2(field_w, 42.0));
        p.rect(
            field,
            CornerRadius::same(12),
            Color32::WHITE,
            Stroke::new(1.0, Color32::from_rgb(0xf0, 0xd5, 0xe6)),
            StrokeKind::Inside,
        );
        // A folder glyph.
        let g = field.left_center() + vec2(16.0, 0.0);
        p.rect_filled(
            Rect::from_min_size(g + vec2(0.0, -6.0), vec2(16.0, 12.0)),
            CornerRadius::same(2),
            PINK,
        );
        p.rect_filled(
            Rect::from_min_size(g + vec2(0.0, -8.0), vec2(7.0, 4.0)),
            CornerRadius::same(1),
            PINK,
        );
        // The folder can be typed in, or picked with Browse.
        let edit_rect =
            Rect::from_min_max(field.min + vec2(42.0, 10.0), field.max - vec2(12.0, 10.0));
        let edit = egui::TextEdit::singleline(&mut self.dir_text)
            .frame(egui::Frame::NONE)
            .font(regular(14.0))
            .text_color(INK)
            .align(egui::Align2::LEFT_CENTER);
        if ui.put(edit_rect, edit).changed() {
            self.dir = PathBuf::from(self.dir_text.trim());
        }
        if has_picker {
            let browse =
                Rect::from_min_size(pos2(field.right() + 10.0, field.top()), vec2(98.0, 42.0));
            if pill(ui, browse, "Browse…", false) {
                if let Some(picked) = platform::pick_folder(&self.dir) {
                    self.dir = work::install_dir_in(&picked);
                    self.dir_text = self.dir.display().to_string();
                }
            }
        }
        p.text(
            pos2(body.left(), field.bottom() + 10.0),
            Align2::LEFT_TOP,
            size_text(self.size),
            regular(12.5),
            INK_3,
        );

        let mut y = field.bottom() + 52.0;
        self.desktop = toggle(
            ui,
            pos2(body.left(), y),
            "Add a desktop shortcut",
            self.desktop,
        );
        y += 34.0;
        self.start_menu = toggle(
            ui,
            pos2(body.left(), y),
            if cfg!(windows) {
                "Add to the Start menu"
            } else {
                "Add to the app menu"
            },
            self.start_menu,
        );
        y += 34.0;
        self.launch = toggle(
            ui,
            pos2(body.left(), y),
            "Open CottonCandy when it's ready",
            self.launch,
        );

        let go = Rect::from_min_size(
            pos2(body.right() - 180.0, body.bottom() - 48.0),
            vec2(180.0, 48.0),
        );
        if pill(ui, go, "Install", true) {
            self.start();
        }
    }

    fn uninstall_ask(&mut self, ui: &mut egui::Ui, body: Rect) {
        self.heading(
            ui,
            body,
            "Leaving so soon?",
            "This removes CottonCandy from this computer.",
        );
        let p = ui.painter().clone();
        let target = self.args.target.clone().unwrap_or_else(work::default_dir);
        p.text(
            pos2(body.left(), body.top() + 104.0),
            Align2::LEFT_TOP,
            "INSTALLED IN",
            bold(11.5),
            INK_3,
        );
        let field = Rect::from_min_size(
            pos2(body.left(), body.top() + 126.0),
            vec2(body.width(), 42.0),
        );
        p.rect(
            field,
            CornerRadius::same(12),
            Color32::WHITE,
            Stroke::new(1.0, Color32::from_rgb(0xf0, 0xd5, 0xe6)),
            StrokeKind::Inside,
        );
        p.with_clip_rect(field.shrink(8.0)).text(
            field.left_center() + vec2(16.0, 0.0),
            Align2::LEFT_CENTER,
            short_path(&target.display().to_string(), 58),
            regular(14.0),
            INK,
        );
        self.remove_data = toggle(
            ui,
            pos2(body.left(), field.bottom() + 30.0),
            "Also delete my history, bookmarks and settings",
            self.remove_data,
        );
        p.text(
            pos2(body.left() + 50.0, field.bottom() + 58.0),
            Align2::LEFT_TOP,
            "Leave this off to keep them for next time.",
            regular(12.5),
            INK_3,
        );
        let keep = Rect::from_min_size(
            pos2(body.right() - 372.0, body.bottom() - 48.0),
            vec2(180.0, 48.0),
        );
        let go = Rect::from_min_size(
            pos2(body.right() - 180.0, body.bottom() - 48.0),
            vec2(180.0, 48.0),
        );
        if pill(ui, keep, "Keep it", false) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if pill(ui, go, "Uninstall", true) {
            self.start();
        }
    }

    fn working(&mut self, ui: &mut egui::Ui, body: Rect, t: f32) {
        let (f, file) = self.fraction();
        let uninstall = self.args.uninstall;
        self.heading(
            ui,
            body,
            if uninstall {
                "Tidying up"
            } else {
                "Spinning your browser"
            },
            if uninstall {
                "Removing CottonCandy. Your other files stay put."
            } else {
                "This takes a moment. Sit back."
            },
        );
        let p = ui.painter().clone();
        let pct = format!("{}%", (f * 100.0).round() as u32);
        p.text(
            pos2(body.left(), body.top() + 128.0),
            Align2::LEFT_TOP,
            pct,
            bold(54.0),
            INK,
        );
        let bar = Rect::from_min_size(
            pos2(body.left(), body.top() + 210.0),
            vec2(body.width(), 22.0),
        );
        candy_bar(&p, bar, f, t);
        let clip = p.with_clip_rect(Rect::from_min_size(
            pos2(body.left(), bar.bottom() + 10.0),
            vec2(body.width(), 20.0),
        ));
        clip.text(
            pos2(body.left(), bar.bottom() + 12.0),
            Align2::LEFT_TOP,
            file,
            regular(12.0),
            INK_3,
        );
        // A new little line every few seconds, fading in and out.
        let period = 3.2;
        let i = ((t / period) as usize) % QUOTES.len();
        let phase = (t % period) / period;
        let alpha = (phase * 6.0).min(1.0).min((1.0 - phase) * 6.0);
        let col = Color32::from_rgba_unmultiplied(0xc2, 0x5e, 0x9a, (alpha * 255.0) as u8);
        sparkle(&p, pos2(body.left() + 7.0, body.bottom() - 30.0), 7.0, col);
        p.text(
            pos2(body.left() + 24.0, body.bottom() - 30.0),
            Align2::LEFT_CENTER,
            QUOTES[i],
            FontId::proportional(16.0),
            col,
        );
    }

    fn done(&mut self, ui: &mut egui::Ui, body: Rect, t: f32) {
        let p = ui.painter().clone();
        // Sprinkles falling gently behind the text.
        let colours = [
            PINK,
            LILAC,
            SKY,
            Color32::from_rgb(0xff, 0xd8, 0x6b),
            Color32::from_rgb(0x7e, 0xe0, 0xb5),
        ];
        for k in 0..26 {
            let seed = k as f32 * 12.9898;
            let x = body.left() + (seed.sin() * 43758.547).fract().abs() * body.width();
            let speed = 18.0 + (k % 5) as f32 * 6.0;
            let y = body.top() - 20.0 + ((t * speed + k as f32 * 37.0) % (body.height() + 40.0));
            let angle = t * 1.5 + k as f32;
            let c = pos2(x, y);
            let d = vec2(angle.cos(), angle.sin()) * 5.0;
            p.line_segment(
                [c - d, c + d],
                Stroke::new(3.5, colours[k % colours.len()].gamma_multiply(0.7)),
            );
        }
        if self.args.uninstall {
            self.heading(
                ui,
                body,
                "All cleaned up",
                "CottonCandy has been removed. Come back any time.",
            );
        } else {
            self.heading(
                ui,
                body,
                "All set, sweet!",
                "CottonCandy is installed and ready to go.",
            );
            p.text(
                pos2(body.left(), body.top() + 108.0),
                Align2::LEFT_TOP,
                format!(
                    "Installed in {}",
                    short_path(&self.dir.display().to_string(), 58)
                ),
                regular(12.5),
                INK_3,
            );
        }
        let close = Rect::from_min_size(
            pos2(body.right() - 180.0, body.bottom() - 48.0),
            vec2(180.0, 48.0),
        );
        if self.args.uninstall {
            if pill(ui, close, "Close", true) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else {
            let label = if self.launch {
                "Open CottonCandy"
            } else {
                "Close"
            };
            if pill(ui, close, label, true) {
                if self.launch && !self.demo {
                    platform::launch(&self.dir);
                }
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn failed(&mut self, ui: &mut egui::Ui, body: Rect) {
        let sub = if self.args.uninstall {
            "Here's what happened. You can run the uninstaller again."
        } else {
            "Here's what happened. Nothing was left half-installed."
        };
        self.heading(ui, body, "That didn't go to plan", sub);
        let p = ui.painter().clone();
        let boxr = Rect::from_min_size(
            pos2(body.left(), body.top() + 104.0),
            vec2(body.width(), 70.0),
        );
        p.rect_filled(boxr, CornerRadius::same(14), PINK_SOFT);
        let galley = p.layout(self.error.clone(), regular(14.0), INK, boxr.width() - 32.0);
        p.galley(boxr.left_top() + vec2(16.0, 14.0), galley, INK);
        let back = Rect::from_min_size(
            pos2(body.right() - 180.0, body.bottom() - 48.0),
            vec2(180.0, 48.0),
        );
        if pill(ui, back, "Try again", true) {
            self.progress = Default::default();
            self.page = if self.args.uninstall {
                Page::UninstallAsk
            } else {
                Page::Welcome
            };
        }
    }
}

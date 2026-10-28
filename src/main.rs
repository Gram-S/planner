#![allow(deprecated)]
//! Daily Planner: calendar to-do app with repeating tasks, subtasks, projects and daily notes.
//! Data: $XDG_DATA_HOME/daily-planner/data.json (older files load fine).

use chrono::{Datelike, Duration, Local, NaiveDate};
use eframe::egui::{
    self,
    text::{LayoutJob, TextFormat},
    Align, Align2, Color32, FontId, Key, Layout, Pos2, Rect, RichText, Rounding, ScrollArea, Sense,
    Stroke, TextStyle, Vec2, Visuals,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    time::Instant,
};

// ---------------------------------------------------------------- palette
#[derive(Clone, Copy)]
struct Pal {
    bg: Color32,
    panel: Color32,
    card: Color32,
    hover: Color32,
    fg: Color32,
    muted: Color32,
    accent: Color32,
    line: Color32,
    sel_fg: Color32,
    green: Color32,
    orange: Color32,
    magenta: Color32,
    code_bg: Color32,
    code_fg: Color32,
    bold: Color32,
}

const fn c(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

// Tokyo Night (slightly boosted contrast)
const DARK: Pal = Pal {
    bg: c(26, 27, 38),
    panel: c(18, 19, 27),
    card: c(38, 43, 64),
    hover: c(54, 61, 90),
    fg: c(210, 218, 248),
    muted: c(125, 133, 175),
    accent: c(130, 170, 255),
    line: c(62, 70, 104),
    sel_fg: c(18, 19, 27),
    green: c(166, 214, 114),
    orange: c(255, 166, 108),
    magenta: c(195, 162, 255),
    code_bg: c(48, 54, 80),
    code_fg: c(166, 214, 114),
    bold: c(255, 166, 108),
};

// Tokyo Night Day
const LIGHT: Pal = Pal {
    bg: c(225, 226, 231),
    panel: c(208, 213, 227),
    card: c(240, 240, 245),
    hover: c(196, 200, 218),
    fg: c(55, 96, 191),
    muted: c(132, 140, 181),
    accent: c(46, 125, 233),
    line: c(196, 200, 218),
    sel_fg: c(225, 226, 231),
    green: c(88, 117, 57),
    orange: c(177, 92, 0),
    magenta: c(152, 84, 241),
    code_bg: c(196, 200, 218),
    code_fg: c(88, 117, 57),
    bold: c(177, 92, 0),
};

fn apply_theme(ctx: &egui::Context, p: &Pal, dark: bool) {
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.extreme_bg_color = p.card;
    v.faint_bg_color = p.card;
    v.code_bg_color = p.code_bg;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.window_stroke = Stroke::new(1.0, p.line);
    v.window_rounding = Rounding::same(12.0);
    let r = Rounding::same(8.0);
    let w = &mut v.widgets;
    for wv in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        wv.rounding = r;
    }
    w.noninteractive.bg_fill = p.bg;
    w.noninteractive.weak_bg_fill = p.bg;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.fg);
    w.inactive.bg_fill = p.card;
    w.inactive.weak_bg_fill = p.card;
    w.inactive.bg_stroke = Stroke::new(1.0, p.line);
    w.inactive.fg_stroke = Stroke::new(1.0, p.fg);
    w.hovered.bg_fill = p.hover;
    w.hovered.weak_bg_fill = p.hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.accent.gamma_multiply(0.6));
    w.hovered.fg_stroke = Stroke::new(1.5, p.fg);
    w.active.bg_fill = p.accent.gamma_multiply(0.45);
    w.active.weak_bg_fill = p.accent.gamma_multiply(0.45);
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(1.5, p.fg);
    w.open.bg_fill = p.hover;
    w.open.weak_bg_fill = p.hover;
    w.open.fg_stroke = Stroke::new(1.0, p.fg);

    let mut s = (*ctx.style()).clone();
    s.visuals = v;
    s.spacing.item_spacing = Vec2::new(8.0, 7.0);
    s.spacing.button_padding = Vec2::new(10.0, 5.0);
    s.text_styles.insert(TextStyle::Heading, FontId::proportional(21.0));
    s.text_styles.insert(TextStyle::Body, FontId::proportional(14.5));
    s.text_styles.insert(TextStyle::Button, FontId::proportional(14.5));
    s.text_styles.insert(TextStyle::Small, FontId::proportional(12.0));
    s.text_styles.insert(TextStyle::Monospace, FontId::monospace(13.5));
    ctx.set_style(s);
}

fn cap(p: &Pal, s: impl Into<String>) -> RichText {
    RichText::new(s).size(12.0).color(p.muted)
}

// ---------------------------------------------------------------- data
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
enum Repeat {
    #[default]
    #[serde(rename = "none")]
    Once,
    Daily,
    Weekdays,
    Weekly,
    Biweekly,
    Monthly,
}

impl Repeat {
    const ALL: [Repeat; 6] = [
        Repeat::Once,
        Repeat::Daily,
        Repeat::Weekdays,
        Repeat::Weekly,
        Repeat::Biweekly,
        Repeat::Monthly,
    ];
    fn label(self) -> &'static str {
        match self {
            Repeat::Once => "Once",
            Repeat::Daily => "Every day",
            Repeat::Weekdays => "Weekdays",
            Repeat::Weekly => "Every week",
            Repeat::Biweekly => "Every 2 weeks",
            Repeat::Monthly => "Every month",
        }
    }
    fn tag(self) -> &'static str {
        match self {
            Repeat::Once => "",
            Repeat::Daily => "daily",
            Repeat::Weekdays => "weekdays",
            Repeat::Weekly => "weekly",
            Repeat::Biweekly => "biweekly",
            Repeat::Monthly => "monthly",
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct Sub {
    id: String,
    title: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct Task {
    id: String,
    title: String,
    #[serde(default)]
    start: String, // "" = not linked to a day (project tasks only)
    #[serde(default)]
    repeat: Repeat,
    #[serde(default)]
    project: String, // "" = no project
    #[serde(default)]
    subs: Vec<Sub>,
    #[serde(default)]
    from: String, // original day if this task was rolled over
    #[serde(skip)]
    sd: Option<NaiveDate>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct Project {
    id: String,
    name: String,
    note: String,
}

/// A custom calendar look: an optional default image plus an optional image per month.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct BgTheme {
    id: String,
    name: String,
    default_img: String,
    months: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Data {
    tasks: Vec<Task>,
    projects: Vec<Project>,
    done: BTreeMap<String, u8>,
    notes: BTreeMap<String, String>,
    theme: String,
    bg_themes: Vec<BgTheme>,
    bg_active: String,
    bg_strength: f32, // percent, kept low so text stays readable
    bg_animate: bool,
}

impl Default for Data {
    fn default() -> Self {
        Data {
            tasks: Vec::new(),
            projects: Vec::new(),
            done: BTreeMap::new(),
            notes: BTreeMap::new(),
            theme: String::new(),
            bg_themes: Vec::new(),
            bg_active: String::new(),
            bg_strength: 30.0,
            bg_animate: true,
        }
    }
}

fn key(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn dk(id: &str, suffix: &str) -> String {
    format!("{}|{}", id, suffix)
}

fn new_id() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:010x}", n & 0xff_ffff_ffff)
}

fn occurs(t: &Task, d: NaiveDate) -> bool {
    let Some(s) = t.sd else { return false };
    if d < s {
        return false;
    }
    match t.repeat {
        Repeat::Once => d == s,
        Repeat::Daily => true,
        Repeat::Weekdays => d.weekday().num_days_from_monday() < 5,
        Repeat::Weekly => d.weekday() == s.weekday(),
        Repeat::Biweekly => (d - s).num_days() % 14 == 0,
        Repeat::Monthly => d.day() == s.day(),
    }
}

impl Data {
    fn tasks_for(&self, d: NaiveDate) -> Vec<&Task> {
        self.tasks.iter().filter(|t| occurs(t, d)).collect()
    }
    fn is_done(&self, id: &str, d: NaiveDate) -> bool {
        self.done.contains_key(&dk(id, &key(d)))
    }
}

fn data_path() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("daily-planner")
        .join("data.json")
}

fn load() -> Data {
    let path = data_path();
    let Ok(text) = fs::read_to_string(&path) else {
        return Data::default();
    };
    let mut data: Data = match serde_json::from_str(&text) {
        Ok(d) => d,
        Err(_) => {
            // Keep a corrupt file instead of overwriting it.
            let _ = fs::rename(&path, path.with_extension("json.bak"));
            Data::default()
        }
    };
    for t in &mut data.tasks {
        t.sd = NaiveDate::parse_from_str(&t.start, "%Y-%m-%d").ok();
    }
    data
}

// ---------------------------------------------------------------- backgrounds
fn bg_dir() -> PathBuf {
    data_path()
        .parent()
        .map(|p| p.join("backgrounds"))
        .unwrap_or_else(|| PathBuf::from("backgrounds"))
}

fn expand_home(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(h) = std::env::var_os("HOME") {
            return PathBuf::from(h).join(rest);
        }
    }
    PathBuf::from(s)
}

struct Loaded {
    key: String,
    frames: Option<Vec<(egui::ColorImage, u32)>>,
}

/// The image currently behind the calendar (only the viewed month's image is kept in memory).
#[derive(Default)]
struct Bg {
    key: String,     // what the textures below were made from ("file|animate")
    loading: String, // what a background thread is decoding right now
    frames: Vec<egui::TextureHandle>,
    delays: Vec<u32>,
    idx: usize,
    next_at: Option<Instant>,
    rx: Option<Receiver<Loaded>>,
}

fn downscale(img: image::RgbaImage, max: u32) -> image::RgbaImage {
    let (w, h) = img.dimensions();
    if w <= max && h <= max {
        return img;
    }
    let s = max as f32 / w.max(h) as f32;
    image::imageops::resize(
        &img,
        ((w as f32 * s).round() as u32).max(1),
        ((h as f32 * s).round() as u32).max(1),
        image::imageops::FilterType::Triangle,
    )
}

fn to_color(img: &image::RgbaImage) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied(
        [img.width() as usize, img.height() as usize],
        img.as_raw(),
    )
}

/// Decode (and shrink) an image. Animated GIFs keep their frames, within a memory budget.
fn decode_bg(path: &Path, animate: bool) -> Option<Vec<(egui::ColorImage, u32)>> {
    use image::AnimationDecoder;
    let is_gif = path
        .extension()
        .and_then(|e| e.to_str())
        .map_or(false, |e| e.eq_ignore_ascii_case("gif"));
    if is_gif && animate {
        let file = std::io::BufReader::new(fs::File::open(path).ok()?);
        let dec = image::codecs::gif::GifDecoder::new(file).ok()?;
        let mut out: Vec<(egui::ColorImage, u32)> = Vec::new();
        let mut bytes = 0usize;
        for fr in dec.into_frames() {
            let Ok(fr) = fr else { break };
            let (n, d) = fr.delay().numer_denom_ms();
            let ms = if d == 0 { 100 } else { (n / d).max(20) };
            let img = downscale(fr.into_buffer(), 640);
            bytes += (img.width() as usize) * (img.height() as usize) * 4;
            out.push((to_color(&img), ms));
            if bytes > 48 * 1024 * 1024 || out.len() >= 120 {
                break;
            }
        }
        if !out.is_empty() {
            return Some(out);
        }
    }
    let img = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    Some(vec![(to_color(&downscale(img.to_rgba8(), 1000)), 0)])
}

type PickResult = Result<Option<PathBuf>, ()>;

/// Ask the desktop for an image file (zenity or kdialog). Err = neither is installed.
fn pick_file() -> PickResult {
    use std::process::Command;
    let attempts: [(&str, &[&str]); 2] = [
        (
            "zenity",
            &[
                "--file-selection",
                "--title=Choose a background image",
                "--file-filter=Images | *.png *.jpg *.jpeg *.gif *.webp",
            ],
        ),
        (
            "kdialog",
            &["--getopenfilename", ".", "*.png *.jpg *.jpeg *.gif *.webp|Images"],
        ),
    ];
    for (cmd, args) in attempts {
        if let Ok(out) = Command::new(cmd).args(args).output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() {
                    return Ok(Some(PathBuf::from(s)));
                }
            }
            return Ok(None);
        }
    }
    Err(())
}

fn spawn_picker(ctx: &egui::Context) -> Receiver<PickResult> {
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(pick_file());
        ctx.request_repaint();
    });
    rx
}

const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September",
    "October", "November", "December",
];

// ---------------------------------------------------------------- markdown
fn push_span(
    job: &mut LayoutJob,
    buf: &mut String,
    flags: (bool, bool, bool),
    base: &FontId,
    color: Color32,
    p: &Pal,
) {
    if buf.is_empty() {
        return;
    }
    let (bold, ital, code) = flags;
    let fmt = TextFormat {
        font_id: if code {
            FontId::monospace((base.size - 1.0).max(8.0))
        } else {
            base.clone()
        },
        color: if code {
            p.code_fg
        } else if bold {
            p.bold
        } else {
            color
        },
        italics: ital,
        background: if code { p.code_bg } else { Color32::TRANSPARENT },
        ..Default::default()
    };
    job.append(buf.as_str(), 0.0, fmt);
    buf.clear();
}

fn inline_into(job: &mut LayoutJob, text: &str, color: Color32, p: &Pal) {
    let base = FontId::proportional(14.5);
    let chars: Vec<char> = text.chars().collect();
    let (mut bold, mut ital, mut code) = (false, false, false);
    let mut buf = String::new();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '`' {
            push_span(job, &mut buf, (bold, ital, code), &base, color, p);
            code = !code;
            i += 1;
            continue;
        }
        if !code && ch == '*' {
            push_span(job, &mut buf, (bold, ital, code), &base, color, p);
            if chars.get(i + 1) == Some(&'*') {
                bold = !bold;
                i += 2;
            } else {
                ital = !ital;
                i += 1;
            }
            continue;
        }
        buf.push(ch);
        i += 1;
    }
    push_span(job, &mut buf, (bold, ital, code), &base, color, p);
}

/// Small markdown subset: headings, bullets, checklists, quotes, rules, code, **bold**, *italic*, `code`.
fn render_md(ui: &mut egui::Ui, text: &str, p: &Pal) {
    let mut in_code = false;
    for raw in text.lines() {
        let line = raw.trim_end();
        let t = line.trim_start();
        if t.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            ui.label(
                RichText::new(if line.is_empty() { " " } else { line })
                    .monospace()
                    .color(p.code_fg)
                    .background_color(p.code_bg),
            );
            continue;
        }
        if t.is_empty() {
            ui.add_space(4.0);
            continue;
        }
        if t == "---" || t == "***" || t == "___" {
            ui.separator();
            continue;
        }
        let hashes = t.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
            let size = match hashes {
                1 => 22.0,
                2 => 18.0,
                3 => 16.0,
                _ => 15.0,
            };
            ui.add_space(4.0);
            ui.label(RichText::new(t[hashes..].trim()).size(size).color(p.accent));
            continue;
        }
        let pad = "    ".repeat(((line.len() - t.len()) / 2).min(4));
        let mut color = p.fg;
        let (prefix, body): (String, &str) = if let Some(r) = t.strip_prefix("- [ ] ") {
            (format!("{pad}[ ] "), r)
        } else if let Some(r) = t.strip_prefix("- [x] ").or_else(|| t.strip_prefix("- [X] ")) {
            color = p.muted;
            (format!("{pad}[x] "), r)
        } else if let Some(r) = t
            .strip_prefix("- ")
            .or_else(|| t.strip_prefix("* "))
            .or_else(|| t.strip_prefix("+ "))
        {
            (format!("{pad}\u{2022} "), r)
        } else if let Some(r) = t.strip_prefix("> ") {
            color = p.muted;
            ("| ".to_string(), r)
        } else {
            (pad, t)
        };
        let mut job = LayoutJob::default();
        if !prefix.is_empty() {
            job.append(
                &prefix,
                0.0,
                TextFormat {
                    font_id: FontId::proportional(14.5),
                    color: p.accent,
                    ..Default::default()
                },
            );
        }
        inline_into(&mut job, body, color, p);
        ui.label(job);
    }
}

// ---------------------------------------------------------------- app
#[derive(Clone, PartialEq)]
enum View {
    Calendar,
    Notes,
    Project(String),
}

const KEYS: &[(&str, &str)] = &[
    ("a", "Add a task (a project task when in a project)"),
    ("n", "Write the day's note"),
    ("m", "Toggle markdown preview"),
    ("t", "Jump to today"),
    ("b", "Backgrounds (custom month images)"),
    ("Left / Right", "Previous / next day"),
    ("Up / Down", "Previous / next week"),
    ("PgUp / PgDn", "Previous / next month"),
    ("c", "Calendar view"),
    ("l", "All notes list"),
    ("1 - 9", "Open project 1 - 9"),
    ("Enter", "Add the task or subtask you're typing"),
    ("Esc", "Leave a text box"),
    ("? or F1", "Show / hide this help"),
];

/// Consume a plain letter key (and its text event) so it doesn't also type into a focused box.
fn take_key(ctx: &egui::Context, k: Key, ch: &str) -> bool {
    ctx.input_mut(|i| {
        if i.modifiers.any() || !i.key_pressed(k) {
            return false;
        }
        i.events
            .retain(|e| !matches!(e, egui::Event::Text(t) if t == ch));
        true
    })
}

struct App {
    data: Data,
    dark: bool,
    today: NaiveDate,
    sel: NaiveDate,
    year: i32,
    month: u32,
    view: View,
    note_buf: String,
    note_preview: bool,
    proj_preview: bool,
    new_title: String,
    new_repeat: Repeat,
    proj_title: String,
    proj_link: bool,
    sub_add: Option<String>,
    sub_buf: String,
    sub_focus: bool,
    focus_task: bool,
    focus_ptask: bool,
    focus_note: bool,
    focus_name: bool,
    confirm_delete: Option<String>,
    confirm_project: Option<String>,
    show_help: bool,
    typing: bool,
    undo: Option<(String, String)>,
    rolled: Vec<(String, String, String)>, // (task id, previous start, previous `from`)
    bg: Bg,
    bg_open: bool,
    pick_slot: Option<usize>,
    pick_path: String,
    pick_note: String,
    pick_rx: Option<Receiver<PickResult>>,
    dirty: bool,
    last_edit: Instant,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let data = load();
        let dark = data.theme != "light";
        let today = Local::now().date_naive();
        let mut app = App {
            data,
            dark,
            today,
            sel: today,
            year: today.year(),
            month: today.month(),
            view: View::Calendar,
            note_buf: String::new(),
            note_preview: false,
            proj_preview: false,
            new_title: String::new(),
            new_repeat: Repeat::Once,
            proj_title: String::new(),
            proj_link: false,
            sub_add: None,
            sub_buf: String::new(),
            sub_focus: false,
            focus_task: false,
            focus_ptask: false,
            focus_note: false,
            focus_name: false,
            confirm_delete: None,
            confirm_project: None,
            show_help: false,
            typing: false,
            undo: None,
            rolled: Vec::new(),
            bg: Bg::default(),
            bg_open: false,
            pick_slot: None,
            pick_path: String::new(),
            pick_note: String::new(),
            pick_rx: None,
            dirty: false,
            last_edit: Instant::now(),
        };
        apply_theme(&cc.egui_ctx, &app.pal(), app.dark);
        app.select(today);
        app.rollover();
        app
    }

    fn pal(&self) -> Pal {
        if self.dark {
            DARK
        } else {
            LIGHT
        }
    }

    fn mark(&mut self) {
        self.dirty = true;
        self.last_edit = Instant::now();
    }

    fn persist(&self) {
        let path = data_path();
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        if let Ok(s) = serde_json::to_string(&self.data) {
            if fs::write(&tmp, s).is_ok() {
                let _ = fs::rename(&tmp, &path);
            }
        }
    }

    fn select(&mut self, d: NaiveDate) {
        self.sel = d;
        self.year = d.year();
        self.month = d.month();
        self.note_buf = self.data.notes.get(&key(d)).cloned().unwrap_or_default();
        self.confirm_delete = None;
        self.sub_add = None;
    }

    fn move_sel(&mut self, days: i64) {
        if let Some(d) = self.sel.checked_add_signed(Duration::days(days)) {
            self.select(d);
        }
    }

    fn shift_month(&mut self, delta: i32) {
        let m0 = self.month as i32 - 1 + delta;
        self.year += m0.div_euclid(12);
        self.month = (m0.rem_euclid(12) + 1) as u32;
    }

    // ----- notes
    fn commit_note(&mut self) {
        let k = key(self.sel);
        if self.note_buf.trim().is_empty() {
            self.data.notes.remove(&k);
        } else {
            self.data.notes.insert(k, self.note_buf.clone());
        }
        self.undo = None;
        self.mark();
    }

    fn delete_note(&mut self, k: &str) {
        if let Some(text) = self.data.notes.remove(k) {
            self.undo = Some((k.to_string(), text));
            if k == key(self.sel) {
                self.note_buf.clear();
            }
            self.mark();
        } else if k == key(self.sel) {
            self.note_buf.clear();
        }
    }

    fn undo_note(&mut self) {
        if let Some((k, text)) = self.undo.take() {
            if k == key(self.sel) {
                self.note_buf = text.clone();
            }
            self.data.notes.insert(k, text);
            self.mark();
        }
    }

    // ----- tasks
    fn add_task(&mut self) {
        let title = self.new_title.trim().to_string();
        if title.is_empty() {
            return;
        }
        self.data.tasks.push(Task {
            id: new_id(),
            title,
            start: key(self.sel),
            repeat: self.new_repeat,
            project: String::new(),
            subs: Vec::new(),
            from: String::new(),
            sd: Some(self.sel),
        });
        self.new_title.clear();
        self.mark();
    }

    fn add_project_task(&mut self, pid: &str) {
        let title = self.proj_title.trim().to_string();
        if title.is_empty() {
            return;
        }
        let (start, sd) = if self.proj_link {
            (key(self.sel), Some(self.sel))
        } else {
            (String::new(), None)
        };
        self.data.tasks.push(Task {
            id: new_id(),
            title,
            start,
            repeat: Repeat::Once,
            project: pid.to_string(),
            subs: Vec::new(),
            from: String::new(),
            sd,
        });
        self.proj_title.clear();
        self.mark();
    }

    /// Unfinished one-off tasks from earlier days. Repeating tasks are skipped: they recur anyway.
    fn overdue_ids(&self) -> Vec<String> {
        self.data
            .tasks
            .iter()
            .filter(|t| {
                t.repeat == Repeat::Once
                    && t.sd.map_or(false, |d| d < self.today && !self.data.is_done(&t.id, d))
            })
            .map(|t| t.id.clone())
            .collect()
    }

    fn move_done(&mut self, id: &str, from: &str, to: &str) {
        if self.data.done.remove(&dk(id, from)).is_some() {
            self.data.done.insert(dk(id, to), 1);
        }
    }

    /// Move every overdue task to today. Finished subtasks keep their checkmarks.
    fn rollover(&mut self) -> usize {
        let ids = self.overdue_ids();
        if ids.is_empty() {
            return 0;
        }
        let today = self.today;
        let new = key(today);
        let mut record = Vec::new();
        for id in &ids {
            let Some(i) = self.data.tasks.iter().position(|t| &t.id == id) else {
                continue;
            };
            let old_start = self.data.tasks[i].start.clone();
            let old_from = self.data.tasks[i].from.clone();
            let subs: Vec<String> = self.data.tasks[i].subs.iter().map(|s| s.id.clone()).collect();
            for sid in &subs {
                self.move_done(sid, &old_start, &new);
            }
            let t = &mut self.data.tasks[i];
            if t.from.is_empty() {
                t.from = old_start.clone();
            }
            t.start = new.clone();
            t.sd = Some(today);
            record.push((id.clone(), old_start, old_from));
        }
        let n = record.len();
        self.rolled = record;
        self.mark();
        n
    }

    fn undo_rollover(&mut self) {
        let record = std::mem::take(&mut self.rolled);
        for (id, old_start, old_from) in record {
            let Some(i) = self.data.tasks.iter().position(|t| t.id == id) else {
                continue;
            };
            let cur = self.data.tasks[i].start.clone();
            let subs: Vec<String> = self.data.tasks[i].subs.iter().map(|s| s.id.clone()).collect();
            for sid in &subs {
                self.move_done(sid, &cur, &old_start);
            }
            let t = &mut self.data.tasks[i];
            t.start = old_start.clone();
            t.sd = NaiveDate::parse_from_str(&old_start, "%Y-%m-%d").ok();
            t.from = old_from;
        }
        self.mark();
    }

    fn purge_done(&mut self, tid: &str) {
        let mut prefixes = vec![format!("{}|", tid)];
        if let Some(t) = self.data.tasks.iter().find(|t| t.id == tid) {
            prefixes.extend(t.subs.iter().map(|s| format!("{}|", s.id)));
        }
        self.data
            .done
            .retain(|k, _| !prefixes.iter().any(|p| k.starts_with(p.as_str())));
    }

    fn delete_task(&mut self, tid: &str) {
        self.purge_done(tid);
        self.data.tasks.retain(|t| t.id != tid);
        if self.sub_add.as_deref() == Some(tid) {
            self.sub_add = None;
        }
        self.mark();
    }

    /// One task row plus its subtasks. `suffix` is the occurrence key ("YYYY-MM-DD" or "-").
    fn task_block(&mut self, ui: &mut egui::Ui, tid: &str, suffix: &str, in_project: bool) {
        let p = self.pal();
        let Some(t) = self.data.tasks.iter().find(|t| t.id == tid).cloned() else {
            return;
        };
        let done = self.data.done.contains_key(&dk(&t.id, suffix));
        let proj_name = if !in_project && !t.project.is_empty() {
            self.data
                .projects
                .iter()
                .find(|x| x.id == t.project)
                .map(|x| x.name.clone())
        } else {
            None
        };
        let subs_done: Vec<bool> = t
            .subs
            .iter()
            .map(|s| self.data.done.contains_key(&dk(&s.id, suffix)))
            .collect();
        let n_done = subs_done.iter().filter(|d| **d).count();

        let mut toggle: Option<bool> = None;
        let mut toggle_sub: Option<(String, bool)> = None;
        let mut del = false;
        let mut del_sub: Option<String> = None;
        let mut open_sub = false;
        let mut add_sub = false;
        let mut cancel_sub = false;
        let mut jump: Option<NaiveDate> = None;
        let mut link = 0u8;

        ui.horizontal(|ui| {
            let mut checked = done;
            let title = if done {
                RichText::new(&t.title).strikethrough().color(p.muted)
            } else {
                RichText::new(&t.title)
            };
            if ui.checkbox(&mut checked, title).changed() {
                toggle = Some(checked);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("×").on_hover_text("Delete task").clicked() {
                    del = true;
                }
                if ui.small_button("+").on_hover_text("Add subtask").clicked() {
                    open_sub = true;
                }
                if in_project {
                    if let Some(d) = t.sd {
                        if ui.small_button("unlink").clicked() {
                            link = 2;
                        }
                        if ui.small_button(d.format("%b %-d").to_string()).clicked() {
                            jump = Some(d);
                        }
                    } else if ui.small_button("link day").clicked() {
                        link = 1;
                    }
                } else {
                    if t.repeat != Repeat::Once {
                        ui.label(RichText::new(t.repeat.tag()).size(11.5).color(p.muted));
                    }
                    if let Some(n) = &proj_name {
                        ui.label(RichText::new(n).size(11.5).color(p.magenta));
                    }
                    if let Ok(d) = NaiveDate::parse_from_str(&t.from, "%Y-%m-%d") {
                        ui.label(
                            RichText::new(format!("from {}", d.format("%b %-d")))
                                .size(11.5)
                                .color(p.orange),
                        );
                    }
                }
                if !t.subs.is_empty() {
                    ui.label(
                        RichText::new(format!("{}/{}", n_done, t.subs.len()))
                            .size(11.5)
                            .color(p.muted),
                    );
                }
            });
        });

        for (i, s) in t.subs.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                let mut checked = subs_done[i];
                let title = if checked {
                    RichText::new(&s.title).size(13.5).strikethrough().color(p.muted)
                } else {
                    RichText::new(&s.title).size(13.5)
                };
                if ui.checkbox(&mut checked, title).changed() {
                    toggle_sub = Some((s.id.clone(), checked));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("×").clicked() {
                        del_sub = Some(s.id.clone());
                    }
                });
            });
        }

        if self.sub_add.as_deref() == Some(tid) {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.sub_buf)
                        .hint_text("Subtask…")
                        .desired_width(f32::INFINITY),
                );
                if std::mem::take(&mut self.sub_focus) {
                    r.request_focus();
                }
                self.typing |= r.has_focus();
                if r.lost_focus() {
                    if ui.input(|i| i.key_pressed(Key::Enter)) {
                        add_sub = true;
                    } else {
                        cancel_sub = true;
                    }
                }
            });
        }

        // ----- apply
        if let Some(v) = toggle {
            let k = dk(&t.id, suffix);
            if v {
                self.data.done.insert(k, 1);
            } else {
                self.data.done.remove(&k);
            }
            self.mark();
        }
        if let Some((sid, v)) = toggle_sub {
            let k = dk(&sid, suffix);
            if v {
                self.data.done.insert(k, 1);
            } else {
                self.data.done.remove(&k);
            }
            self.mark();
        }
        if open_sub {
            self.sub_add = Some(t.id.clone());
            self.sub_buf.clear();
            self.sub_focus = true;
            ui.ctx().request_repaint();
        }
        if add_sub {
            let title = self.sub_buf.trim().to_string();
            if title.is_empty() {
                self.sub_add = None;
            } else {
                if let Some(x) = self.data.tasks.iter_mut().find(|x| x.id == tid) {
                    x.subs.push(Sub { id: new_id(), title });
                }
                self.sub_buf.clear();
                self.sub_focus = true;
                self.mark();
            }
        }
        if cancel_sub {
            self.sub_add = None;
        }
        if let Some(sid) = del_sub {
            if let Some(x) = self.data.tasks.iter_mut().find(|x| x.id == tid) {
                x.subs.retain(|s| s.id != sid);
            }
            let prefix = format!("{}|", sid);
            self.data.done.retain(|k, _| !k.starts_with(&prefix));
            self.mark();
        }
        if let Some(d) = jump {
            self.select(d);
            self.view = View::Calendar;
        }
        if link != 0 {
            self.purge_done(tid);
            let sel = self.sel;
            if let Some(x) = self.data.tasks.iter_mut().find(|x| x.id == tid) {
                if link == 1 {
                    x.start = key(sel);
                    x.sd = Some(sel);
                } else {
                    x.start.clear();
                    x.sd = None;
                }
            }
            self.mark();
        }
        if del {
            if t.repeat != Repeat::Once {
                self.confirm_delete = Some(t.id.clone());
            } else {
                self.delete_task(tid);
            }
        }
    }

    // ----- backgrounds
    fn bg_file_for_month(&self) -> Option<String> {
        let th = self.data.bg_themes.iter().find(|t| t.id == self.data.bg_active)?;
        let month = th.months.get(self.month as usize - 1).cloned().unwrap_or_default();
        let f = if month.is_empty() { th.default_img.clone() } else { month };
        if f.is_empty() {
            None
        } else {
            Some(f)
        }
    }

    fn bg_current(&self) -> Option<(egui::TextureId, Vec2)> {
        self.bg.frames.get(self.bg.idx).map(|t| (t.id(), t.size_vec2()))
    }

    fn gc_bg(&mut self, file: &str) {
        if file.is_empty() {
            return;
        }
        let used = self
            .data
            .bg_themes
            .iter()
            .any(|t| t.default_img == file || t.months.iter().any(|m| m == file));
        if !used {
            let _ = fs::remove_file(bg_dir().join(file));
        }
    }

    /// Copy `src` into the app's backgrounds folder and assign it to a slot (0 = default, 1-12 = month).
    fn set_slot(&mut self, slot: usize, src: &Path) {
        let Some(ti) = self.data.bg_themes.iter().position(|t| t.id == self.data.bg_active) else {
            return;
        };
        if slot > 12 {
            return;
        }
        let ext = src
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if !["png", "jpg", "jpeg", "gif", "webp"].contains(&ext.as_str()) {
            self.pick_note = "Please use a PNG, JPG, GIF or WebP file.".to_string();
            return;
        }
        let dir = bg_dir();
        let _ = fs::create_dir_all(&dir);
        let name = format!("{}.{}", new_id(), ext);
        if let Err(e) = fs::copy(src, dir.join(&name)) {
            self.pick_note = format!("Couldn't read that file: {}", e);
            return;
        }
        let th = &mut self.data.bg_themes[ti];
        th.months.resize(12, String::new());
        let old = if slot == 0 {
            std::mem::replace(&mut th.default_img, name)
        } else {
            std::mem::replace(&mut th.months[slot - 1], name)
        };
        self.gc_bg(&old);
        self.pick_note.clear();
        self.pick_path.clear();
        self.bg.key.clear();
        self.mark();
    }

    fn clear_slot(&mut self, slot: usize) {
        let Some(ti) = self.data.bg_themes.iter().position(|t| t.id == self.data.bg_active) else {
            return;
        };
        if slot > 12 {
            return;
        }
        let th = &mut self.data.bg_themes[ti];
        th.months.resize(12, String::new());
        let old = if slot == 0 {
            std::mem::take(&mut th.default_img)
        } else {
            std::mem::take(&mut th.months[slot - 1])
        };
        self.gc_bg(&old);
        self.bg.key.clear();
        self.mark();
    }

    /// Load / swap / animate the calendar background. Decoding happens on a worker thread.
    fn bg_update(&mut self, ctx: &egui::Context) {
        // finished file dialog?
        let picked = self.pick_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(res) = picked {
            self.pick_rx = None;
            match res {
                Ok(Some(path)) => {
                    if let Some(slot) = self.pick_slot {
                        self.set_slot(slot, &path);
                    }
                }
                Ok(None) => {}
                Err(()) => {
                    self.pick_note = "No file dialog found (install zenity or kdialog). Paste a path below, or drop an image file onto the window.".to_string();
                }
            }
        }
        // dropped files go to the slot chosen last
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for f in dropped {
            if let (Some(slot), Some(path)) = (self.pick_slot, f.path) {
                self.set_slot(slot, &path);
            }
        }

        if self.view == View::Calendar {
            let want = match self.bg_file_for_month() {
                Some(f) => format!("{}|{}", f, self.data.bg_animate),
                None => String::new(),
            };
            if want.is_empty() {
                if self.bg.rx.is_some() || !self.bg.frames.is_empty() || !self.bg.key.is_empty() {
                    self.bg = Bg::default();
                }
            } else if want != self.bg.key && want != self.bg.loading {
                let file = want.split('|').next().unwrap_or("").to_string();
                let path = bg_dir().join(file);
                let animate = self.data.bg_animate;
                let (tx, rx) = mpsc::channel();
                self.bg.rx = Some(rx);
                self.bg.loading = want.clone();
                let ctx2 = ctx.clone();
                std::thread::spawn(move || {
                    let frames = decode_bg(&path, animate);
                    let _ = tx.send(Loaded { key: want, frames });
                    ctx2.request_repaint();
                });
            }
        }

        let got = self.bg.rx.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(l) = got {
            self.bg.rx = None;
            self.bg.loading.clear();
            self.bg.key = l.key;
            self.bg.frames.clear();
            self.bg.delays.clear();
            self.bg.idx = 0;
            if let Some(frames) = l.frames {
                for (i, (img, ms)) in frames.into_iter().enumerate() {
                    self.bg
                        .frames
                        .push(ctx.load_texture(format!("bg{}", i), img, egui::TextureOptions::LINEAR));
                    self.bg.delays.push(ms);
                }
                let first = self.bg.delays.first().copied().unwrap_or(0).max(50) as u64;
                self.bg.next_at = Some(Instant::now() + std::time::Duration::from_millis(first));
            }
        }

        if self.view == View::Calendar && self.data.bg_animate && self.bg.frames.len() > 1 {
            let now = Instant::now();
            let due = self.bg.next_at.unwrap_or(now);
            if now >= due {
                self.bg.idx = (self.bg.idx + 1) % self.bg.frames.len();
                let d = self.bg.delays[self.bg.idx].max(50) as u64;
                self.bg.next_at = Some(now + std::time::Duration::from_millis(d));
            }
            let wait = self
                .bg
                .next_at
                .unwrap_or(now)
                .saturating_duration_since(Instant::now());
            ctx.request_repaint_after(wait);
        }
    }

    /// The "Backgrounds" window.
    fn bg_ui(&mut self, ui: &mut egui::Ui) {
        let p = self.pal();
        let themes: Vec<(String, String)> = self
            .data
            .bg_themes
            .iter()
            .map(|t| (t.id.clone(), t.name.clone()))
            .collect();
        let active_name = themes
            .iter()
            .find(|t| t.0 == self.data.bg_active)
            .map(|t| t.1.clone())
            .unwrap_or_else(|| "None".to_string());
        let mut new_theme = false;
        ui.horizontal(|ui| {
            ui.label("Theme");
            egui::ComboBox::from_id_source("bg-theme")
                .selected_text(active_name)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(self.data.bg_active.is_empty(), "None").clicked() {
                        self.data.bg_active.clear();
                        self.mark();
                    }
                    for (id, name) in &themes {
                        if ui
                            .selectable_label(self.data.bg_active == *id, name.as_str())
                            .clicked()
                        {
                            self.data.bg_active = id.clone();
                            self.mark();
                        }
                    }
                });
            if ui.button("New").clicked() {
                new_theme = true;
            }
        });
        if new_theme {
            let id = new_id();
            let n = self.data.bg_themes.len() + 1;
            self.data.bg_themes.push(BgTheme {
                id: id.clone(),
                name: format!("Theme {}", n),
                default_img: String::new(),
                months: vec![String::new(); 12],
            });
            self.data.bg_active = id;
            self.mark();
        }

        let Some(ti) = self
            .data
            .bg_themes
            .iter()
            .position(|t| t.id == self.data.bg_active)
        else {
            ui.add_space(6.0);
            ui.label(cap(
                &p,
                "Create a theme, then add one image for the year or one per month.",
            ));
            return;
        };
        self.data.bg_themes[ti].months.resize(12, String::new());
        ui.add_space(6.0);

        let mut delete_theme = false;
        ui.horizontal(|ui| {
            ui.label("Name");
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.data.bg_themes[ti].name).desired_width(170.0),
            );
            self.typing |= r.has_focus();
            if r.changed() {
                self.mark();
            }
            if ui.button("Delete theme").clicked() {
                delete_theme = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("Image strength");
            if ui
                .add(
                    egui::Slider::new(&mut self.data.bg_strength, 5.0..=50.0)
                        .suffix("%")
                        .integer(),
                )
                .changed()
            {
                self.mark();
            }
        });
        if ui.checkbox(&mut self.data.bg_animate, "Animate GIFs").changed() {
            self.bg.key.clear();
            self.mark();
        }
        ui.label(cap(
            &p,
            "Images stay faint behind the calendar so dates and tasks remain easy to read.",
        ));
        ui.add_space(6.0);

        let files: Vec<String> = std::iter::once(self.data.bg_themes[ti].default_img.clone())
            .chain(self.data.bg_themes[ti].months.iter().cloned())
            .collect();
        let mut choose: Option<usize> = None;
        let mut clear: Option<usize> = None;
        egui::Grid::new("bg-slots")
            .num_columns(3)
            .spacing([14.0, 6.0])
            .show(ui, |ui| {
                for slot in 0..13usize {
                    ui.label(if slot == 0 { "All months" } else { MONTHS[slot - 1] });
                    let (text, color) = if !files[slot].is_empty() {
                        ("Image set", p.green)
                    } else if slot > 0 && !files[0].is_empty() {
                        ("Uses all-months image", p.muted)
                    } else {
                        ("None", p.muted)
                    };
                    ui.label(RichText::new(text).size(12.5).color(color));
                    ui.horizontal(|ui| {
                        if ui.small_button("Choose…").clicked() {
                            choose = Some(slot);
                        }
                        if !files[slot].is_empty() && ui.small_button("Clear").clicked() {
                            clear = Some(slot);
                        }
                    });
                    ui.end_row();
                }
            });

        if delete_theme {
            let th = self.data.bg_themes.remove(ti);
            self.data.bg_active.clear();
            let mut old = th.months;
            old.push(th.default_img);
            for f in old {
                self.gc_bg(&f);
            }
            self.bg.key.clear();
            self.mark();
            return;
        }
        if let Some(slot) = choose {
            self.pick_slot = Some(slot);
            self.pick_note.clear();
            if self.pick_rx.is_none() {
                self.pick_rx = Some(spawn_picker(ui.ctx()));
            }
        }
        if let Some(slot) = clear {
            self.clear_slot(slot);
        }

        if let Some(slot) = self.pick_slot {
            if !self.pick_note.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new(self.pick_note.clone()).size(12.5).color(p.orange));
                ui.horizontal(|ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.pick_path)
                            .hint_text("/path/to/image.png")
                            .desired_width(260.0),
                    );
                    self.typing |= r.has_focus();
                    if ui.button("Use image").clicked() {
                        let path = expand_home(self.pick_path.trim().trim_matches('"'));
                        self.set_slot(slot, &path);
                    }
                });
            }
        }
    }

    // ----- hotkeys
    fn hotkeys(&mut self, ctx: &egui::Context) {
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k) && !i.modifiers.any());
        if pressed(Key::ArrowLeft) {
            self.move_sel(-1);
        }
        if pressed(Key::ArrowRight) {
            self.move_sel(1);
        }
        if pressed(Key::ArrowUp) {
            self.move_sel(-7);
        }
        if pressed(Key::ArrowDown) {
            self.move_sel(7);
        }
        if pressed(Key::PageUp) {
            self.shift_month(-1);
        }
        if pressed(Key::PageDown) {
            self.shift_month(1);
        }
        let help = ctx.input(|i| {
            i.key_pressed(Key::F1)
                || i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Text(t) if t == "?"))
        });
        if help {
            self.show_help = !self.show_help;
        }
        if take_key(ctx, Key::A, "a") {
            if matches!(self.view, View::Project(_)) {
                self.focus_ptask = true;
            } else {
                self.focus_task = true;
            }
            ctx.request_repaint();
        }
        if take_key(ctx, Key::N, "n") {
            self.note_preview = false;
            self.focus_note = true;
            ctx.request_repaint();
        }
        if take_key(ctx, Key::M, "m") {
            if matches!(self.view, View::Project(_)) {
                self.proj_preview = !self.proj_preview;
            } else {
                self.note_preview = !self.note_preview;
            }
        }
        if take_key(ctx, Key::B, "b") {
            self.bg_open = !self.bg_open;
        }
        if take_key(ctx, Key::T, "t") {
            self.select(self.today);
        }
        if take_key(ctx, Key::C, "c") {
            self.view = View::Calendar;
        }
        if take_key(ctx, Key::L, "l") {
            self.view = View::Notes;
        }
        let nums = [
            (Key::Num1, "1"),
            (Key::Num2, "2"),
            (Key::Num3, "3"),
            (Key::Num4, "4"),
            (Key::Num5, "5"),
            (Key::Num6, "6"),
            (Key::Num7, "7"),
            (Key::Num8, "8"),
            (Key::Num9, "9"),
        ];
        for (n, (k, s)) in nums.iter().enumerate() {
            if take_key(ctx, *k, s) {
                if let Some(pr) = self.data.projects.get(n) {
                    self.view = View::Project(pr.id.clone());
                }
            }
        }
    }

    // ---------- left navigation
    fn nav(&mut self, ui: &mut egui::Ui) {
        let p = self.pal();
        ui.label(RichText::new("Daily Planner").size(16.0).color(p.accent));
        ui.add_space(10.0);
        let mut new_project = false;
        ui.with_layout(Layout::top_down_justified(Align::Min), |ui| {
            if ui.selectable_label(self.view == View::Calendar, "Calendar").clicked() {
                self.view = View::Calendar;
            }
            if ui.selectable_label(self.view == View::Notes, "All notes").clicked() {
                self.view = View::Notes;
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(cap(&p, "PROJECTS"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("+").on_hover_text("New project").clicked() {
                        new_project = true;
                    }
                });
            });
            let list: Vec<(String, String)> = self
                .data
                .projects
                .iter()
                .map(|x| (x.id.clone(), x.name.clone()))
                .collect();
            for (i, (id, name)) in list.iter().enumerate() {
                let shown = if name.trim().is_empty() { "Untitled" } else { name.as_str() };
                let label = if i < 9 {
                    format!("{}  {}", i + 1, shown)
                } else {
                    shown.to_string()
                };
                let here = self.view == View::Project(id.clone());
                if ui.selectable_label(here, label).clicked() {
                    self.view = View::Project(id.clone());
                }
            }
        });
        if new_project {
            let id = new_id();
            self.data.projects.push(Project {
                id: id.clone(),
                name: String::new(),
                note: String::new(),
            });
            self.view = View::Project(id);
            self.focus_name = true;
            self.mark();
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            if ui.button("Shortcuts (?)").clicked() {
                self.show_help = true;
            }
            if ui.button("Backgrounds").clicked() {
                self.bg_open = !self.bg_open;
            }
            if ui.button(if self.dark { "Light theme" } else { "Dark theme" }).clicked() {
                self.dark = !self.dark;
                self.data.theme = if self.dark { "dark" } else { "light" }.to_string();
                apply_theme(ui.ctx(), &self.pal(), self.dark);
                self.mark();
            }
        });
    }

    // ---------- calendar
    fn calendar(&mut self, ui: &mut egui::Ui) {
        let p = self.pal();
        let has_bg = self.bg_current().is_some();
        if let Some((tex, size)) = self.bg_current() {
            // Cover-fit the image behind the whole calendar area, kept faint.
            let full = ui.max_rect().expand(18.0);
            let (ia, ra) = (size.x / size.y.max(1.0), full.width() / full.height().max(1.0));
            let uv = if ia > ra {
                let f = ra / ia;
                Rect::from_min_max(Pos2::new(0.5 - f / 2.0, 0.0), Pos2::new(0.5 + f / 2.0, 1.0))
            } else {
                let f = ia / ra;
                Rect::from_min_max(Pos2::new(0.0, 0.5 - f / 2.0), Pos2::new(1.0, 0.5 + f / 2.0))
            };
            let a = (self.data.bg_strength.clamp(5.0, 50.0) / 100.0 * 255.0) as u8;
            let bgp = ui.painter().with_clip_rect(full);
            bgp.image(tex, full, uv, Color32::from_white_alpha(a));
            // soft backing behind the month title and weekday row
            let m = ui.max_rect();
            bgp.rect_filled(
                Rect::from_min_size(m.min, Vec2::new(m.width(), 66.0)).expand(6.0),
                10.0,
                Color32::from_rgba_unmultiplied(p.bg.r(), p.bg.g(), p.bg.b(), 190),
            );
        }
        let tr = |c: Color32, a: u8| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
        let (card_c, hover_c) = if has_bg {
            (tr(p.card, 214), tr(p.hover, 226))
        } else {
            (p.card, p.hover)
        };
        let first = NaiveDate::from_ymd_opt(self.year, self.month, 1).unwrap();
        ui.horizontal(|ui| {
            ui.heading(first.format("%B %Y").to_string());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(">").clicked() {
                    self.shift_month(1);
                }
                if ui.button("Today").clicked() {
                    self.select(self.today);
                }
                if ui.button("<").clicked() {
                    self.shift_month(-1);
                }
            });
        });
        ui.add_space(8.0);

        let avail = ui.available_size();
        let cw = avail.x / 7.0;
        let hh = 22.0;
        let ch = ((avail.y - hh) / 6.0).clamp(48.0, 150.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(avail.x, hh + ch * 6.0), Sense::hover());
        let painter = ui.painter().clone();

        for (i, n) in ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"].iter().enumerate() {
            painter.text(
                Pos2::new(rect.left() + cw * (i as f32 + 0.5), rect.top() + hh / 2.0),
                Align2::CENTER_CENTER,
                *n,
                FontId::proportional(12.0),
                p.muted,
            );
        }

        let start = first - Duration::days(first.weekday().num_days_from_sunday() as i64);
        let mut clicked = None;
        for i in 0..42i64 {
            let d = start + Duration::days(i);
            let (r, col) = ((i / 7) as f32, (i % 7) as f32);
            let cell = Rect::from_min_size(
                Pos2::new(rect.left() + cw * col + 3.0, rect.top() + hh + ch * r + 3.0),
                Vec2::new(cw - 6.0, ch - 6.0),
            );
            let resp = ui.interact(cell, egui::Id::new(("cell", i)), Sense::click());
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let selected = d == self.sel;
            let fill = if selected {
                p.accent
            } else if resp.hovered() {
                hover_c
            } else {
                card_c
            };
            let fg = if selected {
                p.sel_fg
            } else if d.month() != self.month {
                p.muted
            } else {
                p.fg
            };
            painter.rect_filled(cell, 10.0, fill);
            if d == self.today && !selected {
                painter.rect_stroke(cell, 10.0, Stroke::new(1.5, p.accent));
            }
            let cp = painter.with_clip_rect(cell);
            cp.text(
                Pos2::new(cell.left() + 8.0, cell.top() + 6.0),
                Align2::LEFT_TOP,
                d.day().to_string(),
                FontId::proportional(13.5),
                fg,
            );
            if self.data.notes.contains_key(&key(d)) {
                painter.circle_filled(
                    Pos2::new(cell.right() - 10.0, cell.top() + 13.0),
                    2.5,
                    if selected { p.sel_fg } else { p.magenta },
                );
            }
            let tasks = self.data.tasks_for(d);
            if !tasks.is_empty() {
                let mut items: Vec<(&Task, bool)> = tasks
                    .iter()
                    .map(|t| (*t, self.data.is_done(&t.id, d)))
                    .collect();
                items.sort_by_key(|x| x.1); // unfinished first
                let done = items.iter().filter(|x| x.1).count();
                let (line_h, top_pad, bottom_pad) = (13.0, 25.0, 9.0);
                let rows = (((cell.height() - top_pad - bottom_pad) / line_h).floor() as i64)
                    .clamp(0, 6) as usize;
                let shown = if items.len() <= rows {
                    items.len()
                } else {
                    rows.saturating_sub(1)
                };
                let font = FontId::proportional(10.5);
                let max_chars = (((cell.width() - 16.0) / 5.4).floor() as usize).max(4);
                for (k, (t, is_done)) in items.iter().take(shown).enumerate() {
                    let mut text: String = t.title.chars().take(max_chars).collect();
                    if t.title.chars().count() > max_chars {
                        text.pop();
                        text.push('\u{2026}');
                    }
                    let col = if *is_done { fg.gamma_multiply(0.55) } else { fg };
                    let pos = Pos2::new(cell.left() + 8.0, cell.top() + top_pad + k as f32 * line_h);
                    let r = cp.text(pos, Align2::LEFT_TOP, text, font.clone(), col);
                    if *is_done {
                        let y = r.center().y;
                        cp.line_segment(
                            [Pos2::new(r.left(), y), Pos2::new(r.right(), y)],
                            Stroke::new(1.0, col),
                        );
                    }
                }
                if items.len() > shown && rows > 0 {
                    let more = items.len() - shown;
                    cp.text(
                        Pos2::new(cell.left() + 8.0, cell.top() + top_pad + shown as f32 * line_h),
                        Align2::LEFT_TOP,
                        format!("+{} more", more),
                        font,
                        if selected { p.sel_fg } else { p.accent },
                    );
                }
                let bw = cell.width() - 16.0;
                let bar = Rect::from_min_size(
                    Pos2::new(cell.left() + 8.0, cell.bottom() - 6.0),
                    Vec2::new(bw, 2.5),
                );
                let (track, fillc) = if selected {
                    (p.sel_fg.gamma_multiply(0.3), p.sel_fg)
                } else {
                    (p.line, p.green)
                };
                painter.rect_filled(bar, 1.0, track);
                let w = bw * done as f32 / items.len() as f32;
                painter.rect_filled(Rect::from_min_size(bar.min, Vec2::new(w, 2.5)), 1.0, fillc);
            }
            if resp.clicked() {
                clicked = Some(d);
            }
        }
        if let Some(d) = clicked {
            self.select(d);
        }
    }

    // ---------- all notes
    fn notes_list(&mut self, ui: &mut egui::Ui) {
        let p = self.pal();
        ui.horizontal(|ui| {
            ui.heading("All notes");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.undo.is_some() && ui.button("Undo delete").clicked() {
                    self.undo_note();
                }
            });
        });
        ui.add_space(8.0);
        if self.data.notes.is_empty() {
            ui.label(cap(&p, "No notes yet. Press n to write one."));
            return;
        }
        let mut open: Option<String> = None;
        let mut del: Option<String> = None;
        ScrollArea::vertical()
            .id_source("notes")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (k, text) in self.data.notes.iter().rev() {
                    egui::Frame::none()
                        .fill(p.card)
                        .rounding(10.0)
                        .inner_margin(12.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let label = NaiveDate::parse_from_str(k, "%Y-%m-%d")
                                    .map(|d| d.format("%a, %b %-d %Y").to_string())
                                    .unwrap_or_else(|_| k.clone());
                                if ui.link(RichText::new(label).color(p.accent)).clicked() {
                                    open = Some(k.clone());
                                }
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button("×").on_hover_text("Delete note").clicked() {
                                        del = Some(k.clone());
                                    }
                                });
                            });
                            let preview = text
                                .lines()
                                .filter(|l| !l.trim().is_empty())
                                .take(2)
                                .map(|l| l.chars().take(120).collect::<String>())
                                .collect::<Vec<_>>()
                                .join("\n");
                            ui.label(preview);
                        });
                    ui.add_space(6.0);
                }
            });
        if let Some(k) = del {
            self.delete_note(&k);
        }
        if let Some(k) = open {
            if let Ok(d) = NaiveDate::parse_from_str(&k, "%Y-%m-%d") {
                self.select(d);
                self.view = View::Calendar;
            }
        }
    }

    // ---------- project page
    fn project_view(&mut self, ui: &mut egui::Ui, pid: &str) {
        let p = self.pal();
        let Some(pi) = self.data.projects.iter().position(|x| x.id == pid) else {
            self.view = View::Calendar;
            return;
        };
        let mut delete_it = false;
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.data.projects[pi].name)
                    .font(TextStyle::Heading)
                    .frame(false)
                    .hint_text("Project name")
                    .desired_width(320.0),
            );
            if r.changed() {
                self.mark();
            }
            if std::mem::take(&mut self.focus_name) {
                r.request_focus();
            }
            self.typing |= r.has_focus();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.confirm_project.as_deref() == Some(pid) {
                    if ui.button("Confirm delete").clicked() {
                        delete_it = true;
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_project = None;
                    }
                } else if ui.button("Delete project").clicked() {
                    self.confirm_project = Some(pid.to_string());
                }
            });
        });
        if delete_it {
            let ids: Vec<String> = self
                .data
                .tasks
                .iter()
                .filter(|t| t.project == pid)
                .map(|t| t.id.clone())
                .collect();
            for id in ids {
                self.purge_done(&id);
            }
            self.data.tasks.retain(|t| t.project != pid);
            self.data.projects.retain(|x| x.id != pid);
            self.confirm_project = None;
            self.view = View::Calendar;
            self.mark();
            return;
        }

        let sel_label = self.sel.format("%b %-d").to_string();
        ScrollArea::vertical()
            .id_source("proj")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.label(cap(&p, "TASKS"));
                let items: Vec<(String, String)> = self
                    .data
                    .tasks
                    .iter()
                    .filter(|t| t.project == pid)
                    .map(|t| {
                        (
                            t.id.clone(),
                            t.sd.map(key).unwrap_or_else(|| "-".to_string()),
                        )
                    })
                    .collect();
                if items.is_empty() {
                    ui.label(cap(&p, "No tasks yet. Press a to add one."));
                }
                for (id, suffix) in &items {
                    self.task_block(ui, id, suffix, true);
                }
                ui.add_space(6.0);
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.proj_title)
                        .hint_text("Add a task to this project")
                        .desired_width(f32::INFINITY),
                );
                if std::mem::take(&mut self.focus_ptask) {
                    resp.request_focus();
                }
                self.typing |= resp.has_focus();
                let mut add = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.proj_link, format!("Link to {}", sel_label));
                    add |= ui.button("Add").clicked();
                });
                if add {
                    self.add_project_task(pid);
                    resp.request_focus();
                }

                ui.add_space(14.0);
                ui.separator();
                let mut toggle_prev = false;
                ui.horizontal(|ui| {
                    ui.label(cap(&p, "NOTE"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let has = !self.data.projects[pi].note.trim().is_empty();
                        if has
                            && ui
                                .small_button(if self.proj_preview { "Edit" } else { "Preview" })
                                .clicked()
                        {
                            toggle_prev = true;
                        }
                    });
                });
                if toggle_prev {
                    self.proj_preview = !self.proj_preview;
                }
                if self.proj_preview && !self.data.projects[pi].note.trim().is_empty() {
                    egui::Frame::none()
                        .fill(p.card)
                        .rounding(10.0)
                        .inner_margin(12.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            render_md(ui, &self.data.projects[pi].note, &p);
                        });
                } else {
                    let r = ui.add(
                        egui::TextEdit::multiline(&mut self.data.projects[pi].note)
                            .hint_text("Project notes (markdown supported)")
                            .desired_rows(14)
                            .desired_width(f32::INFINITY),
                    );
                    self.typing |= r.has_focus();
                    if r.changed() {
                        self.mark();
                    }
                }
            });
    }

    // ---------- day side panel
    fn day_panel(&mut self, ui: &mut egui::Ui) {
        let p = self.pal();
        let sel = self.sel;
        let sk = key(sel);
        ui.label(cap(&p, sel.format("%A").to_string()));
        ui.heading(sel.format("%B %-d, %Y").to_string());
        let ids: Vec<String> = self.data.tasks_for(sel).iter().map(|t| t.id.clone()).collect();
        let done = ids
            .iter()
            .filter(|id| self.data.done.contains_key(&dk(id, &sk)))
            .count();
        ui.label(cap(
            &p,
            if ids.is_empty() {
                "Nothing planned".to_string()
            } else {
                format!("{} of {} done", done, ids.len())
            },
        ));
        if !self.rolled.is_empty() {
            ui.horizontal(|ui| {
                ui.label(cap(&p, format!("Rolled over {} task(s)", self.rolled.len())));
                if ui.small_button("Undo").clicked() {
                    self.undo_rollover();
                }
                if ui.small_button("×").on_hover_text("Dismiss").clicked() {
                    self.rolled.clear();
                }
            });
        }
        ui.add_space(4.0);

        let max_h = (ui.available_height() - 330.0).max(100.0);
        ScrollArea::vertical()
            .id_source("day-tasks")
            .max_height(max_h)
            .auto_shrink([true, true])
            .show(ui, |ui| {
                for id in &ids {
                    self.task_block(ui, id, &sk, false);
                }
            });

        if let Some(id) = self.confirm_delete.clone() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Delete from every day?").color(p.orange));
                if ui.button("Delete").clicked() {
                    self.delete_task(&id);
                    self.confirm_delete = None;
                }
                if ui.button("Cancel").clicked() {
                    self.confirm_delete = None;
                }
            });
        }

        // add task
        ui.add_space(6.0);
        let resp = ui.add(
            egui::TextEdit::singleline(&mut self.new_title)
                .hint_text("Add a task")
                .desired_width(f32::INFINITY),
        );
        if std::mem::take(&mut self.focus_task) {
            resp.request_focus();
        }
        self.typing |= resp.has_focus();
        let mut add = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_source("repeat")
                .selected_text(self.new_repeat.label())
                .show_ui(ui, |ui| {
                    for r in Repeat::ALL {
                        ui.selectable_value(&mut self.new_repeat, r, r.label());
                    }
                });
            add |= ui.button("Add").clicked();
        });
        if add {
            self.add_task();
            resp.request_focus();
        }

        // note
        ui.add_space(10.0);
        ui.separator();
        let mut delete_it = false;
        let mut undo_it = false;
        let mut toggle_prev = false;
        ui.horizontal(|ui| {
            ui.label(cap(&p, "NOTE"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !self.note_buf.is_empty() && ui.small_button("Delete").clicked() {
                    delete_it = true;
                }
                if self.undo.is_some() && ui.small_button("Undo").clicked() {
                    undo_it = true;
                }
                if !self.note_buf.trim().is_empty()
                    && ui
                        .small_button(if self.note_preview { "Edit" } else { "Preview" })
                        .clicked()
                {
                    toggle_prev = true;
                }
            });
        });
        if toggle_prev {
            self.note_preview = !self.note_preview;
        }
        if self.note_preview && !self.note_buf.trim().is_empty() {
            egui::Frame::none()
                .fill(p.card)
                .rounding(10.0)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ScrollArea::vertical()
                        .id_source("note-preview")
                        .max_height(240.0)
                        .show(ui, |ui| render_md(ui, &self.note_buf, &p));
                });
        } else {
            let r = ui.add(
                egui::TextEdit::multiline(&mut self.note_buf)
                    .hint_text("Write about your day (markdown supported)")
                    .desired_rows(9)
                    .desired_width(f32::INFINITY),
            );
            if std::mem::take(&mut self.focus_note) {
                r.request_focus();
            }
            self.typing |= r.has_focus();
            if r.changed() {
                self.commit_note();
            }
        }
        if delete_it {
            self.delete_note(&sk);
        }
        if undo_it {
            self.undo_note();
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Local::now().date_naive();
        if now != self.today {
            self.today = now;
            self.rollover();
        }
        self.bg_update(ctx);
        let typing = std::mem::take(&mut self.typing);
        if typing {
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                ctx.memory_mut(|m| {
                    if let Some(id) = m.focused() {
                        m.surrender_focus(id);
                    }
                });
            }
        } else {
            self.hotkeys(ctx);
        }

        let p = self.pal();
        let side = |fill: Color32| {
            egui::Frame::none()
                .fill(fill)
                .inner_margin(egui::Margin::same(14.0))
        };
        egui::SidePanel::left("nav")
            .exact_width(190.0)
            .resizable(false)
            .frame(side(p.panel))
            .show(ctx, |ui| self.nav(ui));
        egui::SidePanel::right("day")
            .exact_width(350.0)
            .resizable(false)
            .frame(side(p.panel))
            .show(ctx, |ui| self.day_panel(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::same(18.0)),
            )
            .show(ctx, |ui| match self.view.clone() {
                View::Calendar => self.calendar(ui),
                View::Notes => self.notes_list(ui),
                View::Project(id) => self.project_view(ui, &id),
            });

        if self.bg_open {
            let mut open = true;
            egui::Window::new("Backgrounds")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(440.0)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| self.bg_ui(ui));
            if !open {
                self.bg_open = false;
            }
        }

        if self.show_help {
            egui::Window::new("Keyboard shortcuts")
                .open(&mut self.show_help)
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::Grid::new("keys")
                        .num_columns(2)
                        .spacing([20.0, 6.0])
                        .show(ui, |ui| {
                            for &(k, d) in KEYS {
                                ui.label(RichText::new(k).monospace().color(p.accent));
                                ui.label(d);
                                ui.end_row();
                            }
                        });
                });
        }

        if self.dirty {
            let closing = ctx.input(|i| i.viewport().close_requested());
            if closing || self.last_edit.elapsed() > std::time::Duration::from_millis(700) {
                self.persist();
                self.dirty = false;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(750));
            }
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1140.0, 700.0])
            .with_min_inner_size([960.0, 640.0])
            .with_app_id("daily-planner"),
        ..Default::default()
    };
    eframe::run_native(
        "Daily Planner",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

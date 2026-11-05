#![allow(clippy::many_single_char_names)]

use std::f64::consts::{PI, TAU};
use std::io::{self, Write};
use crate::sys::{
    LocalTime, PyRandom, cpu_threads, disk_usage, hostname, kernel_release, loadavg, local_now,
    mem_stats, os_pretty_name, sha256_first_u32, uptime_seconds,
};

const W: usize = 57;
const H: usize = 25;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    Dim,
    Ink,
    Blue,
    Gold,
    Green,
    Rose,
}

impl Color {
    const fn ansi_rgb(self) -> &'static str {
        match self {
            Self::Dim => "106;119;143",
            Self::Ink => "219;226;235",
            Self::Blue => "127;178;240",
            Self::Gold => "227;191;126",
            Self::Green => "128;209;173",
            Self::Rose => "220;150;186",
        }
    }
}

struct Canvas {
    grid: [[(char, Color); W]; H],
}

impl Canvas {
    fn new() -> Self {
        Self { grid: [[(' ', Color::Ink); W]; H] }
    }

    fn text(&mut self, x: i32, y: i32, text: &str, color: Color) {
        let Ok(uy) = usize::try_from(y) else { return };
        if uy >= H {
            return;
        }
        for (i, ch) in text.chars().enumerate() {
            let cx = x + i32::try_from(i).unwrap_or(0);
            if let Ok(ux) = usize::try_from(cx)
                && ux < W
            {
                self.grid[uy][ux] = (ch, color);
            }
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, color: Color, ch: &str) {
        let n = (x2 - x1).abs().max((y2 - y1).abs()).max(1);
        for i in 0..=n {
            let t = f64::from(i) / f64::from(n);
            let rx = (f64::from(x1) + f64::from(x2 - x1) * t).round_ties_even() as i32;
            let ry = (f64::from(y1) + f64::from(y2 - y1) * t).round_ties_even() as i32;
            self.text(rx, ry, ch, color);
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn ring(&mut self, cx: i32, cy: i32, rx: f64, ry: f64, color: Color, ch: &str) {
        for i in 0..180 {
            let a = f64::from(i) * TAU / 180.0;
            let x = (f64::from(cx) + rx * a.cos()).round_ties_even() as i32;
            let y = (f64::from(cy) + ry * a.sin()).round_ties_even() as i32;
            self.text(x, y, ch, color);
        }
    }

    fn render(&self, ansi: bool) -> String {
        let mut out = String::with_capacity(H * (W + 32));
        for (row_idx, row) in self.grid.iter().enumerate() {
            if row_idx > 0 {
                out.push('\n');
            }
            let mut line = String::with_capacity(W + 32);
            let mut prev: Option<Color> = None;
            for &(ch, color) in row {
                if ansi && Some(color) != prev {
                    line.push_str("\x1b[38;2;");
                    line.push_str(color.ansi_rgb());
                    line.push('m');
                    prev = Some(color);
                }
                line.push(ch);
            }
            let trimmed = line.trim_end_matches(' ');
            out.push_str(trimmed);
            if ansi {
                out.push_str("\x1b[0m");
            }
        }
        out
    }
}

struct Metrics {
    mem: f64,
    ram: String,
    disk: f64,
    storage: String,
    up: String,
    load: f64,
    cores: usize,
    host: String,
    os: String,
    seed: u32,
    now: LocalTime,
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
fn pct(ratio: f64) -> String {
    format!("{}%", (ratio * 100.0).round_ties_even() as i64)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
fn gather_metrics() -> Metrics {
    let m = mem_stats();
    let mem_ratio = m.used_kib as f64 / m.total_kib as f64;
    let ram = format!(
        "{:.1}/{:.1} GiB",
        m.used_kib as f64 / 1_048_576.0,
        m.total_kib as f64 / 1_048_576.0
    );
    let d = disk_usage("/");
    let disk_ratio = d.used as f64 / d.total.max(1) as f64;
    let storage = format!(
        "{:.1}/{:.1} GiB",
        d.used as f64 / 1_073_741_824.0,
        d.total as f64 / 1_073_741_824.0
    );
    let uptime = uptime_seconds();
    let up_secs = uptime as u64;
    let up = format!("{}h {:02}m", up_secs / 3600, (up_secs % 3600) / 60);
    let load = loadavg()[0];
    let cores = cpu_threads();
    let host = hostname();
    let os = os_pretty_name();
    let seed = sha256_first_u32(host.as_bytes());
    let now = local_now();
    Metrics { mem: mem_ratio, ram, disk: disk_ratio, storage, up, load, cores, host, os, seed, now }
}

#[allow(clippy::cast_possible_truncation)]
fn potion(c: &mut Canvas, s: &Metrics) {
    c.text(2, 0, "A L C H E M Y  /  volatile memory", Color::Rose);
    c.text(9, 3, "╭────────╮", Color::Gold);
    c.text(9, 4, "│ ▒▒▒▒▒▒ │", Color::Gold);
    c.text(9, 5, "╰─┐    ┌─╯", Color::Gold);
    let walls: [(i32, i32); 12] = [
        (11, 16), (11, 16), (10, 17), (8, 19), (6, 21), (5, 22),
        (4, 23), (4, 23), (4, 23), (4, 23), (5, 22), (6, 21),
    ];
    let filled = (s.mem * 8.0).round_ties_even() as i32;
    for (i, &(l, r)) in walls.iter().enumerate() {
        let i_i32 = i32::try_from(i).unwrap_or(0);
        let y = i_i32 + 6;
        let sloped = (2..=6).contains(&i);
        c.text(l, y, if sloped { "╱" } else { "│" }, Color::Rose);
        c.text(r, y, if sloped { "╲" } else { "│" }, Color::Rose);
        if y >= 18 - filled {
            let fill_ch = if y == 18 - filled { "≈" } else { "░" };
            let count = usize::try_from((r - l - 1).max(0)).unwrap_or(0);
            c.text(l + 1, y, &fill_ch.repeat(count), Color::Blue);
            if y > 18 - filled && y % 2 == 0 {
                c.text(l + 3, y, "°  ·   °", Color::Blue);
            }
        }
    }
    c.text(6, 18, "╰──────────────╯", Color::Rose);
    c.text(29, 6, "MANA RESERVOIR", Color::Blue);
    c.text(29, 8, &format!("{} occupied", pct(s.mem)), Color::Ink);
    c.text(29, 9, &s.ram, Color::Dim);
    c.text(29, 12, "ROOT INVENTORY", Color::Gold);
    c.text(29, 14, &format!("{} occupied", pct(s.disk)), Color::Ink);
    c.text(29, 15, &s.storage, Color::Dim);
    c.text(3, 21, "Liquid height = memory in use", Color::Dim);
    c.text(3, 23, &format!("{}  /  awake {}", s.os, s.up), Color::Gold);
}

fn atlas(c: &mut Canvas, s: &Metrics) {
    c.text(2, 0, "D U N G E O N  /  machine cartography", Color::Gold);
    let mut rng = PyRandom::new(u64::from(s.seed));
    let mut rooms = Vec::with_capacity(6);
    let home_short: String = s.host.chars().take(9).collect();
    let labels = [
        ("BOOT", s.up.clone()),
        ("MEM", pct(s.mem)),
        ("ROOT", pct(s.disk)),
        ("CPU", format!("{}T", s.cores)),
        ("LOAD", format!("{:.2}", s.load)),
        ("HOME", home_short),
    ];
    for i in 0..6_i32 {
        let x = 2 + (i % 3) * 18;
        let y = 4 + (i / 3) * 10 + rng.randrange(0, 2);
        rooms.push((x, y));
    }
    for (a, b) in [(0, 1), (1, 2), (0, 3), (3, 4), (4, 5), (2, 5)] {
        let (x, y) = rooms[a];
        let (u, v) = rooms[b];
        c.line(x + 6, y + 2, u + 6, y + 2, Color::Dim, "▪");
        c.line(u + 6, y + 2, u + 6, v + 2, Color::Dim, "▪");
    }
    for (&(x, y), (label, value)) in rooms.iter().zip(labels.iter()) {
        c.text(x, y, "╭───────────╮", Color::Green);
        c.text(x, y + 1, "│           │", Color::Green);
        c.text(x, y + 2, "│           │", Color::Green);
        c.text(x, y + 3, "╰───────────╯", Color::Green);
        c.text(x + 2, y + 1, label, Color::Gold);
        c.text(x + 2, y + 2, value, Color::Ink);
    }
    c.text(3, 21, "@ Frieren     ▪ passage     HOME = destination", Color::Dim);
    c.text(3, 23, &format!("World seed {:08X} / {}", s.seed, s.os), Color::Gold);
    let (x0, y0) = rooms[0];
    c.text(x0 + 10, y0 + 1, "@", Color::Rose);
}

#[allow(clippy::cast_possible_truncation)]
fn orbit(c: &mut Canvas, s: &Metrics) {
    c.text(2, 0, "O R R E R Y  /  local time", Color::Blue);
    let (cx, cy) = (26, 11);
    c.ring(cx, cy, 19.0, 8.0, Color::Dim, "·");
    c.ring(cx, cy, 13.0, 6.0, Color::Dim, "·");
    for n in 0..12 {
        let a = f64::from(n) * TAU / 12.0 - PI / 2.0;
        let x = (f64::from(cx) + 19.0 * a.cos()).round_ties_even() as i32;
        let y = (f64::from(cy) + 8.0 * a.sin()).round_ties_even() as i32;
        let label = if n == 0 { String::from("12") } else { n.to_string() };
        c.text(x, y, &label, Color::Gold);
    }
    let now = &s.now;
    let hands = [
        (f64::from(now.hour % 12) + f64::from(now.minute) / 60.0, 12.0, 10.0, 4.0, Color::Gold, "━"),
        (f64::from(now.minute) + f64::from(now.second) / 60.0, 60.0, 16.0, 7.0, Color::Blue, "·"),
    ];
    for (value, period, rx, ry, color, ch) in hands {
        let a = value / period * TAU - PI / 2.0;
        let tx = (f64::from(cx) + rx * a.cos()).round_ties_even() as i32;
        let ty = (f64::from(cy) + ry * a.sin()).round_ties_even() as i32;
        c.line(cx, cy, tx, ty, color, ch);
    }
    c.text(cx, cy, "✦", Color::Rose);
    c.text(
        3,
        21,
        &format!(
            "{:04}.{:02}.{:02}    {:02}:{:02}:{:02}   /   {}",
            now.year, now.month, now.day, now.hour, now.minute, now.second, now.tzname
        ),
        Color::Ink,
    );
    c.text(
        3,
        23,
        &format!("UP {}   RAM {}   ROOT {}", s.up, pct(s.mem), pct(s.disk)),
        Color::Blue,
    );
}

#[allow(clippy::cast_possible_truncation, clippy::too_many_arguments)]
fn grove_branch(
    c: &mut Canvas,
    rng: &mut PyRandom,
    leaf: Color,
    x: f64,
    y: f64,
    length: f64,
    dx: f64,
    d: i32,
) {
    let nx = ((x + dx * length).round_ties_even() as i32).clamp(3, 52);
    let ny = ((y - length * 0.48).round_ties_even() as i32).max(3);
    let ch = if dx == 0.0 {
        "│"
    } else if dx < 0.0 {
        "╱"
    } else {
        "╲"
    };
    c.line(x.round_ties_even() as i32, y.round_ties_even() as i32, nx, ny, Color::Gold, ch);
    if d > 0 {
        let dl = -1.0 - rng.random() * 0.6;
        grove_branch(c, rng, leaf, f64::from(nx), f64::from(ny), length * 0.68, dl, d - 1);
        let dr = 1.0 + rng.random() * 0.6;
        grove_branch(c, rng, leaf, f64::from(nx), f64::from(ny), length * 0.68, dr, d - 1);
    } else {
        c.text(nx - 1, ny, "♣♧", leaf);
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn grove(c: &mut Canvas, s: &Metrics) {
    c.text(2, 0, "G R O V E  /  a living machine", Color::Green);
    let mut rng = PyRandom::new(u64::from(s.seed));
    let extra = ((s.load / s.cores as f64) * 5.0) as i32;
    let depth = 3 + extra.clamp(0, 2);
    let leaf = if s.mem < 0.6 {
        Color::Green
    } else if s.mem < 0.85 {
        Color::Gold
    } else {
        Color::Rose
    };

    grove_branch(c, &mut rng, leaf, 27.0, 18.0, 9.0, 0.0, depth);
    c.text(24, 18, "╱ │ ╲", Color::Gold);
    c.text(9, 19, "───────┴─────────┴─────────┴────────", Color::Dim);
    c.text(3, 21, &format!("CROWN  load {:.2} / {} threads", s.load, s.cores), Color::Green);
    c.text(3, 22, &format!("LEAVES memory {}   ROOT disk {}", pct(s.mem), pct(s.disk)), leaf);
    c.text(3, 24, "Growth follows load; leaf color follows memory.", Color::Dim);
}

fn stars(c: &mut Canvas, s: &Metrics) {
    c.text(2, 0, &format!("C O N S T E L L A T I O N  /  {}", s.host), Color::Blue);
    let mut rng = PyRandom::new(u64::from(s.seed));
    for _ in 0..45 {
        let sx = rng.randrange(1, 56);
        let sy = rng.randrange(2, 20);
        c.text(sx, sy, "·", Color::Dim);
    }
    let nodes: [(i32, i32); 5] = [(7, 6), (24, 3), (45, 7), (34, 13), (10, 17)];
    for (a, b) in [(0, 1), (1, 2), (2, 3), (3, 4), (4, 0), (0, 3)] {
        let (x1, y1) = nodes[a];
        let (x2, y2) = nodes[b];
        c.line(x1, y1, x2, y2, Color::Blue, "·");
    }
    let krel = kernel_release();
    let kshort = krel.split('-').next().unwrap_or(&krel).to_owned();
    let labels = [
        ("KERNEL", kshort),
        ("CPU", format!("{} threads", s.cores)),
        ("MEM", pct(s.mem)),
        ("ROOT", pct(s.disk)),
        ("UP", s.up.clone()),
    ];
    for (&(x, y), (name, val)) in nodes.iter().zip(labels.iter()) {
        c.text(x, y, "✦", Color::Gold);
        c.text(x - 2, y - 1, name, Color::Gold);
        c.text(x + 2, y + 1, val, Color::Ink);
    }
    c.text(3, 22, &format!("{} / {}", s.os, s.host), Color::Ink);
    c.text(3, 24, "Each point is a piece of this little world.", Color::Dim);
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mut scene: Option<&str> = None;
    let mut plain = false;
    for arg in args {
        if arg == "--plain" {
            plain = true;
        } else if !arg.starts_with('-') && scene.is_none() {
            scene = Some(arg.as_str());
        }
    }
    let Some(scene_name) = scene else {
        return Err(String::from("usage: scenes <potion|dungeon|orrery|grove|constellation> [--plain]"));
    };
    let mut canvas = Canvas::new();
    let metrics = gather_metrics();
    match scene_name {
        "potion" => potion(&mut canvas, &metrics),
        "dungeon" => atlas(&mut canvas, &metrics),
        "orrery" => orbit(&mut canvas, &metrics),
        "grove" => grove(&mut canvas, &metrics),
        "constellation" => stars(&mut canvas, &metrics),
        other => return Err(format!("unknown scene: {other}")),
    }
    let out = canvas.render(!plain);
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{out}");
    Ok(())
}

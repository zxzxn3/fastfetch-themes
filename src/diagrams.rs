#![allow(clippy::many_single_char_names, clippy::too_many_lines)]

use std::collections::HashMap;
use std::env;
use std::f64::consts::{PI, TAU};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use crate::json::{JsonVal, parse_json};
use crate::sys::{
    PyRandom, battery_summary, current_uid, disk_usage, hostname, kernel_arch, kernel_release,
    loadavg, local_now, mem_stats, os_pretty_name, proc_task_count, read_trimmed, root_mount_info,
    uptime_seconds,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    Fg,
    Dim,
    Blue,
    Green,
    Gold,
    Pink,
    Paper,
    Wood,
    ScrollGold,
    Faded,
}

impl Color {
    const fn ansi_rgb(self) -> &'static str {
        match self {
            Self::Fg => "214;224;238",
            Self::Dim => "100;120;145",
            Self::Blue => "111;183;246",
            Self::Green => "116;209;174",
            Self::Gold => "237;191;117",
            Self::Pink => "218;144;196",
            Self::Paper => "221;205;171",
            Self::Wood => "160;119;74",
            Self::ScrollGold => "204;168;105",
            Self::Faded => "155;143;119",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CellChar {
    Char(char),
    Continuation,
}

fn is_east_asian_wide(ch: char) -> bool {
    let u = ch as u32;
    matches!(
        u,
        0x1100..=0x115F
            | 0x231A..=0x231B
            | 0x2329..=0x232A
            | 0x23E9..=0x23EC
            | 0x23F0
            | 0x23F3
            | 0x25FD..=0x25FE
            | 0x2614..=0x2615
            | 0x2648..=0x2653
            | 0x267F
            | 0x2693
            | 0x26A1
            | 0x26AA..=0x26AB
            | 0x26BD..=0x26BE
            | 0x26C4..=0x26C5
            | 0x26CE
            | 0x26D4
            | 0x26EA
            | 0x26F2..=0x26F3
            | 0x26F5
            | 0x26FA
            | 0x26FD
            | 0x2705
            | 0x270A..=0x270B
            | 0x2728
            | 0x274C
            | 0x274E
            | 0x2753..=0x2755
            | 0x2757
            | 0x2795..=0x2797
            | 0x27B0
            | 0x27BF
            | 0x2B1B..=0x2B1C
            | 0x2B50
            | 0x2B55
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0xA4C6
            | 0xA960..=0xA97C
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE19
            | 0xFE30..=0xFE6B
            | 0xFF01..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F004
            | 0x1F0CF
            | 0x1F18E
            | 0x1F191..=0x1F19A
            | 0x1F200..=0x1F251
            | 0x1F300..=0x1F64F
            | 0x1F680..=0x1F6FF
            | 0x1F900..=0x1FAFF
            | 0x20000..=0x3FFFD
    )
}

fn clean(value: &str) -> String {
    value
        .chars()
        .filter(|&c| {
            let u = c as u32;
            !(u <= 0x1f || (0x7f..=0x9f).contains(&u))
        })
        .collect()
}

fn fit(text: &str, n: usize) -> String {
    let cleaned = clean(text);
    let count = cleaned.chars().count();
    if count <= n {
        cleaned
    } else if n == 0 {
        String::from("…")
    } else {
        let mut s: String = cleaned.chars().take(n - 1).collect();
        s.push('…');
        s
    }
}

struct Canvas {
    w: usize,
    h: usize,
    cells: Vec<(CellChar, Color)>,
    edges: Vec<u8>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            cells: vec![(CellChar::Char(' '), Color::Fg); w * h],
            edges: vec![0; w * h],
        }
    }

    fn put(&mut self, mut x: i32, y: i32, text: &str, color: Color) {
        let Ok(uy) = usize::try_from(y) else { return };
        for ch in clean(text).chars() {
            let width = if is_east_asian_wide(ch) { 2_i32 } else { 1_i32 };
            if uy < self.h && x >= 0 {
                let ux = usize::try_from(x).unwrap_or(self.w);
                let uw = usize::try_from(width).unwrap_or(1);
                if ux + uw <= self.w {
                    let idx = uy * self.w + ux;
                    self.cells[idx] = (CellChar::Char(ch), color);
                    if uw == 2 {
                        self.cells[idx + 1] = (CellChar::Continuation, color);
                    }
                }
            }
            x += width;
        }
    }

    fn wire(&mut self, points: &[(i32, i32)], color: Color) {
        const GLYPH: [char; 16] = [
            ' ', '│', '─', '└', '│', '│', '┌', '├', '─', '┘', '─', '┴', '┐', '┤', '┬', '┼',
        ];
        for pair in points.windows(2) {
            let (mut x, mut y) = pair[0];
            let (u, v) = pair[1];
            let dx = i32::from(u > x) - i32::from(u < x);
            let dy = i32::from(v > y) - i32::from(v < y);
            while (x, y) != (u, v) {
                let (nx, ny) = (x + dx, y + dy);
                let (a, b): (u8, u8) = if dx == 1 {
                    (2, 8)
                } else if dx == -1 {
                    (8, 2)
                } else if dy == 1 {
                    (4, 1)
                } else {
                    (1, 4)
                };
                for (xx, yy, bits) in [(x, y, a), (nx, ny, b)] {
                    if let (Ok(ux), Ok(uy)) = (usize::try_from(xx), usize::try_from(yy))
                        && ux < self.w
                        && uy < self.h
                    {
                        let idx = uy * self.w + ux;
                        self.edges[idx] |= bits;
                        let g = GLYPH[usize::from(self.edges[idx] & 0x0f)];
                        self.cells[idx] = (CellChar::Char(g), color);
                    }
                }
                x = nx;
                y = ny;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn box_draw(&mut self, x: i32, y: i32, w: i32, h: i32, title: &str, lines: &[String], color: Color) {
        let blank_w = usize::try_from((w - 2).max(0)).unwrap_or(0);
        let blanks = " ".repeat(blank_w);
        for yy in (y + 1)..(y + h - 1) {
            self.put(x + 1, yy, &blanks, Color::Fg);
        }
        self.wire(
            &[(x, y), (x + w - 1, y), (x + w - 1, y + h - 1), (x, y + h - 1), (x, y)],
            color,
        );
        let title_fit = fit(title, usize::try_from((w - 6).max(0)).unwrap_or(0));
        self.put(x + 2, y, &format!(" {title_fit} "), color);
        let max_lines = usize::try_from((h - 2).max(0)).unwrap_or(0);
        let line_w = usize::try_from((w - 4).max(0)).unwrap_or(0);
        for (i, line) in lines.iter().take(max_lines).enumerate() {
            let row_y = y + 1 + i32::try_from(i).unwrap_or(0);
            self.put(x + 2, row_y, &fit(line, line_w), Color::Fg);
        }
    }

    fn render(&self, ansi: bool) -> String {
        let mut out = String::with_capacity(self.h * (self.w + 32));
        for row_idx in 0..self.h {
            if row_idx > 0 {
                out.push('\n');
            }
            let row = &self.cells[row_idx * self.w..(row_idx + 1) * self.w];
            let mut end = row.len();
            while end > 0 && row[end - 1].0 == CellChar::Char(' ') {
                end -= 1;
            }
            let mut prev: Option<Color> = None;
            for &(cell_ch, color) in &row[..end] {
                if ansi && Some(color) != prev {
                    out.push_str("\x1b[38;2;");
                    out.push_str(color.ansi_rgb());
                    out.push('m');
                    prev = Some(color);
                }
                if let CellChar::Char(ch) = cell_ch {
                    out.push(ch);
                }
            }
            if ansi {
                out.push_str("\x1b[0m");
            }
        }
        out
    }
}

#[allow(clippy::cast_precision_loss)]
fn gib(n: u64) -> String {
    format!("{:.1}G", n as f64 / 1_073_741_824.0)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn bar(r: f64, n: usize) -> String {
    let clamped = r.clamp(0.0, 1.0);
    let filled = ((clamped * f64::from(u32::try_from(n).unwrap_or(15))).round_ties_even() as usize).min(n);
    format!("{}{}", "━".repeat(filled), "·".repeat(n - filled))
}

#[allow(clippy::cast_possible_truncation)]
fn pct(r: f64) -> String {
    format!("{}%", (r * 100.0).round_ties_even() as i64)
}

fn fmt_g(v: f64) -> String {
    if v == 0.0 {
        return String::from("0");
    }
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        String::from("0")
    } else {
        s.to_owned()
    }
}

struct Snapshot {
    host: String,
    os: String,
    kernel: String,
    arch: String,
    model: String,
    cpu: String,
    cores: String,
    freq: String,
    march: String,
    gpus: Vec<[String; 3]>,
    mem: String,
    mr: f64,
    swap: String,
    disk: String,
    dr: f64,
    fs: String,
    dev: String,
    free: String,
    wm: String,
    protocol: String,
    shell: String,
    terminal: String,
    packages: String,
    uptime: String,
    load: String,
    displays: Vec<[String; 3]>,
    battery: String,
    date: String,
    tasks: usize,
}

const FASTFETCH_STATIC_MODS: &str = "OS:Host:Kernel:CPU:GPU:Display:WM:Terminal:Packages";
const CACHE_TTL_SECS: u64 = 60;

fn cache_file_path() -> PathBuf {
    if let Ok(runtime_dir) = env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join("fastfetch-themes-hw.json")
    } else {
        PathBuf::from(format!("/tmp/fastfetch-themes-hw-{}.json", current_uid()))
    }
}

pub fn refresh_cache_sync() -> Option<String> {
    let output = Command::new("fastfetch")
        .args(["-c", "none", "--format", "json", "--structure", FASTFETCH_STATIC_MODS])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let json_text = String::from_utf8(output.stdout).ok()?;
    let path = cache_file_path();
    let tmp_path = path.with_extension(format!("tmp.{}", std::process::id()));
    if fs::write(&tmp_path, &json_text).is_ok() {
        let _ = fs::rename(&tmp_path, &path);
    }
    Some(json_text)
}

fn load_or_refresh_static_json() -> Option<JsonVal> {
    let path = cache_file_path();
    if let Ok(meta) = fs::metadata(&path)
        && let Ok(content) = fs::read_to_string(&path)
        && let Some(parsed) = parse_json(&content)
    {
        let is_stale = meta
            .modified()
            .ok()
            .and_then(|m| SystemTime::now().duration_since(m).ok())
            .is_none_or(|age| age > Duration::from_secs(CACHE_TTL_SECS));
        if is_stale && let Ok(exe) = env::current_exe() {
            let _ = Command::new(exe)
                .arg("--refresh-cache")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
        return Some(parsed);
    }
    let fresh = refresh_cache_sync()?;
    parse_json(&fresh)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
fn snapshot() -> Snapshot {
    let mut data: HashMap<String, JsonVal> = HashMap::new();
    if let Some(JsonVal::Arr(items)) = load_or_refresh_static_json() {
        for item in items {
            if let Some(t) = item.get("type").as_str() {
                data.insert(t.to_ascii_lowercase(), item.get("result").clone());
            }
        }
    }
    let get_obj = |k: &str| -> &JsonVal {
        static NULL: JsonVal = JsonVal::Null;
        data.get(k).unwrap_or(&NULL)
    };

    let cpu_obj = get_obj("cpu");
    let kern_obj = get_obj("kernel");
    let wm_obj = get_obj("wm");
    let os_obj = get_obj("os");
    let host_obj = get_obj("host");
    let term_obj = get_obj("terminal");
    let pkg_obj = get_obj("packages");

    let cores_obj = cpu_obj.get("cores");
    let freq_obj = cpu_obj.get("frequency");

    let mut displays = Vec::new();
    if let Some(arr) = get_obj("display").as_arr() {
        for d in arr {
            let out = d.get("output");
            let scaled = d.get("scaled");
            let name = d.get("name").str_or("?").to_owned();
            let w = out.get("width").i64_or(0);
            let h = out.get("height").i64_or(0);
            let rate = fmt_g(out.get("refreshRate").f64_or(0.0));
            let sw = scaled.get("width").i64_or(0);
            let sh = scaled.get("height").i64_or(0);
            displays.push([
                name,
                format!("{w}x{h} @ {rate}Hz"),
                format!("logical {sw}x{sh}"),
            ]);
        }
    }

    let mut gpus = Vec::new();
    if let Some(arr) = get_obj("gpu").as_arr() {
        for g in arr {
            let name = g
                .get("name")
                .str_or("?")
                .replace("GeForce ", "")
                .replace(" Max-Q / Mobile", " Mobile");
            let gtype = g.get("type").str_or("?").to_owned();
            let driver = g.get("driver").str_or("?").to_owned();
            gpus.push([name, gtype, driver]);
        }
    }

    // Live kernel metrics (~0.1 ms)
    let m = mem_stats();
    let mtotal = m.total_kib * 1024;
    let mused = m.used_kib * 1024;
    let swaptotal = m.swap_total_kib * 1024;
    let swapused = m.swap_used_kib * 1024;

    let d = disk_usage("/");
    let (dev, fs_name) = root_mount_info();

    let up_secs = uptime_seconds() as u64;
    let loads = loadavg();
    let now = local_now();

    let os_str = os_obj
        .get("prettyName")
        .as_str()
        .map_or_else(os_pretty_name, ToOwned::to_owned);
    let kernel_str = kern_obj
        .get("release")
        .as_str()
        .map_or_else(kernel_release, ToOwned::to_owned);
    let arch_str = kern_obj
        .get("architecture")
        .as_str()
        .map_or_else(kernel_arch, ToOwned::to_owned);
    let model_raw = host_obj
        .get("name")
        .as_str()
        .map(ToOwned::to_owned)
        .or_else(|| read_trimmed("/sys/devices/virtual/dmi/id/product_name"))
        .unwrap_or_else(|| String::from("?"));
    let model = model_raw.replace("OMEN by HP Transcend ", "OMEN ");

    let cpu_name = cpu_obj
        .get("cpu")
        .str_or("?")
        .replace("(R)", "")
        .replace("(TM)", "");
    let phys_cores = cores_obj
        .get("physical")
        .as_f64()
        .map_or_else(|| String::from("?"), |v| (v as i64).to_string());
    let log_cores = cores_obj
        .get("logical")
        .as_f64()
        .map_or_else(|| String::from("?"), |v| (v as i64).to_string());

    let fbase = fmt_g(freq_obj.get("base").f64_or(0.0) / 1000.0);
    let fmax = fmt_g(freq_obj.get("max").f64_or(0.0) / 1000.0);

    let wm_str = format!(
        "{} {}",
        wm_obj.get("prettyName").str_or("?"),
        wm_obj.get("version").str_or("")
    )
    .trim()
    .to_owned();

    let term_str = format!(
        "{} {}",
        term_obj.get("prettyName").str_or("?"),
        term_obj.get("version").str_or("")
    )
    .trim()
    .to_owned();

    let shell_env = env::var("SHELL").unwrap_or_else(|_| String::from("unknown"));
    let shell_name = Path::new(&shell_env)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_owned();

    let packages = pkg_obj
        .get("all")
        .as_f64()
        .map_or_else(|| String::from("?"), |v| (v as i64).to_string());

    Snapshot {
        host: hostname(),
        os: os_str,
        kernel: kernel_str,
        arch: arch_str,
        model,
        cpu: cpu_name,
        cores: format!("{phys_cores}C / {log_cores}T"),
        freq: format!("{fbase} / {fmax} GHz base/max"),
        march: cpu_obj.get("march").str_or("?").to_owned(),
        gpus,
        mem: format!("{} / {}", gib(mused), gib(mtotal)),
        mr: mused as f64 / mtotal.max(1) as f64,
        swap: format!("{} / {}", gib(swapused), gib(swaptotal)),
        disk: format!("{} / {}", gib(d.used), gib(d.total)),
        dr: d.used as f64 / d.total.max(1) as f64,
        fs: fs_name,
        dev,
        free: gib(d.available),
        wm: wm_str,
        protocol: wm_obj.get("protocolName").str_or("?").to_owned(),
        shell: shell_name,
        terminal: term_str,
        packages,
        uptime: format!("{}d {:02}h {:02}m", up_secs / 86400, (up_secs % 86400) / 3600, (up_secs % 3600) / 60),
        load: format!("{:.2} / {:.2} / {:.2}", loads[0], loads[1], loads[2]),
        displays,
        battery: battery_summary(),
        date: format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            now.year, now.month, now.day, now.hour, now.minute, now.second
        ),
        tasks: proc_task_count(),
    }
}

fn header(c: &mut Canvas, s: &Snapshot, title: &str, seed: u32) {
    c.put(1, 0, title, Color::Blue);
    c.put(1, 1, &format!("{}  /  {}  /  {}", s.host, s.os, s.date), Color::Dim);
    let last_y = i32::try_from(c.h.saturating_sub(1)).unwrap_or(0);
    c.put(
        1,
        last_y,
        &format!("logical schematic  ·  seed {seed:08x}  ·  local snapshot"),
        Color::Dim,
    );
}

fn details(s: &Snapshot) -> (Vec<[String; 3]>, Vec<[String; 3]>) {
    let gpu = if s.gpus.is_empty() {
        vec![[String::from("none detected"), String::from("?"), String::from("?")]]
    } else {
        s.gpus.clone()
    };
    let displays = if s.displays.is_empty() {
        vec![[String::from("none detected"), String::from("?"), String::from("?")]]
    } else {
        s.displays.clone()
    };
    (gpu, displays)
}

fn circuit(c: &mut Canvas, s: &Snapshot, rng: &mut PyRandom, seed: u32) {
    header(c, s, "01 / CIRCUIT ATLAS                         SYSTEM INTERCONNECT", seed);
    c.box_draw(
        1,
        3,
        74,
        5,
        "MACHINE / CONTROL PLANE",
        &[
            s.model.clone(),
            format!("{}  [{}]", s.kernel, s.arch),
            format!("UP {}    PACKAGES {}    TASKS {}", s.uptime, s.packages, s.tasks),
        ],
        Color::Gold,
    );
    c.wire(&[(37, 7), (37, 9), (18, 9), (18, 11)], Color::Blue);
    c.wire(&[(37, 9), (56, 9), (56, 11)], Color::Green);
    c.box_draw(
        1,
        11,
        35,
        6,
        "PROCESSOR",
        &[
            s.cpu.clone(),
            format!("{}  {}", s.cores, s.march),
            s.freq.clone(),
            format!("load {}", s.load),
        ],
        Color::Blue,
    );
    c.box_draw(
        40,
        11,
        35,
        6,
        "MEMORY CONTROLLER",
        &[
            s.mem.clone(),
            format!("{} {}", bar(s.mr, 20), pct(s.mr)),
            format!("swap {}", s.swap),
            format!("available {}", pct(1.0 - s.mr)),
        ],
        Color::Green,
    );
    c.wire(&[(18, 16), (18, 18), (69, 18), (69, 20)], Color::Gold);
    c.wire(&[(56, 16), (56, 18)], Color::Gold);
    c.wire(&[(37, 18), (37, 20)], Color::Gold);
    c.wire(&[(10, 18), (18, 18)], Color::Gold);
    c.wire(&[(10, 18), (10, 20)], Color::Gold);

    let (gpu, displays) = details(s);
    let mut gpu_lines: Vec<String> = gpu.iter().map(|g| g[0].clone()).collect();
    gpu_lines.push(gpu[0][2].clone());

    let mut cards = [
        ("GRAPHICS", gpu_lines, Color::Pink),
        (
            "STORAGE",
            vec![
                s.dev.clone(),
                format!("{}   {}", s.fs, s.disk),
                format!("available {}", s.free),
            ],
            Color::Gold,
        ),
        (
            "DISPLAY",
            vec![
                displays[0][0].clone(),
                displays[0][1].clone(),
                format!("{} outputs", s.displays.len()),
            ],
            Color::Blue,
        ),
    ];
    rng.shuffle(&mut cards);
    for (x, (title, lines, color)) in [1, 26, 51].into_iter().zip(cards.iter()) {
        c.box_draw(x, 20, 24, 6, title, lines, *color);
    }
    c.wire(&[(12, 25), (12, 27), (63, 27), (63, 25)], Color::Dim);
    c.wire(&[(37, 25), (37, 29)], Color::Dim);
    c.box_draw(
        1,
        29,
        74,
        4,
        "USER SPACE",
        &[
            format!("{} / {}     shell {}     {}", s.wm, s.protocol, s.shell, s.terminal),
            format!("POWER {}     ROOT {} used", s.battery, pct(s.dr)),
        ],
        Color::Green,
    );
}

fn metro(c: &mut Canvas, s: &Snapshot, rng: &mut PyRandom, seed: u32) {
    header(c, s, "02 / METROPOLIS                           SERVICE TRANSIT MAP", seed);
    let upper = *rng.choice(&[37, 38, 39, 40, 41]);
    let lower = *rng.choice(&[49, 50, 51, 52]);
    for (y, color) in [(5, Color::Blue), (13, Color::Green), (21, Color::Pink)] {
        c.wire(&[(3, y), (74, y)], color);
    }
    c.wire(&[(upper, 5), (upper, 10), (lower, 10), (lower, 21)], Color::Gold);
    for x in [1, 76] {
        let inner = if x == 1 { 3 } else { 74 };
        c.wire(&[(inner, 5), (x, 5), (x, 21), (inner, 21)], Color::Dim);
    }
    for (x, y) in [(upper, 5), (lower, 13), (lower, 21)] {
        c.put(x, y, "◎", Color::Gold);
    }
    c.put(3, 3, "A / COMPUTE", Color::Blue);
    c.put(3, 4, &s.cpu, Color::Blue);
    c.put(3, 6, "● CPU", Color::Fg);
    c.put(3, 7, &s.cores, Color::Fg);
    c.put(3, 8, &s.freq, Color::Fg);
    c.put(44, 6, "● KERNEL", Color::Fg);
    c.put(44, 7, &s.kernel, Color::Fg);
    c.put(44, 8, &format!("{} / {}", s.arch, s.march), Color::Fg);
    c.put(3, 11, "B / RESOURCES", Color::Green);
    c.put(3, 12, "memory / storage / virtual memory", Color::Green);

    let res_cols = [
        (3, "● RAM", [s.mem.clone(), format!("{} {}", bar(s.mr, 15), pct(s.mr))]),
        (29, "● ROOT", [s.disk.clone(), format!("{} {}", s.fs, pct(s.dr))]),
        (54, "● SWAP", [s.swap.clone(), format!("free root {}", s.free)]),
    ];
    for (x, title, rows) in &res_cols {
        c.put(*x, 14, title, Color::Fg);
        c.put(*x, 15, &rows[0], Color::Fg);
        c.put(*x, 16, &rows[1], Color::Fg);
    }

    let (gpu, displays) = details(s);
    c.put(3, 19, "C / VISUAL SESSION", Color::Pink);
    c.put(3, 20, &format!("{} / {}", s.wm, s.protocol), Color::Pink);
    c.put(3, 22, "● GPU 0", Color::Fg);
    c.put(3, 23, &fit(&gpu[0][0], 33), Color::Fg);
    c.put(3, 24, &fit(&gpu[0][2], 33), Color::Dim);
    c.put(42, 22, "● DISPLAY", Color::Fg);
    c.put(42, 23, &format!("{} {}", displays[0][0], displays[0][1]), Color::Fg);
    c.put(
        42,
        24,
        &format!("{} outputs / {} adapters", s.displays.len(), s.gpus.len()),
        Color::Dim,
    );

    c.box_draw(
        1,
        27,
        37,
        6,
        "OPERATIONS",
        &[
            format!("uptime    {}", s.uptime),
            format!("packages  {}", s.packages),
            format!("tasks     {}", s.tasks),
            format!("load      {}", s.load),
        ],
        Color::Gold,
    );
    let last_disp = &displays[displays.len() - 1];
    c.box_draw(
        40,
        27,
        37,
        6,
        "TERMINUS",
        &[
            s.terminal.clone(),
            format!("shell {}", s.shell),
            s.battery.clone(),
            format!("{} {}", last_disp[0], last_disp[1]),
        ],
        Color::Pink,
    );
    c.put(2, 34, "● station  ◎ interchange    A compute / B resources / C session", Color::Dim);
}

fn strata(c: &mut Canvas, s: &Snapshot, _rng: &mut PyRandom, seed: u32) {
    header(c, s, "03 / STRATA                               MACHINE CROSS-SECTION", seed);
    let (gpu, displays) = details(s);
    let levels = [
        (
            "SESSION",
            format!("{} / {}", s.wm, s.protocol),
            format!("{}  |  shell {}", s.terminal, s.shell),
            Color::Pink,
        ),
        (
            "SOFTWARE",
            format!("{} / {}", s.os, s.arch),
            format!("{} packages  |  {} processes", s.packages, s.tasks),
            Color::Blue,
        ),
        (
            "KERNEL",
            s.kernel.clone(),
            format!("uptime {}  |  load {}", s.uptime, s.load),
            Color::Gold,
        ),
        (
            "COMPUTE",
            s.cpu.clone(),
            format!("{}  |  {}", s.cores, s.freq),
            Color::Blue,
        ),
        (
            "MEMORY",
            format!("{}  {} {}", s.mem, bar(s.mr, 20), pct(s.mr)),
            format!("swap {}", s.swap),
            Color::Green,
        ),
        (
            "PERSISTENCE",
            format!("{}  /  {}", s.dev, s.fs),
            format!("{}  {} {}", s.disk, bar(s.dr, 20), pct(s.dr)),
            Color::Gold,
        ),
    ];
    let indents = [0, 2, 4, 2, 0, 2];
    for (i, (name, a, b, color)) in levels.iter().enumerate() {
        let i_i32 = i32::try_from(i).unwrap_or(0);
        let y = 3 + i_i32 * 4;
        let indent = indents[i];
        let x = 1 + indent;
        c.box_draw(
            x,
            y,
            70 - indent,
            4,
            &format!("{i:02} / {name}"),
            &[a.clone(), b.clone()],
            *color,
        );
        c.wire(&[(71, y + 1), (74, y + 1), (74, y + 4)], Color::Dim);
        c.put(72, y + 2, "↓", Color::Dim);
    }
    let mut gpu_lines: Vec<String> = gpu.iter().map(|g| g[0].clone()).collect();
    gpu_lines.push(gpu[0][2].clone());
    c.box_draw(1, 28, 36, 6, "GRAPHICS SIDECAR", &gpu_lines, Color::Pink);
    let last_disp = &displays[displays.len() - 1];
    c.box_draw(
        39,
        28,
        36,
        6,
        "OUTPUT / POWER",
        &[
            displays[0][1].clone(),
            last_disp[1].clone(),
            s.battery.clone(),
            format!("{} connected displays", s.displays.len()),
        ],
        Color::Green,
    );
    c.put(2, 35, "▲ interaction     ▼ abstraction     sidecar: visual pipeline", Color::Dim);
}

#[allow(clippy::cast_possible_truncation)]
fn radar(c: &mut Canvas, s: &Snapshot, rng: &mut PyRandom, seed: u32) {
    header(c, s, "04 / OBSERVATORY                          RADIAL SYSTEM INDEX", seed);
    let (cx, cy) = (38, 16);
    for (rx, ry) in [(12.0_f64, 5.0_f64), (22.0_f64, 9.0_f64)] {
        for i in 0..200 {
            let a = f64::from(i) * TAU / 200.0;
            let px = (f64::from(cx) + rx * a.cos()).round_ties_even() as i32;
            let py = (f64::from(cy) + ry * a.sin()).round_ties_even() as i32;
            c.put(px, py, "·", Color::Dim);
        }
    }
    let angles: Vec<i32> = (0..12).map(|i| i * 30).collect();
    for a in rng.sample(&angles, 5) {
        let rad = f64::from(a) * PI / 180.0;
        for t in 4..20 {
            let px = (f64::from(cx) + f64::from(t) * rad.cos()).round_ties_even() as i32;
            let py = (f64::from(cy) + f64::from(t) * 0.4 * rad.sin()).round_ties_even() as i32;
            c.put(px, py, "·", Color::Blue);
        }
    }
    c.box_draw(
        27,
        14,
        24,
        5,
        "SYSTEM CORE",
        &[s.host.clone(), s.cores.clone(), s.uptime.clone()],
        Color::Gold,
    );
    for points in [
        &[(16, 7), (24, 7), (24, 16), (27, 16)][..],
        &[(61, 7), (53, 7), (53, 16), (50, 16)][..],
        &[(16, 25), (24, 25), (24, 17)][..],
        &[(61, 25), (53, 25), (53, 17)][..],
    ] {
        c.wire(points, Color::Blue);
    }
    c.box_draw(
        1,
        3,
        30,
        6,
        "COMPUTE",
        &[s.cpu.clone(), s.freq.clone(), format!("load {}", s.load), s.march.clone()],
        Color::Blue,
    );
    c.box_draw(
        47,
        3,
        30,
        6,
        "MEMORY",
        &[
            s.mem.clone(),
            format!("{} {}", bar(s.mr, 18), pct(s.mr)),
            format!("swap {}", s.swap),
            format!("{} processes", s.tasks),
        ],
        Color::Green,
    );
    c.box_draw(
        1,
        23,
        30,
        6,
        "STORAGE",
        &[
            s.dev.clone(),
            format!("{} {}", s.fs, s.disk),
            format!("{} {}", bar(s.dr, 18), pct(s.dr)),
            format!("available {}", s.free),
        ],
        Color::Gold,
    );
    let (gpu, displays) = details(s);
    c.box_draw(
        47,
        23,
        30,
        6,
        "VISUAL",
        &[
            gpu[0][0].clone(),
            format!("{} GPU / {} screens", s.gpus.len(), s.displays.len()),
            displays[0][1].clone(),
            s.protocol.clone(),
        ],
        Color::Pink,
    );
    c.put(2, 11, "KERNEL", Color::Blue);
    c.put(2, 12, &fit(&s.kernel, 24), Color::Fg);
    c.put(54, 11, "SESSION", Color::Pink);
    c.put(54, 12, &fit(&s.wm, 23), Color::Fg);
    c.put(2, 19, &format!("PACKAGES {}", s.packages), Color::Gold);
    c.put(54, 19, &format!("SHELL {}", s.shell), Color::Green);
    c.put(2, 31, &format!("{}  |  power {}", s.terminal, s.battery), Color::Fg);
    c.put(2, 32, &s.model, Color::Dim);
}

fn fabric(c: &mut Canvas, s: &Snapshot, rng: &mut PyRandom, seed: u32) {
    header(c, s, "05 / FABRIC                               PROCEDURAL NODE GRAPH", seed);
    let (gpu, displays) = details(s);
    let mut gpu_lines: Vec<String> = gpu.iter().map(|g| g[0].clone()).collect();
    gpu_lines.push(gpu[0][2].clone());
    let last_disp = &displays[displays.len() - 1];
    let mut cards = [
        ("CPU", vec![s.cpu.clone(), s.cores.clone(), s.freq.clone()], Color::Blue),
        (
            "RAM",
            vec![
                s.mem.clone(),
                format!("{} {}", bar(s.mr, 15), pct(s.mr)),
                format!("swap {}", s.swap),
            ],
            Color::Green,
        ),
        (
            "ROOT",
            vec![
                s.dev.clone(),
                format!("{} {}", s.fs, s.disk),
                format!("available {}", s.free),
            ],
            Color::Gold,
        ),
        ("GPU", gpu_lines, Color::Pink),
        ("SESSION", vec![s.wm.clone(), s.protocol.clone(), s.terminal.clone()], Color::Pink),
        (
            "DISPLAY",
            vec![
                displays[0][1].clone(),
                last_disp[1].clone(),
                format!("{} outputs", s.displays.len()),
            ],
            Color::Blue,
        ),
    ];
    rng.shuffle(&mut cards);
    c.box_draw(
        20,
        3,
        38,
        5,
        "HOST / ROOT NODE",
        &[
            format!("{} / {}", s.host, s.os),
            s.kernel.clone(),
            format!("{} / {} pkgs", s.uptime, s.packages),
        ],
        Color::Gold,
    );
    c.wire(&[(38, 7), (38, 32)], Color::Dim);
    for (i, (title, lines, color)) in cards.iter().enumerate() {
        let i_i32 = i32::try_from(i).unwrap_or(0);
        let left = i % 2 == 0;
        let x = if left { 1 } else { 44 };
        let y = 10 + (i_i32 / 2) * 8 + rng.randrange(0, 2);
        let near = if left { 33 } else { 44 };
        c.wire(&[(38, y + 2), (near, y + 2)], *color);
        c.put(38, y + 2, "◆", *color);
        c.box_draw(x, y, 33, 6, &format!("{:02} / {title}", i + 1), lines, *color);
    }
    c.box_draw(
        1,
        33,
        76,
        4,
        "HEALTH / INVENTORY",
        &[
            format!("load {}   tasks {}   shell {}", s.load, s.tasks, s.shell),
            format!("power {}   arch {}   root {}", s.battery, s.arch, pct(s.dr)),
        ],
        Color::Green,
    );
}

fn scroll(c: &mut Canvas, s: &Snapshot, _rng: &mut PyRandom, _seed: u32) {
    let centered = |c: &mut Canvas, y: i32, text: &str, color: Color| {
        let width: i32 = text
            .chars()
            .map(|ch| if is_east_asian_wide(ch) { 2 } else { 1 })
            .sum();
        c.put((42 - width) / 2, y, text, color);
    };
    c.put(3, 0, &format!("╭{}╴", "─".repeat(35)), Color::ScrollGold);
    c.put(1, 1, "╭─╯", Color::ScrollGold);
    for y in 2..23 {
        c.put(1, y, "│", Color::ScrollGold);
    }
    let user = env::var("USER")
        .or_else(|_| env::var("LOGNAME"))
        .unwrap_or_else(|_| String::from("user"));
    centered(c, 2, &format!("{user}@{}", s.host), Color::ScrollGold);
    let (gpu, _) = details(s);
    let dgpu = gpu
        .iter()
        .find(|g| g[1] == "Discrete")
        .map_or_else(|| String::from("not detected"), |g| g[0].clone());
    let igpu = gpu
        .iter()
        .find(|g| g[1] == "Integrated")
        .map_or_else(|| String::from("not detected"), |g| g[0].clone());

    let hardware = [
        (" CPU", s.cpu.clone()),
        ("󰾲 GPU", dgpu),
        ("󰾲 iGPU", igpu),
        (" Memory", format!("{}  ({})", s.mem, pct(s.mr))),
        (" Disk", format!("{}  ({})", s.disk, pct(s.dr))),
        ("󰓡 Swap", s.swap.clone()),
        (" Battery", s.battery.clone()),
    ];
    let software = [
        (" OS", s.os.clone()),
        (" Kernel", s.kernel.clone()),
        (" WM", s.wm.clone()),
        (" Shell", s.shell.clone()),
        (" Terminal", s.terminal.clone()),
        ("󰏖 Packages", s.packages.clone()),
        (" Uptime", s.uptime.clone()),
    ];
    for (y, title, values) in [(4_i32, "Hardware", &hardware), (13_i32, "Software", &software)] {
        let dashes = "─".repeat(24_usize.saturating_sub(title.len()));
        c.put(5, y, &format!("── {title} {dashes}"), Color::ScrollGold);
        for (idx, (label, value)) in values.iter().enumerate() {
            let row = y + 1 + i32::try_from(idx).unwrap_or(0);
            c.put(5, row, label, Color::Faded);
            c.put(19, row, "·", Color::Wood);
            c.put(22, row, value, Color::Paper);
        }
    }
    centered(c, 22, "─ · ❦ · ─", Color::Wood);
    c.put(1, 23, "╰─╮", Color::ScrollGold);
    c.put(3, 24, &format!("╰{} · ❦", "─".repeat(29)), Color::ScrollGold);
}

fn parse_int_auto(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else if let Some(oct) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
        u32::from_str_radix(oct, 8).ok()
    } else if let Some(bin) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        u32::from_str_radix(bin, 2).ok()
    } else {
        s.parse::<u32>().ok()
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mut scene: Option<&str> = None;
    let mut plain = false;
    let mut seed_opt: Option<u32> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--plain" {
            plain = true;
        } else if arg == "--seed" {
            i += 1;
            let val = args.get(i).ok_or_else(|| String::from("--seed requires a value"))?;
            seed_opt = Some(parse_int_auto(val).ok_or_else(|| format!("invalid seed: {val}"))?);
        } else if let Some(val) = arg.strip_prefix("--seed=") {
            seed_opt = Some(parse_int_auto(val).ok_or_else(|| format!("invalid seed: {val}"))?);
        } else if !arg.starts_with('-') && scene.is_none() {
            scene = Some(arg.as_str());
        }
        i += 1;
    }

    let Some(name) = scene else {
        return Err(String::from(
            "usage: diagrams <circuit|metro|strata|radar|fabric|scroll> [--plain] [--seed N]",
        ));
    };

    let (w, h) = match name {
        "circuit" => (76, 35),
        "metro" => (78, 36),
        "strata" => (76, 37),
        "radar" => (78, 35),
        "fabric" => (78, 38),
        "scroll" => (80, 25),
        other => return Err(format!("unknown diagram scene: {other}")),
    };

    let (mut rng, seed) = if let Some(s) = seed_opt {
        (PyRandom::new(u64::from(s)), s)
    } else {
        PyRandom::from_entropy()
    };

    let mut canvas = Canvas::new(w, h);
    let snap = snapshot();
    match name {
        "circuit" => circuit(&mut canvas, &snap, &mut rng, seed),
        "metro" => metro(&mut canvas, &snap, &mut rng, seed),
        "strata" => strata(&mut canvas, &snap, &mut rng, seed),
        "radar" => radar(&mut canvas, &snap, &mut rng, seed),
        "fabric" => fabric(&mut canvas, &snap, &mut rng, seed),
        "scroll" => scroll(&mut canvas, &snap, &mut rng, seed),
        _ => unreachable!(),
    }

    let out = canvas.render(!plain);
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{out}");
    Ok(())
}

#![allow(
    unsafe_code,
    clippy::many_single_char_names,
    clippy::struct_field_names,
    clippy::unreadable_literal
)]

use std::ffi::{CStr, CString, c_char, c_int, c_long, c_ulong, c_ushort};
use std::fs::{self, File};
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::time::{SystemTime, UNIX_EPOCH};

#[repr(C)]
struct Winsize {
    ws_row: c_ushort,
    ws_col: c_ushort,
    ws_xpixel: c_ushort,
    ws_ypixel: c_ushort,
}

const TIOCGWINSZ: c_ulong = 0x5413;

#[repr(C)]
struct Statvfs {
    f_bsize: c_ulong,
    f_frsize: c_ulong,
    f_blocks: u64,
    f_bfree: u64,
    f_bavail: u64,
    f_files: u64,
    f_ffree: u64,
    f_favail: u64,
    f_fsid: c_ulong,
    f_flag: c_ulong,
    f_namemax: c_ulong,
    f_spare: [c_int; 6],
}

#[repr(C)]
struct Tm {
    tm_sec: c_int,
    tm_min: c_int,
    tm_hour: c_int,
    tm_mday: c_int,
    tm_mon: c_int,
    tm_year: c_int,
    tm_wday: c_int,
    tm_yday: c_int,
    tm_isdst: c_int,
    tm_gmtoff: c_long,
    tm_zone: *const c_char,
}

unsafe extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    fn statvfs(path: *const c_char, buf: *mut Statvfs) -> c_int;
    fn localtime_r(timep: *const i64, result: *mut Tm) -> *mut Tm;
    fn getuid() -> u32;
}

#[derive(Clone, Copy, Debug)]
pub struct TtySize {
    pub rows: u16,
    pub cols: u16,
    pub xpixel: u16,
    pub ypixel: u16,
}

pub fn tty_winsize() -> Option<TtySize> {
    let file = File::open("/dev/tty").ok()?;
    let fd = file.as_raw_fd();
    let mut ws = MaybeUninit::<Winsize>::zeroed();
    // SAFETY: `fd` is a valid open file descriptor to `/dev/tty`, and `ws` points to a
    // properly aligned and sized `Winsize` struct for `TIOCGWINSZ`.
    let rc = unsafe { ioctl(fd, TIOCGWINSZ, ws.as_mut_ptr()) };
    if rc == 0 {
        // SAFETY: `ioctl` returned 0, so `ws` has been initialized by the kernel.
        let ws = unsafe { ws.assume_init() };
        Some(TtySize {
            rows: ws.ws_row,
            cols: ws.ws_col,
            xpixel: ws.ws_xpixel,
            ypixel: ws.ws_ypixel,
        })
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DiskStats {
    pub total: u64,
    pub used: u64,
    pub available: u64,
}

pub fn disk_usage(path: &str) -> DiskStats {
    let Ok(cpath) = CString::new(path) else {
        return DiskStats { total: 1, used: 0, available: 0 };
    };
    let mut buf = MaybeUninit::<Statvfs>::zeroed();
    // SAFETY: `cpath` is a valid NUL-terminated C string and `buf` is a valid mutable pointer
    // to a `Statvfs` struct matching the Linux x86_64 glibc ABI.
    let rc = unsafe { statvfs(cpath.as_ptr(), buf.as_mut_ptr()) };
    if rc == 0 {
        // SAFETY: `statvfs` succeeded (rc == 0) and initialized `buf`.
        let st = unsafe { buf.assume_init() };
        let frsize = if st.f_frsize > 0 { st.f_frsize } else { st.f_bsize };
        let total = st.f_blocks.saturating_mul(frsize);
        let free = st.f_bfree.saturating_mul(frsize);
        let available = st.f_bavail.saturating_mul(frsize);
        let used = total.saturating_sub(free);
        DiskStats { total, used, available }
    } else {
        DiskStats { total: 1, used: 0, available: 0 }
    }
}

#[derive(Clone, Debug)]
pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub tzname: String,
}

pub fn local_now() -> LocalTime {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let mut tm = MaybeUninit::<Tm>::zeroed();
    // SAFETY: `secs` is a valid pointer to `time_t` (i64) and `tm` is a valid mutable pointer
    // to `struct tm` for reentrant `localtime_r`.
    let ptr = unsafe { localtime_r(&raw const secs, tm.as_mut_ptr()) };
    if ptr.is_null() {
        return LocalTime {
            year: 1970,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            tzname: String::from("UTC"),
        };
    }
    // SAFETY: `localtime_r` returned non-null, so `tm` is initialized.
    let tm = unsafe { tm.assume_init() };
    let tzname = if tm.tm_zone.is_null() {
        String::from("UTC")
    } else {
        // SAFETY: `tm.tm_zone` is a static NUL-terminated string managed by libc.
        unsafe { CStr::from_ptr(tm.tm_zone) }.to_string_lossy().into_owned()
    };
    LocalTime {
        year: tm.tm_year + 1900,
        month: u32::try_from(tm.tm_mon + 1).unwrap_or(1),
        day: u32::try_from(tm.tm_mday).unwrap_or(1),
        hour: u32::try_from(tm.tm_hour).unwrap_or(0),
        minute: u32::try_from(tm.tm_min).unwrap_or(0),
        second: u32::try_from(tm.tm_sec).unwrap_or(0),
        tzname,
    }
}

pub fn current_uid() -> u32 {
    // SAFETY: `getuid()` is always safe to call on POSIX systems.
    unsafe { getuid() }
}

pub fn read_trimmed(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_owned())
}

pub fn hostname() -> String {
    read_trimmed("/proc/sys/kernel/hostname").unwrap_or_else(|| String::from("localhost"))
}

pub fn kernel_release() -> String {
    read_trimmed("/proc/sys/kernel/osrelease").unwrap_or_else(|| String::from("Linux"))
}

pub fn kernel_arch() -> String {
    read_trimmed("/proc/sys/kernel/arch").unwrap_or_else(|| String::from(std::env::consts::ARCH))
}

pub fn os_pretty_name() -> String {
    for path in ["/etc/os-release", "/usr/lib/os-release"] {
        if let Ok(content) = fs::read_to_string(path) {
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("PRETTY_NAME=") {
                    let trimmed = rest.trim().trim_matches('"').trim_matches('\'');
                    if !trimmed.is_empty() {
                        return trimmed.to_owned();
                    }
                }
            }
        }
    }
    String::from("Linux")
}

#[derive(Clone, Copy, Debug)]
pub struct MemStats {
    pub total_kib: u64,
    pub used_kib: u64,
    pub swap_total_kib: u64,
    pub swap_used_kib: u64,
}

pub fn mem_stats() -> MemStats {
    let mut mem_total = 1_u64;
    let mut mem_avail = 0_u64;
    let mut swap_total = 0_u64;
    let mut swap_free = 0_u64;
    if let Ok(text) = fs::read_to_string("/proc/meminfo") {
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let Some(key) = parts.next() else { continue };
            let Some(val_str) = parts.next() else { continue };
            let Ok(val) = val_str.parse::<u64>() else { continue };
            match key {
                "MemTotal:" => mem_total = val,
                "MemAvailable:" => mem_avail = val,
                "SwapTotal:" => swap_total = val,
                "SwapFree:" => swap_free = val,
                _ => {}
            }
        }
    }
    MemStats {
        total_kib: mem_total.max(1),
        used_kib: mem_total.saturating_sub(mem_avail),
        swap_total_kib: swap_total,
        swap_used_kib: swap_total.saturating_sub(swap_free),
    }
}

pub fn uptime_seconds() -> f64 {
    if let Ok(text) = fs::read_to_string("/proc/uptime")
        && let Some(first) = text.split_whitespace().next()
        && let Ok(val) = first.parse::<f64>()
    {
        return val;
    }
    0.0
}

pub fn loadavg() -> [f64; 3] {
    if let Ok(text) = fs::read_to_string("/proc/loadavg") {
        let mut iter = text.split_whitespace();
        let a = iter.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let b = iter.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let c = iter.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        return [a, b, c];
    }
    [0.0, 0.0, 0.0]
}

pub fn cpu_threads() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZero::get)
}

pub fn proc_task_count() -> usize {
    let Ok(entries) = fs::read_dir("/proc") else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        })
        .count()
}

pub fn root_mount_info() -> (String, String) {
    if let Ok(text) = fs::read_to_string("/proc/mounts") {
        let mut found = None;
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let Some(dev) = parts.next() else { continue };
            let Some(mnt) = parts.next() else { continue };
            let Some(fs) = parts.next() else { continue };
            if mnt == "/" && dev != "rootfs" {
                found = Some((dev.to_owned(), fs.to_owned()));
            }
        }
        if let Some(res) = found {
            return res;
        }
    }
    (String::from("?"), String::from("?"))
}

pub fn battery_summary() -> String {
    let Ok(entries) = fs::read_dir("/sys/class/power_supply") else {
        return String::from("not present");
    };
    let mut bat_dir = None;
    let mut ac_online = false;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let ptype = fs::read_to_string(path.join("type")).unwrap_or_default();
        let ptype = ptype.trim();
        if ptype == "Battery" && bat_dir.is_none() {
            bat_dir = Some(path);
        } else if (ptype == "Mains" || ptype == "USB")
            && let Ok(online) = fs::read_to_string(path.join("online"))
            && online.trim() == "1"
        {
            ac_online = true;
        }
    }
    let Some(bat) = bat_dir else {
        return String::from("not present");
    };
    let cap = fs::read_to_string(bat.join("capacity"))
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .unwrap_or(0.0);
    let status = fs::read_to_string(bat.join("status"))
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();
    let mut statuses = Vec::new();
    if ac_online {
        statuses.push(String::from("AC Connected"));
    }
    if !status.is_empty() && status != "Unknown" {
        statuses.push(status);
    }
    format!("{cap:.0}% / {}", statuses.join(", "))
}

pub fn sha256_first_u32(data: &[u8]) -> u32 {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0_u32; 64];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    h[0]
}

/// Python-compatible Mersenne Twister (`random.Random`) so procedural layouts match 1:1.
pub struct PyRandom {
    mt: [u32; 624],
    index: usize,
}

impl PyRandom {
    pub fn new(seed: u64) -> Self {
        let mut rng = Self { mt: [0; 624], index: 624 };
        rng.init_genrand(19_650_218);
        let mut keys = Vec::new();
        let mut s = seed;
        while s > 0 {
            keys.push((s & 0xffff_ffff) as u32);
            s >>= 32;
        }
        if keys.is_empty() {
            keys.push(0);
        }
        rng.init_by_array(&keys);
        rng
    }

    pub fn from_entropy() -> (Self, u32) {
        let mut seed_bytes = [0_u8; 4];
        if let Ok(mut f) = File::open("/dev/urandom") {
            let _ = std::io::Read::read_exact(&mut f, &mut seed_bytes);
        } else {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos());
            seed_bytes = nanos.to_ne_bytes();
        }
        let seed = u32::from_ne_bytes(seed_bytes);
        (Self::new(u64::from(seed)), seed)
    }

    fn init_genrand(&mut self, s: u32) {
        self.mt[0] = s;
        for mti in 1..624 {
            let prev = self.mt[mti - 1];
            self.mt[mti] = 1_812_433_253_u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(u32::try_from(mti).unwrap_or(0));
        }
        self.index = 624;
    }

    fn init_by_array(&mut self, init_key: &[u32]) {
        let mut i = 1_usize;
        let mut j = 0_usize;
        let k_start = 624_usize.max(init_key.len());
        for _ in (1..=k_start).rev() {
            let prev = self.mt[i - 1];
            let idx_u32 = u32::try_from(j).unwrap_or(0);
            self.mt[i] = (self.mt[i] ^ ((prev ^ (prev >> 30)).wrapping_mul(1_664_525)))
                .wrapping_add(init_key[j])
                .wrapping_add(idx_u32);
            i += 1;
            j += 1;
            if i >= 624 {
                self.mt[0] = self.mt[623];
                i = 1;
            }
            if j >= init_key.len() {
                j = 0;
            }
        }
        for _ in (1..=623).rev() {
            let prev = self.mt[i - 1];
            let i_u32 = u32::try_from(i).unwrap_or(0);
            self.mt[i] = (self.mt[i] ^ ((prev ^ (prev >> 30)).wrapping_mul(1_566_083_941)))
                .wrapping_sub(i_u32);
            i += 1;
            if i >= 624 {
                self.mt[0] = self.mt[623];
                i = 1;
            }
        }
        self.mt[0] = 0x8000_0000;
    }

    pub fn genrand_uint32(&mut self) -> u32 {
        const MAG01: [u32; 2] = [0, 0x9908_b0df];
        if self.index >= 624 {
            for kk in 0..(624 - 397) {
                let y = (self.mt[kk] & 0x8000_0000) | (self.mt[kk + 1] & 0x7fff_ffff);
                self.mt[kk] = self.mt[kk + 397] ^ (y >> 1) ^ MAG01[(y & 1) as usize];
            }
            for kk in (624 - 397)..623 {
                let y = (self.mt[kk] & 0x8000_0000) | (self.mt[kk + 1] & 0x7fff_ffff);
                self.mt[kk] = self.mt[kk - (624 - 397)] ^ (y >> 1) ^ MAG01[(y & 1) as usize];
            }
            let y = (self.mt[623] & 0x8000_0000) | (self.mt[0] & 0x7fff_ffff);
            self.mt[623] = self.mt[396] ^ (y >> 1) ^ MAG01[(y & 1) as usize];
            self.index = 0;
        }
        let mut y = self.mt[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    pub fn random(&mut self) -> f64 {
        let a = self.genrand_uint32() >> 5;
        let b = self.genrand_uint32() >> 6;
        (f64::from(a) * 67_108_864.0 + f64::from(b)) * (1.0 / 9_007_199_254_740_992.0)
    }

    pub fn getrandbits(&mut self, k: u32) -> u32 {
        if k == 0 {
            return 0;
        }
        self.genrand_uint32() >> (32 - k)
    }

    pub fn randbelow(&mut self, n: u32) -> u32 {
        if n <= 1 {
            return 0;
        }
        let k = 32 - n.leading_zeros();
        loop {
            let r = self.getrandbits(k);
            if r < n {
                return r;
            }
        }
    }

    pub fn randrange(&mut self, start: i32, stop: i32) -> i32 {
        let span = u32::try_from((stop - start).max(1)).unwrap_or(1);
        start + i32::try_from(self.randbelow(span)).unwrap_or(0)
    }

    pub fn choice<'a, T>(&mut self, slice: &'a [T]) -> &'a T {
        let idx = self.randbelow(u32::try_from(slice.len()).unwrap_or(1)) as usize;
        &slice[idx]
    }

    pub fn shuffle<T>(&mut self, slice: &mut [T]) {
        for i in (1..slice.len()).rev() {
            let j = self.randbelow(u32::try_from(i + 1).unwrap_or(1)) as usize;
            slice.swap(i, j);
        }
    }

    pub fn sample<T: Clone>(&mut self, population: &[T], k: usize) -> Vec<T> {
        let mut pool = population.to_vec();
        let n = pool.len();
        let mut result = Vec::with_capacity(k);
        for i in 0..k.min(n) {
            let j = self.randbelow(u32::try_from(n - i).unwrap_or(1)) as usize;
            result.push(pool[j].clone());
            pool[j] = pool[n - i - 1].clone();
        }
        result
    }
}

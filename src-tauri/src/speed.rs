use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::net::{self, Node, UA_BROWSER, UA_UPLOAD};

pub const THREADS: usize = 4;
pub const PREHEAT: usize = 2;
pub const SAMPLES: usize = 16;
const SUB_PER_SLOT: usize = 5;
const SUB_MS: u64 = 100;
const CHUNK: usize = 262144;

const MIN_VALID_BYTES: u64 = 131_072;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const IO_TIMEOUT: Duration = Duration::from_millis(600);

pub struct PhaseCtx {
    pub hostip: String,
    pub port: u16,
    pub uuid: String,
    pub bytes: AtomicU64,
    pub stop: AtomicBool,

    pub socks: Mutex<Vec<Option<std::net::TcpStream>>>,
}

impl PhaseCtx {
    pub fn new(node: &Node, uuid: String) -> Arc<Self> {
        Arc::new(Self {
            hostip: node.hostip.clone(),
            port: node.port,
            uuid,
            bytes: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            socks: Mutex::new((0..THREADS).map(|_| None).collect()),
        })
    }

    fn aborted(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn read_bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }
}

fn register_socket(ctx: &PhaseCtx, idx: usize, s: &std::net::TcpStream) {
    if let Ok(mut g) = ctx.socks.lock() {
        if idx < g.len() {
            g[idx] = s.try_clone().ok();
        }
    }
}

fn download_worker(ctx: Arc<PhaseCtx>, idx: usize) {
    let Ok(mut s) = net::tcp_connect(&ctx.hostip, ctx.port, CONNECT_TIMEOUT) else {
        return;
    };
    register_socket(&ctx, idx, &s);
    if ctx.aborted() {
        return;
    }
    let _ = s.set_read_timeout(Some(IO_TIMEOUT));

    let req = format!(
        "GET /speed/File(1G).dl?r={}&key={} HTTP/1.1\r\nHost: {}:{}\r\n\
Connection: close\r\nAccept: */*\r\nUser-Agent: {UA_BROWSER}\r\n\r\n",
        net::now_millis(),
        ctx.uuid,
        ctx.hostip,
        ctx.port
    );
    if s.write_all(req.as_bytes()).is_err() {
        return;
    }

    let mut buf = vec![0u8; CHUNK];
    loop {
        if ctx.aborted() {
            break;
        }
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                ctx.bytes.fetch_add(n as u64, Ordering::Relaxed);
            }
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) =>
            {
                continue
            }
            Err(_) => break,
        }
    }
}

fn upload_worker(ctx: Arc<PhaseCtx>, idx: usize) {
    let Ok(mut s) = net::tcp_connect(&ctx.hostip, ctx.port, CONNECT_TIMEOUT) else {
        return;
    };
    register_socket(&ctx, idx, &s);
    if ctx.aborted() {
        return;
    }
    let _ = s.set_write_timeout(Some(IO_TIMEOUT));

    let fname = format!("SPEED_{}_000", compact_timestamp());
    let mut hdr = String::with_capacity(512);
    hdr.push_str("POST /speed/doAnalsLoad.do HTTP/1.1\r\n");
    hdr.push_str("Connection: close\r\nCache-Control: no-cache\r\nCharset: UTF-8\r\n");
    hdr.push_str(&format!("Key: {}\r\n", ctx.uuid));
    hdr.push_str("Content-Type: multipart/form-data;boundary=00content0boundary00\r\n");
    hdr.push_str(&format!("User-Agent: {UA_UPLOAD}\r\n"));
    hdr.push_str(&format!("Host: {}:{}\r\n", ctx.hostip, ctx.port));
    hdr.push_str("Accept-Encoding: gzip\r\nContent-Length: 900000000\r\n\r\n");
    hdr.push_str("--00content0boundary00\r\nContent-Disposition: form-data; ");
    hdr.push_str(&format!("name=\"upload\";filename=\"{fname}\"\r\n\r\n"));

    if s.write_all(hdr.as_bytes()).is_err() {
        return;
    }

    let zeros = vec![0u8; CHUNK];
    'outer: loop {
        if ctx.aborted() {
            break;
        }
        let mut off = 0usize;
        while off < zeros.len() {
            if ctx.aborted() {
                break 'outer;
            }
            match s.write(&zeros[off..]) {
                Ok(0) => break 'outer,
                Ok(n) => {
                    off += n;
                    ctx.bytes.fetch_add(n as u64, Ordering::Relaxed);
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                    ) =>
                {
                    continue
                }
                Err(_) => break 'outer,
            }
        }
    }
}

fn compact_timestamp() -> String {
    let secs = net::now_secs();
    let (y, mo, d, h, mi, s) = civil_from_unix(secs);
    format!("{y:04}{mo:02}{d:02}_{h:02}{mi:02}{s:02}")
}

fn civil_from_unix(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let h = (rem / 3600) as u32;
    let mi = ((rem % 3600) / 60) as u32;
    let s = (rem % 60) as u32;

    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, h, mi, s)
}

pub struct PhaseResult {
    pub avg: f64,
    pub has_samples: bool,
}

pub fn run_phase(
    ctx: &Arc<PhaseCtx>,
    upload: bool,
    on_sample: &mut dyn FnMut(bool, f64),
    on_instant: &mut dyn FnMut(f64),
) -> PhaseResult {
    ctx.bytes.store(0, Ordering::Relaxed);
    ctx.stop.store(false, Ordering::Relaxed);

    let mut handles = Vec::with_capacity(THREADS);
    for i in 0..THREADS {
        let c = Arc::clone(ctx);
        let h = std::thread::spawn(move || {
            if upload {
                upload_worker(c, i)
            } else {
                download_worker(c, i)
            }
        });
        handles.push(h);
        std::thread::sleep(Duration::from_millis(25));
    }

    let mut samples: Vec<f64> = Vec::with_capacity(SAMPLES);
    let mut prev_slot = 0u64;
    let mut prev_sub = 0u64;
    let mut saw_bytes = false;
    let total = PREHEAT + SAMPLES;

    for i in 0..total {
        if ctx.aborted() {
            break;
        }
        let is_pre = i < PREHEAT;
        let slot_start = Instant::now();

        for _ in 0..SUB_PER_SLOT {
            let sub_start = Instant::now();
            std::thread::sleep(Duration::from_millis(SUB_MS));
            if ctx.aborted() {
                break;
            }
            let sub_dt = sub_start.elapsed().as_secs_f64().max(0.001);
            let cur = ctx.read_bytes();
            let inst = (cur.saturating_sub(prev_sub)) as f64 * 8.0 / sub_dt / 1e6;
            prev_sub = cur;
            on_instant(inst);
        }
        if ctx.aborted() {
            break;
        }

        let slot_dt = slot_start.elapsed().as_secs_f64().max(0.001);
        let cur = ctx.read_bytes();
        if cur > 0 {
            saw_bytes = true;
        }
        let mbps = cur.saturating_sub(prev_slot) as f64 * 8.0 / slot_dt / 1e6;
        prev_slot = cur;
        prev_sub = cur;

        on_sample(is_pre, mbps);
        if !is_pre {
            samples.push(mbps);
        }
    }

    ctx.stop.store(true, Ordering::Relaxed);

    if let Ok(g) = ctx.socks.lock() {
        for s in g.iter().flatten() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
    for h in handles {
        let _ = h.join();
    }

    let mut r = PhaseResult {
        avg: 0.0,
        has_samples: false,
    };
    if !samples.is_empty() {
        let sum: f64 = samples.iter().sum();
        r.avg = sum / samples.len() as f64;

        r.has_samples = saw_bytes && ctx.read_bytes() >= MIN_VALID_BYTES;
    }
    r
}

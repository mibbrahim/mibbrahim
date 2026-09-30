//! JSONL writers on a dedicated thread so file I/O never blocks the decision path.

use crate::model::Event;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

pub enum LogMsg {
    Line(&'static str, String),
    Record(Box<Event>),
    Replace(&'static str, String),
    Flush,
    Stop,
}

#[derive(Clone)]
pub struct Logger {
    tx: Sender<LogMsg>,
}

/// Gzipped event recorder, one file per `rotate` period: events-<unix>.jsonl.gz
struct Recorder {
    dir: PathBuf,
    rotate: Duration,
    w: Option<GzEncoder<BufWriter<File>>>,
    opened: Instant,
}

impl Recorder {
    fn write(&mut self, line: &[u8]) {
        if self.w.is_none() || self.opened.elapsed() >= self.rotate {
            self.finish();
            let name = format!("events-{}.jsonl.gz", crate::types::now_ms() / 1000);
            match OpenOptions::new().create(true).append(true).open(self.dir.join(&name)) {
                Ok(f) => {
                    self.w = Some(GzEncoder::new(BufWriter::with_capacity(1 << 20, f), Compression::fast()));
                    self.opened = Instant::now();
                }
                Err(e) => {
                    eprintln!("recorder: cannot open {name}: {e}");
                    return;
                }
            }
        }
        if let Some(w) = self.w.as_mut() {
            let _ = w.write_all(line);
            let _ = w.write_all(b"\n");
        }
    }
    fn flush(&mut self) {
        if let Some(w) = self.w.as_mut() {
            let _ = w.flush();
        }
    }
    fn finish(&mut self) {
        if let Some(w) = self.w.take() {
            if let Ok(mut inner) = w.finish() {
                let _ = inner.flush();
            }
        }
    }
}

impl Logger {
    /// `record_rotate`: Some(period) enables the gzipped event recorder.
    pub fn start(dir: &str, record_rotate: Option<Duration>) -> anyhow::Result<(Logger, std::thread::JoinHandle<()>)> {
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir)?;
        let (tx, rx) = mpsc::channel::<LogMsg>();
        let d2 = dir.clone();
        let h = std::thread::Builder::new().name("hs-log".into()).spawn(move || {
            let mut files: HashMap<&'static str, BufWriter<File>> = HashMap::new();
            let mut rec: Option<Recorder> =
                record_rotate.map(|rotate| Recorder { dir: d2.clone(), rotate, w: None, opened: Instant::now() });
            let mut last_flush = Instant::now();
            loop {
                let msg = rx.recv_timeout(Duration::from_millis(500));
                match msg {
                    Ok(LogMsg::Line(name, line)) => {
                        let f = files.entry(name).or_insert_with(|| {
                            BufWriter::new(OpenOptions::new().create(true).append(true).open(d2.join(name)).expect("open log"))
                        });
                        let _ = f.write_all(line.as_bytes());
                        let _ = f.write_all(b"\n");
                    }
                    Ok(LogMsg::Record(ev)) => {
                        if let Some(r) = rec.as_mut() {
                            if let Ok(s) = serde_json::to_string(&ev) {
                                r.write(s.as_bytes());
                            }
                        }
                    }
                    Ok(LogMsg::Replace(name, body)) => {
                        let tmp = d2.join(format!("{name}.tmp"));
                        if fs::write(&tmp, body).is_ok() {
                            let _ = fs::rename(&tmp, d2.join(name));
                        }
                    }
                    Ok(LogMsg::Flush) => {
                        for f in files.values_mut() {
                            let _ = f.flush();
                        }
                        if let Some(r) = rec.as_mut() {
                            let _ = r.flush();
                        }
                    }
                    Ok(LogMsg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {}
                }
                if last_flush.elapsed() > Duration::from_secs(1) {
                    for f in files.values_mut() {
                        let _ = f.flush();
                    }
                    if let Some(r) = rec.as_mut() {
                        let _ = r.flush();
                    }
                    last_flush = Instant::now();
                }
            }
            for f in files.values_mut() {
                let _ = f.flush();
            }
            if let Some(r) = rec.as_mut() {
                r.finish();
            }
        })?;
        Ok((Logger { tx }, h))
    }

    pub fn line<T: serde::Serialize>(&self, file: &'static str, v: &T) {
        if let Ok(s) = serde_json::to_string(v) {
            let _ = self.tx.send(LogMsg::Line(file, s));
        }
    }

    pub fn record(&self, ev: &Event) {
        let _ = self.tx.send(LogMsg::Record(Box::new(ev.clone())));
    }

    pub fn replace(&self, file: &'static str, body: String) {
        let _ = self.tx.send(LogMsg::Replace(file, body));
    }

    pub fn flush(&self) {
        let _ = self.tx.send(LogMsg::Flush);
    }

    pub fn stop(&self) {
        let _ = self.tx.send(LogMsg::Stop);
    }
}

/// Fixed-size sample buffer for latency percentiles.
#[derive(Default, Clone)]
pub struct Samples {
    v: Vec<f64>,
    next: usize,
    pub count: u64,
}

impl Samples {
    const CAP: usize = 4096;
    pub fn push(&mut self, x: f64) {
        if !x.is_finite() {
            return;
        }
        self.count += 1;
        if self.v.len() < Self::CAP {
            self.v.push(x);
        } else {
            self.v[self.next] = x;
            self.next = (self.next + 1) % Self::CAP;
        }
    }
    pub fn pct(&self) -> serde_json::Value {
        if self.v.is_empty() {
            return serde_json::json!({"n": 0});
        }
        let mut s = self.v.clone();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let q = |p: f64| s[((s.len() - 1) as f64 * p).round() as usize];
        serde_json::json!({"n": self.count, "p50": q(0.5), "p90": q(0.9), "p99": q(0.99), "max": s[s.len() - 1]})
    }
}

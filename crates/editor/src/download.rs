//! Download helper with progress (P4.11) — egui-free, pure-Rust.
//!
//! Streams a file to disk in chunks (so a multi-GB model never sits fully in RAM),
//! reporting bytes-done / total / speed so the UI can show a progress bar, the
//! amount of total, and the live MB/s. Writes to a `.part` temp then atomically
//! renames, so an interrupted download never leaves a half file masquerading as
//! complete. Blocking — call off the UI thread.

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use std::time::Instant;

/// Snapshot of an in-flight download, handed to the progress callback.
#[derive(Clone, Copy)]
pub struct Progress {
    /// Bytes written so far.
    pub done: u64,
    /// Total bytes if the server sent `Content-Length`.
    pub total: Option<u64>,
    /// Average speed since the download started.
    pub bytes_per_sec: f64,
}

/// Download `url` to `dest`, calling `on_progress` roughly every ~120 ms and once
/// at completion. Atomic (temp → rename). Errors are human-readable strings.
pub fn download_with_progress(
    url: &str,
    dest: &Path,
    mut on_progress: impl FnMut(Progress),
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create cache dir: {e}"))?;
    }
    let mut response = ureq::get(url)
        .call()
        .map_err(|e| format!("download {url}: {e}"))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    let tmp = dest.with_extension("part");
    let mut file =
        BufWriter::new(File::create(&tmp).map_err(|e| format!("create download file: {e}"))?);
    let mut reader = response.body_mut().as_reader();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done: u64 = 0;
    let start = Instant::now();
    let mut last = Instant::now();
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("download read: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("write download: {e}"))?;
        done += n as u64;
        if last.elapsed().as_millis() >= 120 {
            let secs = start.elapsed().as_secs_f64().max(0.001);
            on_progress(Progress {
                done,
                total,
                bytes_per_sec: done as f64 / secs,
            });
            last = Instant::now();
        }
    }
    file.flush().map_err(|e| format!("flush download: {e}"))?;
    drop(file);
    fs::rename(&tmp, dest).map_err(|e| format!("finalize download: {e}"))?;

    let secs = start.elapsed().as_secs_f64().max(0.001);
    on_progress(Progress {
        done,
        total: Some(done),
        bytes_per_sec: done as f64 / secs,
    });
    Ok(())
}

/// Human-readable byte count, e.g. `1.20 GB`, `350.4 MB`, `18 KB`.
pub fn fmt_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let n = n as f64;
    if n >= GB {
        format!("{:.2} GB", n / GB)
    } else if n >= MB {
        format!("{:.1} MB", n / MB)
    } else if n >= KB {
        format!("{:.0} KB", n / KB)
    } else {
        format!("{n:.0} B")
    }
}

/// Human-readable speed, e.g. `4.3 MB/s`.
pub fn fmt_speed(bytes_per_sec: f64) -> String {
    format!("{}/s", fmt_bytes(bytes_per_sec as u64))
}

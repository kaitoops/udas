/// JSONL archive read/write with gzip support.
///
/// L1 (hot): raw JSONL, one line per entry.
/// L2 (warm): gzip-compressed JSONL.
/// L3 (cold): gzip-compressed JSONL + summary JSON sidecar.

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde_json::Value as JsonValue;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

/// Append one JSON line to a JSONL file (uncompressed).
pub fn append_jsonl(path: &Path, value: &JsonValue) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open jsonl for append: {}", path.display()))?;
    let line = serde_json::to_string(value)?;
    writeln!(file, "{}", line)?;
    Ok(())
}

/// Read all JSON lines from an uncompressed JSONL file.
pub fn read_jsonl(path: &Path) -> Result<Vec<JsonValue>> {
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open jsonl: {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if !line.trim().is_empty() {
            entries.push(serde_json::from_str(&line)?);
        }
    }
    Ok(entries)
}

/// Write JSON lines to a gzip-compressed file.
pub fn write_gzip_jsonl(path: &Path, entries: &[JsonValue]) -> Result<()> {
    let file = fs::File::create(path)
        .with_context(|| format!("failed to create gzip: {}", path.display()))?;
    let mut encoder = GzEncoder::new(file, Compression::default());
    for entry in entries {
        let line = serde_json::to_string(entry)?;
        writeln!(encoder, "{}", line)?;
    }
    encoder.finish()?;
    Ok(())
}

/// Read JSON lines from a gzip-compressed file.
pub fn read_gzip_jsonl(path: &Path) -> Result<Vec<JsonValue>> {
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open gzip: {}", path.display()))?;
    let decoder = GzDecoder::new(file);
    let reader = BufReader::new(decoder);
    let mut entries = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if !line.trim().is_empty() {
            entries.push(serde_json::from_str(&line)?);
        }
    }
    Ok(entries)
}

/// Compress a raw JSONL file into a gzip file in the same directory, deleting the original.
pub fn compress_file(path: &Path) -> Result<()> {
    let gz_path = path.with_extension("jsonl.gz");
    let entries = read_jsonl(path)?;
    write_gzip_jsonl(&gz_path, &entries)?;
    fs::remove_file(path)?;
    Ok(())
}

/// Decompress a gzip JSONL file back to raw JSONL.
pub fn decompress_file(path: &Path) -> Result<()> {
    if !path.to_string_lossy().ends_with(".gz") {
        anyhow::bail!("not a .gz file: {}", path.display());
    }
    let raw_path = path.with_extension("");
    let entries = read_gzip_jsonl(path)?;
    let mut file = fs::File::create(&raw_path)?;
    for entry in &entries {
        let line = serde_json::to_string(entry)?;
        writeln!(file, "{}", line)?;
    }
    fs::remove_file(path)?;
    Ok(())
}

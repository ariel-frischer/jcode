#![cfg_attr(test, allow(clippy::items_after_test_module))]

use super::{Tool, ToolContext, ToolOutput};
use crate::bus::{Bus, BusEvent, FileOp, FileTouch};
use anyhow::Result;
use async_trait::async_trait;
use jcode_terminal_image::{ImageDisplayParams, ImageProtocol, display_image};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

const DEFAULT_LIMIT: usize = 5000;
const MAX_LINE_LEN: usize = 2000;
const INDEX_MIN_BYTES: u64 = 8 * 1024 * 1024;
const INDEX_MAX_BYTES: usize = 16 * 1024 * 1024;

pub struct ReadTool;

impl ReadTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct ReadInput {
    file_path: String,
    #[serde(default)]
    start_line: Option<usize>,
    #[serde(default)]
    end_line: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadRangeStyle {
    OffsetLimit,
    StartEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NormalizedReadRange {
    offset: usize,
    limit: usize,
    style: ReadRangeStyle,
}

struct TextReadResult {
    output: String,
    total_lines: usize,
    truncated_line_count: usize,
}

impl NormalizedReadRange {
    fn next_offset(self) -> usize {
        self.offset + self.limit
    }

    fn next_start_line(self) -> usize {
        self.next_offset() + 1
    }
}

fn normalize_read_range(params: &ReadInput) -> Result<NormalizedReadRange> {
    let has_start_end = params.start_line.is_some() || params.end_line.is_some();
    let has_mixed_offset = match (params.start_line, params.end_line, params.offset) {
        (Some(start_line), _, Some(offset)) => {
            if start_line == 0 {
                true
            } else {
                offset.checked_add(1) != Some(start_line)
            }
        }
        (None, Some(_), Some(offset)) => offset != 0,
        _ => params.offset.is_some(),
    };

    if has_start_end && has_mixed_offset {
        return Err(anyhow::anyhow!(
            "Use either start_line/end_line (1-based) or offset (0-based), not both. `limit` may be used with either style."
        ));
    }

    if has_start_end {
        let start_line = params.start_line.unwrap_or(1);
        if start_line == 0 {
            return Err(anyhow::anyhow!(
                "start_line must be 1 or greater (it is 1-based)."
            ));
        }

        let limit = if let Some(end_line) = params.end_line {
            if end_line == 0 {
                return Err(anyhow::anyhow!(
                    "end_line must be 1 or greater (it is 1-based)."
                ));
            }
            if end_line < start_line {
                return Err(anyhow::anyhow!(
                    "end_line ({}) must be greater than or equal to start_line ({}).",
                    end_line,
                    start_line
                ));
            }
            end_line - start_line + 1
        } else {
            params.limit.unwrap_or(DEFAULT_LIMIT)
        };

        return Ok(NormalizedReadRange {
            offset: start_line - 1,
            limit,
            style: ReadRangeStyle::StartEnd,
        });
    }

    Ok(NormalizedReadRange {
        offset: params.offset.unwrap_or(0),
        limit: params.limit.unwrap_or(DEFAULT_LIMIT),
        style: ReadRangeStyle::OffsetLimit,
    })
}

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read a file. Supports text files, image files, and PDFs."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["file_path"],
            "properties": {
                "intent": super::intent_schema_property(),
                "file_path": {
                    "type": "string",
                    "description": "Path to a file."
                },
                "start_line": {
                    "type": "integer",
                    "description": "1-based start line for text files."
                },
                "limit": {
                    "type": "integer",
                    "description": "Max text lines to read. Default 5000."
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: ReadInput = serde_json::from_value(input)?;
        let range = normalize_read_range(&params)?;
        let end_exclusive = range
            .offset
            .checked_add(range.limit)
            .ok_or_else(|| anyhow::anyhow!("offset + limit exceeds the supported line range"))?;

        let path = ctx.resolve_path(Path::new(&params.file_path));

        // Check if file exists
        if !path.exists() {
            // Try to find similar files
            let suggestions = find_similar_files(&path);
            if suggestions.is_empty() {
                return Err(anyhow::anyhow!("File not found: {}", params.file_path));
            } else {
                return Err(anyhow::anyhow!(
                    "File not found: {}\nDid you mean: {}",
                    params.file_path,
                    suggestions.join(", ")
                ));
            }
        }

        // Check for image files and display in terminal if supported
        if is_image_file(&path) {
            return handle_image_file(&path, &params.file_path);
        }

        // Check for PDF files and extract text
        if is_pdf_file(&path) {
            return handle_pdf_file(&path, &params.file_path);
        }

        // Check for binary files
        if is_binary_file(&path) {
            return Ok(ToolOutput::new(format!(
                "Binary file detected: {}\nUse appropriate tools to handle binary files.",
                params.file_path
            )));
        }

        // Stream text instead of materializing the whole file. We still scan to
        // EOF so total-line and continuation metadata remain exact, but peak
        // memory is bounded by the buffered reader, one input line, and output.
        // The synchronous buffered scan runs on the blocking pool so large files
        // cannot stall a Tokio worker or the TUI render/input loop.
        let read_path = path.clone();
        let text = tokio::task::spawn_blocking(move || read_text_range_cached(&read_path, range))
            .await
            .map_err(|err| anyhow::anyhow!("read task failed to join: {err}"))??;
        let mut output = text.output;
        let total_lines = text.total_lines;
        let truncated_line_count = text.truncated_line_count;
        let end = end_exclusive.min(total_lines);

        // Publish file touch event for swarm coordination
        Bus::global().publish(BusEvent::FileTouch(FileTouch {
            session_id: ctx.session_id.clone(),
            path: path.to_path_buf(),
            op: FileOp::Read,
            intent: None,
            summary: Some(format!(
                "read lines {}-{} of {}",
                range.offset + 1,
                end,
                total_lines
            )),
            detail: None,
        }));

        if truncated_line_count > 0 || end < total_lines {
            crate::logging::warn(&format!(
                "[tool:read] returned truncated output for {} in session {} (tool_call={} range={}..{} total_lines={} truncated_lines={})",
                params.file_path,
                ctx.session_id,
                ctx.tool_call_id,
                range.offset + 1,
                end,
                total_lines,
                truncated_line_count
            ));
        }

        // Add metadata
        if end < total_lines {
            let continuation_hint = match range.style {
                ReadRangeStyle::OffsetLimit => format!("offset={}", range.next_offset()),
                ReadRangeStyle::StartEnd => format!("start_line={}", range.next_start_line()),
            };
            output.push_str(&format!(
                "\n... {} more lines (use {} to continue)\n",
                total_lines - end,
                continuation_hint
            ));
        }

        if output.is_empty() {
            Ok(ToolOutput::new("(empty file)"))
        } else {
            Ok(ToolOutput::new(output))
        }
    }
}

fn read_text_range(path: &Path, range: NormalizedReadRange) -> Result<TextReadResult> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut chunk = [0u8; 64 * 1024];
    let mut utf8_carry = Vec::with_capacity(3);
    let mut line_prefix = Vec::with_capacity(MAX_LINE_LEN + 4);
    let mut line_len = 0usize;
    let mut line_last_byte = None;
    let mut output = String::with_capacity(range.limit.min(2000) * 80);
    let mut total_lines = 0usize;
    let mut truncated_line_count = 0usize;
    let end_exclusive = range
        .offset
        .checked_add(range.limit)
        .ok_or_else(|| anyhow::anyhow!("offset + limit exceeds the supported line range"))?;

    loop {
        let bytes_read = file.read(&mut chunk)?;
        if bytes_read == 0 {
            break;
        }
        validate_utf8_chunk(&mut utf8_carry, &chunk[..bytes_read])?;

        let bytes = &chunk[..bytes_read];
        let mut segment_start = 0;
        for newline in memchr::memchr_iter(b'\n', bytes) {
            retain_line_segment(
                &mut line_prefix,
                &mut line_len,
                &mut line_last_byte,
                &bytes[segment_start..newline],
            );
            total_lines += 1;
            append_text_line(
                &mut output,
                total_lines,
                range,
                end_exclusive,
                &line_prefix,
                line_len,
                line_last_byte,
                &mut truncated_line_count,
            )?;
            line_prefix.clear();
            line_len = 0;
            line_last_byte = None;
            segment_start = newline + 1;
        }
        retain_line_segment(
            &mut line_prefix,
            &mut line_len,
            &mut line_last_byte,
            &bytes[segment_start..],
        );
    }

    if !utf8_carry.is_empty() {
        anyhow::bail!("stream did not contain valid UTF-8");
    }
    if line_len > 0 {
        total_lines += 1;
        append_text_line(
            &mut output,
            total_lines,
            range,
            end_exclusive,
            &line_prefix,
            line_len,
            line_last_byte,
            &mut truncated_line_count,
        )?;
    }

    Ok(TextReadResult {
        output,
        total_lines,
        truncated_line_count,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileFreshness {
    len: u64,
    modified: std::time::SystemTime,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

#[derive(Clone, Debug)]
struct LineIndex {
    path: PathBuf,
    freshness: FileFreshness,
    offsets: Vec<u64>,
    total_lines: usize,
}

impl LineIndex {
    fn memory_bytes(&self) -> usize {
        self.offsets
            .len()
            .saturating_mul(std::mem::size_of::<u64>())
    }
}

#[derive(Default)]
struct LineIndexCache {
    entries: VecDeque<LineIndex>,
    bytes: usize,
}

static LINE_INDEX_CACHE: LazyLock<Mutex<LineIndexCache>> =
    LazyLock::new(|| Mutex::new(LineIndexCache::default()));

fn read_text_range_cached(path: &Path, range: NormalizedReadRange) -> Result<TextReadResult> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() < INDEX_MIN_BYTES {
        return read_text_range(path, range);
    }
    let canonical = std::fs::canonicalize(path)?;
    let freshness = file_freshness(&metadata);
    if let Some(index) = cached_index(&canonical, &freshness) {
        let result = read_indexed_range(path, range, &index)?;
        if file_freshness(&std::fs::metadata(path)?) == freshness {
            return Ok(result);
        }
        return read_text_range(path, range);
    }

    let index = build_line_index(path, canonical, freshness)?;
    let result = read_indexed_range(path, range, &index)?;
    if file_freshness(&std::fs::metadata(path)?) != index.freshness {
        return read_text_range(path, range);
    }
    insert_index(index);
    Ok(result)
}

fn file_freshness(metadata: &std::fs::Metadata) -> FileFreshness {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return FileFreshness {
            len: metadata.len(),
            modified: metadata.modified().unwrap_or(std::time::UNIX_EPOCH),
            device: metadata.dev(),
            inode: metadata.ino(),
        };
    }
    #[cfg(not(unix))]
    FileFreshness {
        len: metadata.len(),
        modified: metadata.modified().unwrap_or(std::time::UNIX_EPOCH),
    }
}

fn cached_index(path: &Path, freshness: &FileFreshness) -> Option<LineIndex> {
    let mut cache = LINE_INDEX_CACHE.lock().ok()?;
    let position = cache
        .entries
        .iter()
        .position(|entry| entry.path == path && entry.freshness == *freshness)?;
    let entry = cache.entries.remove(position)?;
    cache.entries.push_front(entry.clone());
    Some(entry)
}

fn insert_index(index: LineIndex) {
    let index_bytes = index.memory_bytes();
    if index_bytes > INDEX_MAX_BYTES {
        return;
    }
    let Ok(mut cache) = LINE_INDEX_CACHE.lock() else {
        return;
    };
    if let Some(position) = cache
        .entries
        .iter()
        .position(|entry| entry.path == index.path)
    {
        if let Some(old) = cache.entries.remove(position) {
            cache.bytes = cache.bytes.saturating_sub(old.memory_bytes());
        }
    }
    cache.bytes = cache.bytes.saturating_add(index_bytes);
    cache.entries.push_front(index);
    while cache.bytes > INDEX_MAX_BYTES {
        if let Some(old) = cache.entries.pop_back() {
            cache.bytes = cache.bytes.saturating_sub(old.memory_bytes());
        } else {
            break;
        }
    }
}

fn build_line_index(
    path: &Path,
    canonical: PathBuf,
    freshness: FileFreshness,
) -> Result<LineIndex> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut chunk = [0u8; 64 * 1024];
    let mut carry = Vec::with_capacity(3);
    let mut offsets = vec![0u64];
    let mut byte_offset = 0u64;
    loop {
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        validate_utf8_chunk(&mut carry, &chunk[..count])?;
        for newline in memchr::memchr_iter(b'\n', &chunk[..count]) {
            let next = byte_offset
                .checked_add(newline as u64)
                .and_then(|offset| offset.checked_add(1))
                .ok_or_else(|| anyhow::anyhow!("file offset exceeds supported range"))?;
            offsets.push(next);
        }
        byte_offset = byte_offset
            .checked_add(count as u64)
            .ok_or_else(|| anyhow::anyhow!("file offset exceeds supported range"))?;
    }
    if !carry.is_empty() {
        anyhow::bail!("stream did not contain valid UTF-8");
    }
    let total_lines = if byte_offset == 0 {
        0
    } else if offsets.last() == Some(&byte_offset) {
        offsets.len() - 1
    } else {
        offsets.len()
    };
    Ok(LineIndex {
        path: canonical,
        freshness,
        offsets,
        total_lines,
    })
}

fn read_indexed_range(
    path: &Path,
    range: NormalizedReadRange,
    index: &LineIndex,
) -> Result<TextReadResult> {
    use std::io::{Read, Seek, SeekFrom};
    let end_exclusive = range
        .offset
        .checked_add(range.limit)
        .ok_or_else(|| anyhow::anyhow!("offset + limit exceeds the supported line range"))?;
    let mut output = String::with_capacity(range.limit.min(2000) * 80);
    let mut truncated_line_count = 0;
    if range.offset < index.total_lines {
        let mut file = std::fs::File::open(path)?;
        file.seek(SeekFrom::Start(index.offsets[range.offset]))?;
        let mut chunk = [0u8; 64 * 1024];
        let mut utf8_carry = Vec::with_capacity(3);
        let mut line_prefix = Vec::with_capacity(MAX_LINE_LEN + 4);
        let mut line_len = 0usize;
        let mut line_last_byte = None;
        let mut line_number = range.offset;
        loop {
            let bytes_read = file.read(&mut chunk)?;
            if bytes_read == 0 {
                break;
            }
            validate_utf8_chunk(&mut utf8_carry, &chunk[..bytes_read])?;
            let bytes = &chunk[..bytes_read];
            let mut segment_start = 0;
            for newline in memchr::memchr_iter(b'\n', bytes) {
                retain_line_segment(
                    &mut line_prefix,
                    &mut line_len,
                    &mut line_last_byte,
                    &bytes[segment_start..newline],
                );
                line_number += 1;
                append_text_line(
                    &mut output,
                    line_number,
                    range,
                    end_exclusive,
                    &line_prefix,
                    line_len,
                    line_last_byte,
                    &mut truncated_line_count,
                )?;
                line_prefix.clear();
                line_len = 0;
                line_last_byte = None;
                segment_start = newline + 1;
            }
            retain_line_segment(
                &mut line_prefix,
                &mut line_len,
                &mut line_last_byte,
                &bytes[segment_start..],
            );
        }
        if !utf8_carry.is_empty() {
            anyhow::bail!("stream did not contain valid UTF-8");
        }
        if line_len > 0 {
            line_number += 1;
            append_text_line(
                &mut output,
                line_number,
                range,
                end_exclusive,
                &line_prefix,
                line_len,
                line_last_byte,
                &mut truncated_line_count,
            )?;
        }
    }
    Ok(TextReadResult {
        output,
        total_lines: index.total_lines,
        truncated_line_count,
    })
}

fn retain_line_segment(
    line_prefix: &mut Vec<u8>,
    line_len: &mut usize,
    line_last_byte: &mut Option<u8>,
    segment: &[u8],
) {
    *line_len = line_len.saturating_add(segment.len());
    if let Some(&last) = segment.last() {
        *line_last_byte = Some(last);
    }
    let remaining = (MAX_LINE_LEN + 4).saturating_sub(line_prefix.len());
    line_prefix.extend_from_slice(&segment[..segment.len().min(remaining)]);
}

fn validate_utf8_chunk(carry: &mut Vec<u8>, chunk: &[u8]) -> Result<()> {
    let mut combined = Vec::with_capacity(carry.len() + chunk.len());
    combined.extend_from_slice(carry);
    combined.extend_from_slice(chunk);
    carry.clear();

    if let Err(error) = std::str::from_utf8(&combined) {
        if error.error_len().is_some() {
            anyhow::bail!("stream did not contain valid UTF-8");
        }
        carry.extend_from_slice(&combined[error.valid_up_to()..]);
    }
    Ok(())
}

fn append_text_line(
    output: &mut String,
    line_number: usize,
    range: NormalizedReadRange,
    end_exclusive: usize,
    line_prefix: &[u8],
    line_len: usize,
    line_last_byte: Option<u8>,
    truncated_line_count: &mut usize,
) -> Result<()> {
    if line_number <= range.offset || line_number > end_exclusive {
        return Ok(());
    }

    let logical_len = line_len - usize::from(line_last_byte == Some(b'\r'));
    let retained_len = logical_len.min(line_prefix.len());
    let retained_bytes = &line_prefix[..retained_len];
    let retained = match std::str::from_utf8(retained_bytes) {
        Ok(retained) => retained,
        Err(error) if error.error_len().is_none() => {
            std::str::from_utf8(&retained_bytes[..error.valid_up_to()])?
        }
        Err(error) => return Err(error.into()),
    };
    use std::fmt::Write;
    if logical_len > MAX_LINE_LEN {
        *truncated_line_count += 1;
        writeln!(
            output,
            "{:>5}\t{}...",
            line_number,
            crate::util::truncate_str(retained, MAX_LINE_LEN)
        )?;
    } else {
        writeln!(output, "{:>5}\t{}", line_number, retained)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

fn is_binary_file(path: &Path) -> bool {
    // Check by extension first (no I/O needed)
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy().to_lowercase();
        let binary_exts = [
            "png", "jpg", "jpeg", "gif", "bmp", "ico", "webp", "zip", "tar", "gz", "bz2", "xz",
            "7z", "rar", "exe", "dll", "so", "dylib", "o", "a", "class", "pyc", "wasm", "mp3",
            "mp4", "avi", "mov", "mkv", "flac", "ogg", "wav",
        ];
        if binary_exts.contains(&ext.as_str()) {
            return true;
        }
    }

    // Read only the first 8KB to check for binary content (not the entire file)
    use std::io::Read;
    if let Ok(mut file) = std::fs::File::open(path) {
        let mut buf = [0u8; 8192];
        if let Ok(n) = file.read(&mut buf)
            && n > 0
        {
            let null_count = buf[..n].iter().filter(|&&b| b == 0).count();
            return null_count > n / 10;
        }
    }

    false
}

fn find_similar_files(path: &Path) -> Vec<String> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let filename = path.file_name().map(|s| s.to_string_lossy().to_lowercase());

    let mut suggestions = Vec::new();

    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if let Some(ref target) = filename {
                // Simple similarity check
                let target_str: &str = target.as_ref();
                if name.contains(target_str) || target_str.contains(&name as &str) {
                    suggestions.push(entry.path().display().to_string());
                    if suggestions.len() >= 3 {
                        break;
                    }
                }
            }
        }
    }

    suggestions
}

/// Check if a file is an image based on extension
fn is_image_file(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy().to_lowercase();
        matches!(
            ext.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico"
        )
    } else {
        false
    }
}

/// Handle reading an image file - display in terminal if supported AND return base64 for model vision
fn handle_image_file(path: &Path, file_path: &str) -> Result<ToolOutput> {
    let protocol = ImageProtocol::detect();

    let data = std::fs::read(path)?;
    let file_size = data.len() as u64;

    let dimensions = get_image_dimensions_from_data(&data);

    let dim_str = dimensions
        .map(|(w, h)| format!("{}x{}", w, h))
        .unwrap_or_else(|| "unknown".to_string());

    let size_str = if file_size < 1024 {
        format!("{} bytes", file_size)
    } else if file_size < 1024 * 1024 {
        format!("{:.1} KB", file_size as f64 / 1024.0)
    } else {
        format!("{:.1} MB", file_size as f64 / 1024.0 / 1024.0)
    };

    let mut terminal_displayed = false;
    if protocol.is_supported() {
        let params = ImageDisplayParams::from_terminal();
        match display_image(path, &params) {
            Ok(true) => {
                terminal_displayed = true;
            }
            Ok(false) => {}
            Err(e) => {
                crate::logging::info(&format!("Warning: Failed to display image: {}", e));
            }
        }
    }

    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let media_type = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        _ => "image/png",
    };

    const MAX_IMAGE_SIZE: u64 = 20 * 1024 * 1024;
    let mut output = if file_size <= MAX_IMAGE_SIZE {
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);
        let display_note = if terminal_displayed {
            "Displayed in terminal. "
        } else {
            ""
        };
        ToolOutput::new(format!(
            "Image: {} ({})\nDimensions: {}\n{}Image sent to model for vision analysis.",
            file_path, size_str, dim_str, display_note
        ))
        .with_labeled_image(media_type, b64, file_path.to_string())
    } else {
        let display_note = if terminal_displayed {
            "\nDisplayed in terminal."
        } else {
            ""
        };
        ToolOutput::new(format!(
            "Image: {} ({})\nDimensions: {}\nImage too large for vision (max 20MB).{}",
            file_path, size_str, dim_str, display_note
        ))
    };

    output = output.with_title(format!("📷 {}", file_path));
    Ok(output)
}

/// Get image dimensions from raw data (duplicated from tui::image for convenience)
fn get_image_dimensions_from_data(data: &[u8]) -> Option<(u32, u32)> {
    // PNG: check signature and parse IHDR chunk
    if data.len() > 24 && &data[0..8] == b"\x89PNG\r\n\x1a\n" {
        let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        return Some((width, height));
    }

    // JPEG: look for SOF0/SOF2 markers
    if data.len() > 2 && data[0] == 0xFF && data[1] == 0xD8 {
        let mut i = 2;
        while i + 9 < data.len() {
            if data[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = data[i + 1];
            // SOF0 (baseline) or SOF2 (progressive)
            if marker == 0xC0 || marker == 0xC2 {
                let height = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
                let width = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
                return Some((width, height));
            }
            // Skip to next marker
            if i + 3 < data.len() {
                let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
                i += 2 + len;
            } else {
                break;
            }
        }
    }

    // GIF: parse header
    if data.len() > 10 && (&data[0..6] == b"GIF87a" || &data[0..6] == b"GIF89a") {
        let width = u16::from_le_bytes([data[6], data[7]]) as u32;
        let height = u16::from_le_bytes([data[8], data[9]]) as u32;
        return Some((width, height));
    }

    None
}

/// Check if a file is a PDF based on extension
fn is_pdf_file(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        ext.to_string_lossy().to_lowercase() == "pdf"
    } else {
        false
    }
}

/// Handle reading a PDF file - extract text content
#[cfg(feature = "pdf")]
fn handle_pdf_file(path: &Path, file_path: &str) -> Result<ToolOutput> {
    // Get file metadata
    let metadata = std::fs::metadata(path)?;
    let file_size = metadata.len();

    let size_str = if file_size < 1024 {
        format!("{} bytes", file_size)
    } else if file_size < 1024 * 1024 {
        format!("{:.1} KB", file_size as f64 / 1024.0)
    } else {
        format!("{:.1} MB", file_size as f64 / 1024.0 / 1024.0)
    };

    // Extract text from PDF
    match jcode_pdf::extract_text(path) {
        Ok(text) => {
            let mut output = String::new();
            output.push_str(&format!("PDF: {} ({})\n", file_path, size_str));
            output.push_str(&format!("{}\n", "=".repeat(60)));

            // Split into pages (pdf_extract uses form feed \x0c as page separator)
            let pages: Vec<&str> = text.split('\x0c').collect();
            let page_count = pages.len();

            output.push_str(&format!("Pages: {}\n\n", page_count));

            for (i, page) in pages.iter().enumerate() {
                let page_text = page.trim();
                if !page_text.is_empty() {
                    output.push_str(&format!("--- Page {} ---\n", i + 1));
                    // Limit each page to reasonable length
                    if page_text.len() > 10000 {
                        output.push_str(crate::util::truncate_str(page_text, 10000));
                        output.push_str("\n... (page truncated)\n");
                    } else {
                        output.push_str(page_text);
                    }
                    output.push_str("\n\n");
                }
            }

            Ok(ToolOutput::new(output))
        }
        Err(e) => {
            // Fall back to metadata only if text extraction fails
            Ok(ToolOutput::new(format!(
                "PDF: {} ({})\nCould not extract text: {}\nThis may be a scanned/image-based PDF.",
                file_path, size_str, e
            )))
        }
    }
}

/// Handle reading a PDF file when PDF support is not compiled in.
#[cfg(not(feature = "pdf"))]
fn handle_pdf_file(path: &Path, file_path: &str) -> Result<ToolOutput> {
    let metadata = std::fs::metadata(path)?;
    let file_size = metadata.len();

    let size_str = if file_size < 1024 {
        format!("{} bytes", file_size)
    } else if file_size < 1024 * 1024 {
        format!("{:.1} KB", file_size as f64 / 1024.0)
    } else {
        format!("{:.1} MB", file_size as f64 / 1024.0 / 1024.0)
    };

    Ok(ToolOutput::new(format!(
        "PDF: {} ({})\nPDF text extraction is not available in this build. Rebuild with the `pdf` feature enabled to extract text.",
        file_path, size_str
    )))
}

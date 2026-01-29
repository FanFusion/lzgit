//! Async preview loading with cancellation support.
//!
//! This module provides non-blocking file preview loading to avoid UI freezes
//! when loading large files. It reads files in chunks and supports cancellation
//! via `CancellationToken`.

use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Maximum file size to load for preview (10MB - large enough for most source files).
const MAX_PREVIEW_BYTES: usize = 10 * 1024 * 1024;

/// Chunk size for reading file content.
const CHUNK_SIZE: usize = 8 * 1024;

/// Request sent to the preview loader task.
pub enum PreviewRequest {
    /// Load a file for preview with cancellation support.
    Load {
        path: PathBuf,
        cancel: CancellationToken,
        /// Starting line number (0-indexed).
        start_line: usize,
        /// Number of lines to load (visible lines + buffer).
        visible_lines: usize,
    },
    /// Cancel the current preview load.
    Cancel,
}

/// Result of a preview load operation.
#[derive(Debug)]
pub enum PreviewResult {
    /// Successfully loaded text content.
    Ready {
        #[allow(dead_code)]
        path: PathBuf,
        content: String,
        truncated: bool,
    },
    /// Partially loaded text content (streaming mode).
    Partial {
        #[allow(dead_code)]
        path: PathBuf,
        content: String,
        start_line: usize,
        lines_loaded: usize,
        has_more_before: bool,
        has_more_after: bool,
    },
    /// File appears to be binary (contains control characters).
    Binary {
        #[allow(dead_code)]
        path: PathBuf,
    },
    /// Error occurred while loading.
    Error {
        #[allow(dead_code)]
        path: PathBuf,
        error: String,
    },
    /// Load was cancelled.
    Cancelled,
}

/// Handle for requesting file previews.
pub struct PreviewLoader {
    tx: mpsc::Sender<PreviewRequest>,
}

impl PreviewLoader {
    /// Create a new preview loader and its result receiver.
    ///
    /// This spawns a background task that processes preview requests.
    /// The receiver should be polled in the main event loop.
    pub fn new() -> (Self, mpsc::Receiver<PreviewResult>) {
        let (request_tx, request_rx) = mpsc::channel::<PreviewRequest>(16);
        let (result_tx, result_rx) = mpsc::channel::<PreviewResult>(16);

        tokio::spawn(preview_loader_task(request_rx, result_tx));

        (Self { tx: request_tx }, result_rx)
    }

    /// Request a preview for the given file path (async version).
    ///
    /// Returns a `CancellationToken` that can be used to cancel this request.
    #[allow(dead_code)]
    pub async fn request_preview(&self, path: PathBuf) -> CancellationToken {
        let cancel = CancellationToken::new();
        let _ = self
            .tx
            .send(PreviewRequest::Load {
                path,
                cancel: cancel.clone(),
                start_line: 0,
                visible_lines: 100_000, // Load up to 100k lines
            })
            .await;
        cancel
    }

    /// Request a preview synchronously (non-blocking send).
    ///
    /// Returns a `CancellationToken` that can be used to cancel this request.
    pub fn request_preview_sync(&self, path: PathBuf) -> CancellationToken {
        let cancel = CancellationToken::new();
        let _ = self.tx.try_send(PreviewRequest::Load {
            path,
            cancel: cancel.clone(),
            start_line: 0,
            visible_lines: 100_000, // Load up to 100k lines
        });
        cancel
    }

    /// Request a specific range of lines for preview (streaming mode).
    ///
    /// Returns a `CancellationToken` that can be used to cancel this request.
    pub fn request_preview_range(
        &self,
        path: PathBuf,
        start_line: usize,
        visible_lines: usize,
    ) -> CancellationToken {
        let cancel = CancellationToken::new();
        let _ = self.tx.try_send(PreviewRequest::Load {
            path,
            cancel: cancel.clone(),
            start_line,
            visible_lines,
        });
        cancel
    }

    /// Cancel the current preview load.
    pub fn cancel_current(&self) {
        let _ = self.tx.try_send(PreviewRequest::Cancel);
    }
}

/// Background task that processes preview requests.
async fn preview_loader_task(
    mut rx: mpsc::Receiver<PreviewRequest>,
    tx: mpsc::Sender<PreviewResult>,
) {
    let mut current_cancel: Option<CancellationToken> = None;

    while let Some(request) = rx.recv().await {
        match request {
            PreviewRequest::Cancel => {
                if let Some(token) = current_cancel.take() {
                    token.cancel();
                }
            }
            PreviewRequest::Load {
                path,
                cancel,
                start_line,
                visible_lines,
            } => {
                // Cancel any previous load
                if let Some(token) = current_cancel.take() {
                    token.cancel();
                }
                current_cancel = Some(cancel.clone());

                let result = load_preview(&path, &cancel, start_line, visible_lines).await;

                // Only send result if not cancelled
                if !cancel.is_cancelled() {
                    let _ = tx.send(result).await;
                } else {
                    let _ = tx.send(PreviewResult::Cancelled).await;
                }
            }
        }
    }
}

/// Load a file for preview, checking for cancellation frequently.
///
/// Supports streaming mode: loads only the requested line range (start_line + visible_lines).
/// Returns Partial if start_line > 0 or visible_lines is limited, otherwise Ready.
async fn load_preview(
    path: &PathBuf,
    cancel: &CancellationToken,
    start_line: usize,
    visible_lines: usize,
) -> PreviewResult {
    // Check cancellation at start
    if cancel.is_cancelled() {
        return PreviewResult::Cancelled;
    }

    // Open the file
    let file = match File::open(path).await {
        Ok(f) => f,
        Err(e) => {
            return PreviewResult::Error {
                path: path.clone(),
                error: format!("Could not open file: {}", e),
            };
        }
    };

    // Check cancellation after open
    if cancel.is_cancelled() {
        return PreviewResult::Cancelled;
    }

    let mut reader = BufReader::new(file);
    let mut content = String::with_capacity(CHUNK_SIZE);
    let mut line_buf = String::new();
    let mut current_line = 0;
    let mut lines_read = 0;
    let mut total_bytes = 0;
    let buffer_lines = 50; // Extra lines to load as buffer
    let target_lines = visible_lines + buffer_lines;

    // Phase 1: Skip to start_line
    while current_line < start_line {
        if cancel.is_cancelled() {
            return PreviewResult::Cancelled;
        }

        line_buf.clear();
        match reader.read_line(&mut line_buf).await {
            Ok(0) => {
                // EOF before reaching start_line - file has fewer lines
                return PreviewResult::Partial {
                    path: path.clone(),
                    content: String::new(),
                    start_line,
                    lines_loaded: 0,
                    has_more_before: start_line > 0,
                    has_more_after: false,
                };
            }
            Ok(_) => {
                current_line += 1;
            }
            Err(e) => {
                return PreviewResult::Error {
                    path: path.clone(),
                    error: format!("Error reading file: {}", e),
                };
            }
        }
    }

    // Phase 2: Read target_lines starting from start_line
    while lines_read < target_lines {
        if cancel.is_cancelled() {
            return PreviewResult::Cancelled;
        }

        line_buf.clear();
        match reader.read_line(&mut line_buf).await {
            Ok(0) => {
                // EOF reached
                break;
            }
            Ok(n) => {
                total_bytes += n;

                // Check for binary content (control characters except common ones)
                if is_binary_content(&line_buf) {
                    return PreviewResult::Binary { path: path.clone() };
                }

                content.push_str(&line_buf);
                lines_read += 1;

                // Check size limit
                if total_bytes >= MAX_PREVIEW_BYTES {
                    break;
                }
            }
            Err(e) => {
                // If we already have content, return what we have
                if !content.is_empty() {
                    break;
                }
                return PreviewResult::Error {
                    path: path.clone(),
                    error: format!("Error reading file: {}", e),
                };
            }
        }
    }

    // Check if there's more content after what we read
    let has_more_after = if lines_read >= target_lines || total_bytes >= MAX_PREVIEW_BYTES {
        // Try to read one more line to see if there's more
        line_buf.clear();
        match reader.read_line(&mut line_buf).await {
            Ok(n) => n > 0,
            Err(_) => false,
        }
    } else {
        false
    };

    // Determine if we're in streaming mode (partial load) or full load
    let is_partial = start_line > 0 || has_more_after;

    if is_partial {
        PreviewResult::Partial {
            path: path.clone(),
            content,
            start_line,
            lines_loaded: lines_read,
            has_more_before: start_line > 0,
            has_more_after,
        }
    } else {
        // Full file loaded from beginning to end
        PreviewResult::Ready {
            path: path.clone(),
            content,
            truncated: total_bytes >= MAX_PREVIEW_BYTES,
        }
    }
}

/// Check if content appears to be binary by looking for control characters.
///
/// Allows common whitespace characters (tab, newline, carriage return).
fn is_binary_content(text: &str) -> bool {
    text.bytes().any(|b| {
        // Control characters except tab (0x09), newline (0x0A), carriage return (0x0D)
        b < 0x20 && b != 0x09 && b != 0x0A && b != 0x0D
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::NamedTempFile;

    #[test]
    fn test_is_binary_content() {
        assert!(!is_binary_content("Hello, world!"));
        assert!(!is_binary_content("Line with\ttab"));
        assert!(!is_binary_content("Line with\nnewline"));
        assert!(!is_binary_content("Line with\r\nCRLF"));
        assert!(is_binary_content("Binary\x00null"));
        assert!(is_binary_content("\x01SOH character"));
    }

    #[tokio::test]
    async fn test_streaming_preview_full_load() {
        // Create a temporary file with 10 lines
        let mut temp_file = NamedTempFile::new().unwrap();
        for i in 1..=10 {
            writeln!(temp_file, "Line {}", i).unwrap();
        }
        temp_file.flush().unwrap();

        let path = temp_file.path().to_path_buf();
        let cancel = CancellationToken::new();

        // Request all lines from start (should return Ready, not Partial)
        let result = load_preview(&path, &cancel, 0, 1000).await;

        match result {
            PreviewResult::Ready {
                content, truncated, ..
            } => {
                assert!(!truncated);
                let lines: Vec<_> = content.lines().collect();
                assert_eq!(lines.len(), 10);
                assert_eq!(lines[0], "Line 1");
                assert_eq!(lines[9], "Line 10");
            }
            _ => panic!("Expected Ready result, got {:?}", result),
        }
    }

    #[tokio::test]
    async fn test_streaming_preview_partial_load() {
        // Create a temporary file with 200 lines (large enough for buffer)
        let mut temp_file = NamedTempFile::new().unwrap();
        for i in 1..=200 {
            writeln!(temp_file, "Line {}", i).unwrap();
        }
        temp_file.flush().unwrap();

        let path = temp_file.path().to_path_buf();
        let cancel = CancellationToken::new();

        // Request 20 lines starting from line 40
        // With buffer_lines=50, this will load 70 lines total (lines 41-110)
        // File has 200 lines, so there should be more after
        let result = load_preview(&path, &cancel, 40, 20).await;

        match result {
            PreviewResult::Partial {
                content,
                start_line,
                has_more_before,
                has_more_after,
                ..
            } => {
                assert_eq!(start_line, 40);
                assert!(has_more_before);
                assert!(has_more_after);

                let lines: Vec<_> = content.lines().collect();
                // Should have loaded visible_lines (20) + buffer (50) = 70 lines
                assert!(lines.len() >= 20);
                assert_eq!(lines[0], "Line 41"); // start_line is 0-indexed, so line 40 -> "Line 41"
            }
            _ => panic!("Expected Partial result for mid-file load"),
        }
    }

    #[tokio::test]
    async fn test_streaming_preview_eof_before_start() {
        // Create a small file with 5 lines
        let mut temp_file = NamedTempFile::new().unwrap();
        for i in 1..=5 {
            writeln!(temp_file, "Line {}", i).unwrap();
        }
        temp_file.flush().unwrap();

        let path = temp_file.path().to_path_buf();
        let cancel = CancellationToken::new();

        // Try to start at line 100 (beyond EOF)
        let result = load_preview(&path, &cancel, 100, 20).await;

        match result {
            PreviewResult::Partial {
                content,
                start_line,
                lines_loaded,
                has_more_before,
                has_more_after,
                ..
            } => {
                assert_eq!(start_line, 100);
                assert_eq!(lines_loaded, 0);
                assert!(has_more_before);
                assert!(!has_more_after);
                assert!(content.is_empty());
            }
            _ => panic!("Expected Partial result with empty content"),
        }
    }
}

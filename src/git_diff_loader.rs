//! Async git diff loading with cancellation support.
//!
//! This module provides non-blocking git diff loading to avoid UI freezes
//! when loading diffs for large files. It uses `tokio::task::spawn_blocking`
//! for the actual git command execution since git_ops functions are blocking I/O.

use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::git_ops;

/// Request sent to the git diff loader task.
pub enum GitDiffRequest {
    /// Load a diff for a file with cancellation support.
    Load {
        repo_root: PathBuf,
        path: String,
        is_untracked: bool,
        staged: bool,
        request_id: u64,
        cancel: CancellationToken,
    },
}

/// Result of a git diff load operation.
#[derive(Debug)]
pub enum GitDiffResult {
    /// Successfully loaded diff lines.
    Ready {
        request_id: u64,
        lines: Vec<String>,
    },
    /// Error occurred while loading.
    Error {
        request_id: u64,
        error: String,
    },
    /// Load was cancelled.
    Cancelled,
}

/// Handle for requesting git diffs.
pub struct GitDiffLoader {
    tx: mpsc::Sender<GitDiffRequest>,
}

impl GitDiffLoader {
    /// Create a new git diff loader and its result receiver.
    ///
    /// This spawns a background task that processes diff requests.
    /// The receiver should be polled in the main event loop.
    pub fn new() -> (Self, mpsc::Receiver<GitDiffResult>) {
        let (request_tx, request_rx) = mpsc::channel::<GitDiffRequest>(16);
        let (result_tx, result_rx) = mpsc::channel::<GitDiffResult>(16);

        tokio::spawn(git_diff_loader_task(request_rx, result_tx));

        (Self { tx: request_tx }, result_rx)
    }

    /// Request a diff synchronously (non-blocking send).
    ///
    /// Returns a `CancellationToken` that can be used to cancel this request.
    pub fn request_diff(
        &self,
        repo_root: PathBuf,
        path: String,
        is_untracked: bool,
        staged: bool,
        request_id: u64,
    ) -> CancellationToken {
        let cancel = CancellationToken::new();
        let _ = self.tx.try_send(GitDiffRequest::Load {
            repo_root,
            path,
            is_untracked,
            staged,
            request_id,
            cancel: cancel.clone(),
        });
        cancel
    }
}

/// Background task that processes git diff requests.
async fn git_diff_loader_task(
    mut rx: mpsc::Receiver<GitDiffRequest>,
    tx: mpsc::Sender<GitDiffResult>,
) {
    let mut current_cancel: Option<CancellationToken> = None;

    while let Some(request) = rx.recv().await {
        match request {
            GitDiffRequest::Load {
                repo_root,
                path,
                is_untracked,
                staged,
                request_id,
                cancel,
            } => {
                // Cancel any previous load
                if let Some(token) = current_cancel.take() {
                    token.cancel();
                }
                current_cancel = Some(cancel.clone());

                // Check cancellation before starting work
                if cancel.is_cancelled() {
                    let _ = tx.send(GitDiffResult::Cancelled).await;
                    continue;
                }

                // Clone values for the blocking task
                let repo_root_clone = repo_root.clone();
                let path_clone = path.clone();

                // Use spawn_blocking for the blocking git operation
                let result = tokio::task::spawn_blocking(move || {
                    load_diff(&repo_root_clone, &path_clone, is_untracked, staged)
                })
                .await;

                // Check cancellation after the blocking work
                if cancel.is_cancelled() {
                    let _ = tx.send(GitDiffResult::Cancelled).await;
                    continue;
                }

                // Process the result
                let diff_result = match result {
                    Ok(Ok(lines)) => GitDiffResult::Ready { request_id, lines },
                    Ok(Err(e)) => GitDiffResult::Error {
                        request_id,
                        error: e,
                    },
                    Err(e) => GitDiffResult::Error {
                        request_id,
                        error: format!("Task join error: {}", e),
                    },
                };

                let _ = tx.send(diff_result).await;
            }
        }
    }
}

/// Load diff for a file (blocking I/O).
fn load_diff(
    repo_root: &PathBuf,
    path: &str,
    is_untracked: bool,
    staged: bool,
) -> Result<Vec<String>, String> {
    if is_untracked {
        // For untracked files, read the content and format as a diff
        let file_path = repo_root.join(path);
        if file_path.is_dir() {
            // List directory contents for untracked directories
            match std::fs::read_dir(&file_path) {
                Ok(entries) => {
                    let mut diff_lines =
                        vec![format!("Untracked directory: {}/", path), String::new()];
                    for entry in entries.filter_map(|e| e.ok()) {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let prefix = if entry.path().is_dir() { "  " } else { "  + " };
                        diff_lines.push(format!("{}{}", prefix, name));
                    }
                    Ok(diff_lines)
                }
                Err(e) => Ok(vec![format!("Cannot read directory: {}", e)]),
            }
        } else {
            match std::fs::read_to_string(&file_path) {
                Ok(content) => {
                    let lines: Vec<&str> = content.lines().collect();
                    let line_count = lines.len();
                    let mut diff_lines = vec![
                        format!("diff --git a/{} b/{}", path, path),
                        "new file mode 100644".to_string(),
                        "--- /dev/null".to_string(),
                        format!("+++ b/{}", path),
                        format!("@@ -0,0 +1,{} @@", line_count),
                    ];
                    for line in lines {
                        diff_lines.push(format!("+{}", line));
                    }
                    Ok(diff_lines)
                }
                Err(e) => Ok(vec![format!("Cannot read file: {}", e)]),
            }
        }
    } else {
        match git_ops::diff_path(repo_root, path, staged) {
            Ok(text) => {
                if text.trim().is_empty() {
                    Ok(vec!["No diff".to_string()])
                } else {
                    Ok(text.lines().map(|l| l.to_string()).collect())
                }
            }
            Err(e) => Err(format!("git diff failed: {}", e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_load_untracked_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "line 1").unwrap();
        writeln!(file, "line 2").unwrap();

        let result = load_diff(&temp_dir.path().to_path_buf(), "test.txt", true, false);

        assert!(result.is_ok());
        let lines = result.unwrap();
        assert!(lines.iter().any(|l| l.contains("diff --git")));
        assert!(lines.iter().any(|l| l == "+line 1"));
        assert!(lines.iter().any(|l| l == "+line 2"));
    }

    #[tokio::test]
    async fn test_load_untracked_directory() {
        let temp_dir = TempDir::new().unwrap();
        let sub_dir = temp_dir.path().join("subdir");
        std::fs::create_dir(&sub_dir).unwrap();
        std::fs::write(sub_dir.join("file.txt"), "content").unwrap();

        let result = load_diff(&temp_dir.path().to_path_buf(), "subdir", true, false);

        assert!(result.is_ok());
        let lines = result.unwrap();
        assert!(lines.iter().any(|l| l.contains("Untracked directory")));
    }
}

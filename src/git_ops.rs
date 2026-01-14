use std::{fs, io, path::Path, process::Command};

use crate::branch::BranchEntry;

#[derive(Clone, Debug)]
pub struct CommitEntry {
    pub hash: String,
    pub short: String,
    pub date: String,
    pub author: String,
    pub subject: String,
    pub decoration: String,
}

#[derive(Clone, Debug)]
pub struct ReflogEntry {
    pub hash: String,
    pub selector: String,
    pub subject: String,
}

#[derive(Clone, Debug)]
pub struct CommitFileChange {
    pub status: String,
    pub path: String,
    pub old_path: Option<String>,
}

fn run_git(cwd: &Path, args: &[&str]) -> io::Result<std::process::Output> {
    Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_PAGER", "cat")
        .env("PAGER", "cat")
        .env("GIT_EDITOR", ":")
        .env("EDITOR", ":")
        .env("GIT_SEQUENCE_EDITOR", ":")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .output()
}

pub fn has_staged_changes(repo_root: &Path) -> Result<bool, String> {
    let out = run_git(repo_root, &["diff", "--cached", "--quiet"]).map_err(|e| e.to_string())?;
    match out.status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
    }
}

pub fn staged_diff(repo_root: &Path) -> Result<String, String> {
    let out = run_git(repo_root, &["diff", "--cached"]).map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn list_history(repo_root: &Path, max: usize) -> Result<Vec<CommitEntry>, String> {
    let max_s = max.to_string();
    let out = run_git(
        repo_root,
        &[
            "log",
            "--no-color",
            "--decorate=short",
            "--date=short",
            "--max-count",
            max_s.as_str(),
            "--pretty=format:%H\t%h\t%ad\t%an\t%s\t%d",
        ],
    )
    .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }

    let mut entries = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut it = line.splitn(6, '\t');
        let hash = it.next().unwrap_or("").trim().to_string();
        let short = it.next().unwrap_or("").trim().to_string();
        let date = it.next().unwrap_or("").trim().to_string();
        let author = it.next().unwrap_or("").trim().to_string();
        let subject = it.next().unwrap_or("").trim().to_string();
        let decoration = it.next().unwrap_or("").trim().to_string();
        if hash.is_empty() {
            continue;
        }
        entries.push(CommitEntry {
            hash,
            short,
            date,
            author,
            subject,
            decoration,
        });
    }

    Ok(entries)
}

pub fn list_reflog(repo_root: &Path, max: usize) -> Result<Vec<ReflogEntry>, String> {
    let max_s = max.to_string();
    let out = run_git(
        repo_root,
        &[
            "log",
            "-g",
            "--date=relative",
            "--max-count",
            max_s.as_str(),
            "--pretty=format:%H\t%gD\t%gs",
        ],
    )
    .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }

    let mut entries = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut it = line.splitn(3, '\t');
        let hash = it.next().unwrap_or("").trim().to_string();
        let selector = it.next().unwrap_or("").trim().to_string();
        let subject = it.next().unwrap_or("").trim().to_string();
        if hash.is_empty() {
            continue;
        }
        entries.push(ReflogEntry {
            hash,
            selector,
            subject,
        });
    }

    Ok(entries)
}

pub fn show_commit(repo_root: &Path, hash: &str) -> Result<String, String> {
    let out = run_git(
        repo_root,
        &[
            "show",
            "--no-color",
            "--format=fuller",
            "--stat",
            "--patch",
            hash,
        ],
    )
    .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn commit_parents(repo_root: &Path, hash: &str) -> Result<Vec<String>, String> {
    let out = run_git(repo_root, &["rev-list", "--parents", "-n", "1", hash])
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next().unwrap_or("");
    Ok(line
        .split_whitespace()
        .skip(1)
        .map(|s| s.to_string())
        .collect())
}

fn parse_name_status(text: &str) -> Vec<CommitFileChange> {
    let mut files = Vec::new();

    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }

        let parts: Vec<&str> = t.split('\t').collect();
        if parts.is_empty() {
            continue;
        }

        let status = parts[0].trim().to_string();
        if status.starts_with('R') || status.starts_with('C') {
            let old_path = parts.get(1).map(|s| s.to_string());
            let path = parts.get(2).map(|s| s.to_string()).unwrap_or_default();
            if path.is_empty() {
                continue;
            }
            files.push(CommitFileChange {
                status,
                path,
                old_path,
            });
        } else {
            let path = parts.get(1).map(|s| s.to_string()).unwrap_or_default();
            if path.is_empty() {
                continue;
            }
            files.push(CommitFileChange {
                status,
                path,
                old_path: None,
            });
        }
    }

    files
}

pub fn list_commit_files(repo_root: &Path, hash: &str) -> Result<Vec<CommitFileChange>, String> {
    let parents = commit_parents(repo_root, hash)?;
    if let Some(first_parent) = parents.first() {
        let out = run_git(
            repo_root,
            &["diff", "--no-color", "--name-status", first_parent, hash],
        )
        .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        return Ok(parse_name_status(&String::from_utf8_lossy(&out.stdout)));
    }

    let out = run_git(
        repo_root,
        &[
            "show",
            "--no-color",
            "--format=",
            "--name-status",
            "--no-patch",
            hash,
        ],
    )
    .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }

    Ok(parse_name_status(&String::from_utf8_lossy(&out.stdout)))
}

pub fn show_commit_file_diff(repo_root: &Path, hash: &str, path: &str) -> Result<String, String> {
    let parents = commit_parents(repo_root, hash)?;
    if let Some(first_parent) = parents.first() {
        let out = run_git(
            repo_root,
            &["diff", "--no-color", first_parent, hash, "--", path],
        )
        .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        return Ok(String::from_utf8_lossy(&out.stdout).to_string());
    }

    let hash_s = hash.to_string();
    let path_s = path.to_string();

    let args = [
        "show".to_string(),
        "--no-color".to_string(),
        "--format=".to_string(),
        "--patch".to_string(),
        hash_s,
        "--".to_string(),
        path_s,
    ];
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    let out = run_git(repo_root, &refs).map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn add_to_gitignore(repo_root: &Path, patterns: &[String]) -> Result<usize, String> {
    if patterns.is_empty() {
        return Ok(0);
    }

    let path = repo_root.join(".gitignore");
    let existing = fs::read_to_string(&path).unwrap_or_default();

    let mut set = std::collections::BTreeSet::new();
    for line in existing.lines() {
        let t = line.trim_end();
        if !t.is_empty() {
            set.insert(t.to_string());
        }
    }

    let mut to_add: Vec<String> = Vec::new();
    for p in patterns {
        let t = p.trim();
        if t.is_empty() || t == ".gitignore" {
            continue;
        }
        if !set.contains(t) {
            to_add.push(t.to_string());
            set.insert(t.to_string());
        }
    }

    if to_add.is_empty() {
        return Ok(0);
    }

    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    for p in &to_add {
        out.push_str(p);
        out.push('\n');
    }

    fs::write(&path, out).map_err(|e| e.to_string())?;
    Ok(to_add.len())
}

pub fn stage_path(repo_root: &Path, path: &str) -> Result<(), String> {
    stage_paths(repo_root, &[path.to_string()])
}

pub fn stage_paths(repo_root: &Path, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    let mut args: Vec<&str> = Vec::with_capacity(2 + paths.len());
    args.push("add");
    args.push("--");

    let owned: Vec<String> = paths.iter().cloned().collect();
    let mut refs: Vec<&str> = Vec::with_capacity(owned.len());
    for p in &owned {
        refs.push(p.as_str());
    }

    let mut all: Vec<&str> = Vec::with_capacity(args.len() + refs.len());
    all.extend(args);
    all.extend(refs);

    let out = run_git(repo_root, &all).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn unstage_paths(repo_root: &Path, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    let owned: Vec<String> = paths.iter().cloned().collect();
    let mut refs: Vec<&str> = Vec::with_capacity(owned.len());
    for p in &owned {
        refs.push(p.as_str());
    }

    let mut all: Vec<&str> = Vec::with_capacity(4 + refs.len());
    all.push("restore");
    all.push("--staged");
    all.push("--");
    all.extend(refs);

    let out = run_git(repo_root, &all).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn discard_worktree_path(repo_root: &Path, path: &str) -> Result<(), String> {
    let out = run_git(repo_root, &["restore", "--", path]).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn discard_untracked_path(repo_root: &Path, path: &str) -> Result<(), String> {
    let out = run_git(repo_root, &["clean", "-f", "--", path]).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn discard_all_changes_path(repo_root: &Path, path: &str) -> Result<(), String> {
    let out = run_git(
        repo_root,
        &["restore", "--staged", "--worktree", "--", path],
    )
    .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn merge_head_exists(repo_root: &Path) -> Result<bool, String> {
    let out = run_git(repo_root, &["rev-parse", "-q", "--verify", "MERGE_HEAD"])
        .map_err(|e| e.to_string())?;
    Ok(out.status.success())
}

pub fn rebase_in_progress(repo_root: &Path) -> Result<bool, String> {
    let out = run_git(repo_root, &["rev-parse", "--git-path", "rebase-merge"])
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() && repo_root.join(p).exists() {
            return Ok(true);
        }
    }

    let out = run_git(repo_root, &["rev-parse", "--git-path", "rebase-apply"])
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() && repo_root.join(p).exists() {
            return Ok(true);
        }
    }

    Ok(false)
}

pub fn merge_continue(repo_root: &Path) -> Result<(), String> {
    let out = run_git(repo_root, &["merge", "--continue"]).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn merge_abort(repo_root: &Path) -> Result<(), String> {
    let out = run_git(repo_root, &["merge", "--abort"]).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn rebase_continue(repo_root: &Path) -> Result<(), String> {
    let out = run_git(repo_root, &["rebase", "--continue"]).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn rebase_abort(repo_root: &Path) -> Result<(), String> {
    let out = run_git(repo_root, &["rebase", "--abort"]).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn rebase_skip(repo_root: &Path) -> Result<(), String> {
    let out = run_git(repo_root, &["rebase", "--skip"]).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn list_branches(repo_root: &Path) -> Result<Vec<BranchEntry>, String> {
    let format = "%(HEAD)\t%(refname:short)\t%(upstream:short)\t%(upstream:track)";

    let local_out = run_git(
        repo_root,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "refs/heads",
            "--format",
            format,
        ],
    )
    .map_err(|e| e.to_string())?;
    if !local_out.status.success() {
        return Err(String::from_utf8_lossy(&local_out.stderr)
            .trim()
            .to_string());
    }

    let remote_out = run_git(
        repo_root,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "refs/remotes",
            "--format",
            format,
        ],
    )
    .map_err(|e| e.to_string())?;
    if !remote_out.status.success() {
        return Err(String::from_utf8_lossy(&remote_out.stderr)
            .trim()
            .to_string());
    }

    let mut branches = Vec::new();

    for line in String::from_utf8_lossy(&local_out.stdout).lines() {
        let mut it = line.split('\t');
        let head = it.next().unwrap_or("").trim();
        let name = it.next().unwrap_or("").trim().to_string();
        if name.is_empty() {
            continue;
        }
        let upstream = it
            .next()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let track = it
            .next()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        branches.push(BranchEntry {
            name,
            is_current: head == "*",
            is_remote: false,
            upstream,
            track,
        });
    }

    for line in String::from_utf8_lossy(&remote_out.stdout).lines() {
        let mut it = line.split('\t');
        let _head = it.next().unwrap_or("").trim();
        let name = it.next().unwrap_or("").trim().to_string();
        if name.is_empty() || name.ends_with("/HEAD") {
            continue;
        }
        let upstream = it
            .next()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let track = it
            .next()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        branches.push(BranchEntry {
            name,
            is_current: false,
            is_remote: true,
            upstream,
            track,
        });
    }

    Ok(branches)
}

pub fn is_dirty(repo_root: &Path) -> Result<bool, String> {
    let out = run_git(repo_root, &["status", "--porcelain"]).map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(!out.stdout.is_empty())
}

pub fn checkout_branch(repo_root: &Path, branch: &str) -> Result<(), String> {
    let out = run_git(repo_root, &["checkout", branch]).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn checkout_branch_entry(repo_root: &Path, branch: &BranchEntry) -> Result<(), String> {
    if !branch.is_remote {
        return checkout_branch(repo_root, branch.name.as_str());
    }

    let local_name = branch
        .name
        .split_once('/')
        .map(|(_, rest)| rest)
        .unwrap_or(branch.name.as_str());

    let out = run_git(
        repo_root,
        &[
            "checkout",
            "--track",
            "-b",
            local_name,
            branch.name.as_str(),
        ],
    )
    .map_err(|e| e.to_string())?;

    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn checkout_detached(repo_root: &Path, hash: &str) -> Result<(), String> {
    let out = run_git(repo_root, &["checkout", "--detach", hash]).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn fetch_prune(repo_root: &Path) -> Result<(), String> {
    let out = run_git(repo_root, &["fetch", "--prune"]).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn pull_rebase(repo_root: &Path) -> Result<(), String> {
    let out = run_git(repo_root, &["pull", "--rebase"]).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn push(repo_root: &Path) -> Result<(), String> {
    let out = run_git(repo_root, &["push"]).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn commit_message(repo_root: &Path, message: &str) -> Result<(), String> {
    let msg = message.trim();
    if msg.is_empty() {
        return Err("Empty commit message".to_string());
    }

    let mut path = std::env::temp_dir();
    path.push(format!(
        "te-commit-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));

    fs::write(&path, msg).map_err(|e| e.to_string())?;

    let out = run_git(
        repo_root,
        &["commit", "-F", path.to_string_lossy().as_ref()],
    )
    .map_err(|e| e.to_string())?;

    let _ = fs::remove_file(&path);

    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

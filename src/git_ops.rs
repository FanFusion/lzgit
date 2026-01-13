use std::{fs, io, path::Path, process::Command};

use crate::branch::BranchEntry;

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

pub fn list_local_branches(repo_root: &Path) -> Result<Vec<BranchEntry>, String> {
    let out = run_git(
        repo_root,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "refs/heads",
            "--format=%(HEAD)\t%(refname:short)\t%(upstream:short)\t%(upstream:track)",
        ],
    )
    .map_err(|e| e.to_string())?;

    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let mut branches = Vec::new();
    for line in text.lines() {
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

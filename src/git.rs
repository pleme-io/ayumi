//! Git subprocess integration.
//!
//! Runs `git diff` and `git blame` as child processes and parses their output.

use std::path::Path;
use std::process::Command;

use crate::diff::{self, Hunk};

/// Run `git diff` for a file against HEAD and return parsed hunks.
///
/// Uses `-U0` for minimal output (header-only hunks without context lines),
/// which is fastest for sign-column display.
pub fn diff_file(file_path: &Path) -> Result<Vec<Hunk>, GitError> {
    let dir = file_path
        .parent()
        .ok_or_else(|| GitError::InvalidPath(file_path.display().to_string()))?;

    let output = Command::new("git")
        .args(["diff", "-U0", "HEAD", "--"])
        .arg(file_path)
        .current_dir(dir)
        .output()
        .map_err(|e| GitError::Spawn(e.to_string()))?;

    if !output.status.success() {
        // `git diff` exits 0 for no diff, 1 for diff present — both are fine.
        // Only truly fatal errors (128+) are failures.
        let code = output.status.code().unwrap_or(-1);
        if code > 1 {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::Command(stderr.into_owned()));
        }
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(diff::parse_unified_diff(&stdout))
}

/// A single blame line result.
#[derive(Debug, Clone)]
pub struct BlameLine {
    /// Short commit hash.
    pub hash: String,
    /// Author name.
    pub author: String,
    /// Date string (ISO-like).
    pub date: String,
    /// The commit summary.
    pub summary: String,
}

/// Run `git blame` for a specific line in a file.
///
/// `line` is 1-indexed (matching Neovim display lines).
pub fn blame_line(file_path: &Path, line: usize) -> Result<BlameLine, GitError> {
    let dir = file_path
        .parent()
        .ok_or_else(|| GitError::InvalidPath(file_path.display().to_string()))?;

    let line_arg = format!("{line},{line}");
    let output = Command::new("git")
        .args([
            "blame",
            "--porcelain",
            "-L",
            &line_arg,
            "--",
        ])
        .arg(file_path)
        .current_dir(dir)
        .output()
        .map_err(|e| GitError::Spawn(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GitError::Command(stderr.into_owned()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_blame_porcelain(&stdout)
}

/// Check whether a file is inside a git repository.
pub fn is_in_repo(file_path: &Path) -> bool {
    let Some(dir) = file_path.parent() else {
        return false;
    };

    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(dir)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Parse porcelain blame output into a [`BlameLine`].
fn parse_blame_porcelain(output: &str) -> Result<BlameLine, GitError> {
    let mut hash = String::new();
    let mut author = String::new();
    let mut date = String::new();
    let mut summary = String::new();

    for line in output.lines() {
        if hash.is_empty() {
            // First line: `<hash> <orig_line> <final_line> [<num_lines>]`
            if let Some(h) = line.split_whitespace().next() {
                hash = h.chars().take(8).collect();
            }
        } else if let Some(val) = line.strip_prefix("author ") {
            author = val.to_string();
        } else if let Some(val) = line.strip_prefix("author-time ") {
            date = val.to_string();
        } else if let Some(val) = line.strip_prefix("summary ") {
            summary = val.to_string();
        }
    }

    if hash.is_empty() {
        return Err(GitError::Parse("empty blame output".to_string()));
    }

    Ok(BlameLine {
        hash,
        author,
        date,
        summary,
    })
}

/// Errors from git subprocess operations.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("failed to spawn git: {0}")]
    Spawn(String),

    #[error("git command failed: {0}")]
    Command(String),

    #[error("invalid file path: {0}")]
    InvalidPath(String),

    #[error("failed to parse git output: {0}")]
    Parse(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_blame_porcelain_basic() {
        let output = "\
abcdef12 10 10 1
author Jane Doe
author-mail <jane@example.com>
author-time 1700000000
author-tz +0000
committer Jane Doe
committer-mail <jane@example.com>
committer-time 1700000000
committer-tz +0000
summary Fix the widget
filename src/lib.rs
\tlet x = 42;
";
        let blame = parse_blame_porcelain(output).unwrap();
        assert_eq!(blame.hash, "abcdef12");
        assert_eq!(blame.author, "Jane Doe");
        assert_eq!(blame.date, "1700000000");
        assert_eq!(blame.summary, "Fix the widget");
    }

    #[test]
    fn parse_blame_empty_output() {
        let result = parse_blame_porcelain("");
        assert!(result.is_err());
    }
}

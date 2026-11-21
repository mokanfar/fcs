use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::SystemTime;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub path: PathBuf,
    pub line_number: Option<usize>,
    pub snippet: Option<String>,
    pub project_name: Option<String>,
}

/// Middle-truncates any path longer than `max_len` characters with `...`,
/// preserving leading root segments and trailing path segments
/// (e.g. `/Users/dan/.../code/asdf/`).
pub fn truncate_path_middle(path: &str, max_len: usize) -> String {
    let char_count = path.chars().count();
    if char_count <= max_len {
        return path.to_string();
    }
    if max_len <= 3 {
        return ".".repeat(max_len);
    }

    let trailing_slash = path.ends_with('/');
    let trimmed = path.trim_end_matches('/');
    let leading_slash = trimmed.starts_with('/');
    let segments: Vec<&str> = trimmed
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    if segments.len() >= 3 {
        // Try preserving leading segments (e.g. /Users/dan) and trailing segments
        let mut lead = if leading_slash {
            format!("/{}", segments[0])
        } else {
            segments[0].to_string()
        };
        let mut lead_idx = 1usize;

        let mut tail = if trailing_slash {
            format!("{}/", segments[segments.len() - 1])
        } else {
            segments[segments.len() - 1].to_string()
        };
        let mut tail_idx = segments.len() - 1;

        let make_candidate = |l: &str, t: &str| format!("{}/.../{}", l, t);

        if make_candidate(&lead, &tail).chars().count() <= max_len {
            // Greedily add alternating trailing and leading segments while fitting in max_len
            let mut add_tail = true;
            loop {
                if lead_idx >= tail_idx {
                    break;
                }
                let mut progressed = false;
                if add_tail && tail_idx > lead_idx {
                    let next_tail = format!("{}/{}", segments[tail_idx - 1], tail);
                    if make_candidate(&lead, &next_tail).chars().count() <= max_len {
                        tail = next_tail;
                        tail_idx -= 1;
                        progressed = true;
                    }
                }
                if !progressed && lead_idx < tail_idx {
                    let next_lead = format!("{}/{}", lead, segments[lead_idx]);
                    if make_candidate(&next_lead, &tail).chars().count() <= max_len {
                        lead = next_lead;
                        lead_idx += 1;
                        progressed = true;
                    }
                }
                if !progressed {
                    break;
                }
                add_tail = !add_tail;
            }
            return make_candidate(&lead, &tail);
        }
    }

    // Fallback character-level middle truncation
    let avail = max_len - 3;
    let left_len = avail / 2;
    let right_len = avail - left_len;
    let chars: Vec<char> = path.chars().collect();
    let left: String = chars[..left_len].iter().collect();
    let right: String = chars[chars.len() - right_len..].iter().collect();
    format!("{}...{}", left, right)
}

/// Lists files under `roots` using `rg --files`.
pub async fn list_files(roots: &[PathBuf], show_hidden: bool) -> Result<Vec<PathBuf>> {
    let existing: Vec<&PathBuf> = roots.iter().filter(|p| p.exists()).collect();
    if existing.is_empty() {
        return Ok(Vec::new());
    }

    let mut cmd = Command::new("rg");
    cmd.arg("--files").arg("--no-messages");
    if show_hidden {
        cmd.arg("--hidden");
    }
    for root in &existing {
        cmd.arg(root);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());

    let output = cmd.output().await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut files: Vec<PathBuf> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(PathBuf::from)
        .collect();
    files.sort();
    Ok(files)
}

/// Lists directories under `roots` using `fd --type d` (fallback: `find`).
pub async fn list_directories(roots: &[PathBuf], show_hidden: bool) -> Result<Vec<PathBuf>> {
    let existing: Vec<&PathBuf> = roots.iter().filter(|p| p.exists()).collect();
    if existing.is_empty() {
        return Ok(Vec::new());
    }

    let mut dirs_out: Vec<PathBuf> = existing.iter().map(|p| (*p).clone()).collect();

    // Prefer `fd` if available, else fallback to `find`
    let mut fd_cmd = Command::new("fd");
    fd_cmd.arg("--type").arg("d").arg("--absolute-path");
    if show_hidden {
        fd_cmd.arg("--hidden");
    }
    fd_cmd.arg(".");
    for root in &existing {
        fd_cmd.arg(root);
    }
    fd_cmd.stdout(Stdio::piped()).stderr(Stdio::null());

    if let Ok(output) = fd_cmd.output().await {
        if output.status.success() || !output.stdout.is_empty() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
                dirs_out.push(PathBuf::from(line.trim_end_matches('/')));
            }
            dirs_out.sort();
            dirs_out.dedup();
            return Ok(dirs_out);
        }
    }

    // Fallback to `find`
    let mut find_cmd = Command::new("find");
    for root in &existing {
        find_cmd.arg(root);
    }
    find_cmd.arg("-maxdepth").arg("5").arg("-type").arg("d");
    if !show_hidden {
        find_cmd.arg("-not").arg("-path").arg("*/.*");
    }
    find_cmd.stdout(Stdio::piped()).stderr(Stdio::null());

    if let Ok(output) = find_cmd.output().await {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
            dirs_out.push(PathBuf::from(line.trim_end_matches('/')));
        }
    }
    dirs_out.sort();
    dirs_out.dedup();
    Ok(dirs_out)
}

/// Filters candidate lines through `fzf --filter` (with `--exact` for strict path matching).
pub async fn fzf_filter(candidates: &[String], query: &str, exact: bool) -> Result<Vec<String>> {
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(candidates.iter().take(200).cloned().collect());
    }

    let mut cmd = Command::new("fzf");
    cmd.arg("--filter").arg(trimmed);
    if exact {
        cmd.arg("--exact");
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => {
            // Fallback in-process filter if fzf is unavailable
            let q = trimmed.to_ascii_lowercase();
            return Ok(candidates
                .iter()
                .filter(|c| c.to_ascii_lowercase().contains(&q))
                .take(200)
                .cloned()
                .collect());
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let joined = candidates.join("\n");
        let _ = stdin.write_all(joined.as_bytes()).await;
        drop(stdin);
    }

    let output = child.wait_with_output().await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter(|l| !l.is_empty())
        .take(200)
        .map(String::from)
        .collect())
}

/// Searches file contents with `ripgrep` (`rg`) and ranks matches via `fzf --filter`.
/// Requires at least 3 characters in `query`.
pub async fn search_content(
    roots: &[PathBuf],
    query: &str,
    show_hidden: bool,
) -> Result<Vec<SearchResult>> {
    let trimmed = query.trim();
    if trimmed.chars().count() < 3 {
        return Ok(Vec::new());
    }
    let existing: Vec<&PathBuf> = roots.iter().filter(|p| p.exists()).collect();
    if existing.is_empty() {
        return Ok(Vec::new());
    }

    let mut cmd = Command::new("rg");
    cmd.arg("--line-number")
        .arg("--no-heading")
        .arg("--color=never")
        .arg("--smart-case")
        .arg("--max-count=30")
        .arg("--max-filesize=2M")
        .arg("--no-messages")
        .arg("--fixed-strings");
    if show_hidden {
        cmd.arg("--hidden");
    }
    cmd.arg(trimmed);
    for root in &existing {
        cmd.arg(root);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());

    let output = cmd.output().await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let raw_lines: Vec<String> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(1000)
        .map(String::from)
        .collect();

    if raw_lines.is_empty() {
        return Ok(Vec::new());
    }

    // Pipe through fzf for fuzzy ranking
    let ranked = fzf_filter(&raw_lines, trimmed, false).await?;
    let lines_to_parse = if ranked.is_empty() { raw_lines } else { ranked };

    let mut results = Vec::new();
    for line in lines_to_parse.into_iter().take(200) {
        let mut parts = line.splitn(3, ':');
        if let (Some(file), Some(line_no), Some(snippet)) =
            (parts.next(), parts.next(), parts.next())
        {
            let parsed_line = line_no.parse::<usize>().ok();
            let path = PathBuf::from(file);
            results.push(SearchResult {
                path,
                line_number: parsed_line,
                snippet: Some(snippet.trim().to_string()),
                project_name: None,
            });
        }
    }

    Ok(results)
}

/// Returns files in a project root sorted by last-modified timestamp (most recent first).
pub async fn project_recent_entries(
    root: &Path,
    show_hidden: bool,
) -> Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let files = list_files(&[root.to_path_buf()], show_hidden).await?;
    let mut with_mtime: Vec<(PathBuf, SystemTime)> = files
        .into_iter()
        .filter_map(|p| {
            let mtime = fs::metadata(&p)
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Some((p, mtime))
        })
        .collect();

    with_mtime.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(with_mtime.into_iter().map(|(p, _)| p).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_path_middle_short_unchanged() {
        let p = "/Users/dev/code/app.rs";
        assert_eq!(truncate_path_middle(p, 40), p);
    }

    #[test]
    fn test_truncate_path_middle_long_preserves_ends() {
        let p = "/Users/dev/Documents/projects/very-long-directory-name/code/asdf/";
        let out = truncate_path_middle(p, 40);
        assert!(out.chars().count() <= 40, "got len {}: {}", out.chars().count(), out);
        assert!(out.starts_with("/Users/dev/"));
        assert!(out.contains("..."));
        assert!(out.ends_with("/code/asdf/"));
    }
}

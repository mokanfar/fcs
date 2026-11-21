use anyhow::{anyhow, Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PLAINTEXT_EXTENSIONS: &[&str] = &[
    "rs", "js", "jsx", "ts", "tsx", "mjs", "cjs", "txt", "md", "markdown", "json", "toml",
    "yaml", "yml", "py", "rb", "go", "java", "kt", "swift", "c", "h", "cpp", "hpp", "cc",
    "cs", "php", "html", "htm", "css", "scss", "sass", "less", "xml", "svg", "sql", "sh",
    "bash", "zsh", "fish", "env", "ini", "cfg", "conf", "log", "csv", "tsv", "gitignore",
    "gitattributes", "editorconfig", "lock", "vim", "lua", "zig", "ex", "exs", "erl",
];

const PLAINTEXT_FILENAMES: &[&str] = &[
    "Makefile",
    "Dockerfile",
    "LICENSE",
    "README",
    "CHANGELOG",
    "Gemfile",
    "Rakefile",
    "Procfile",
    ".bashrc",
    ".zshrc",
    ".gitconfig",
];

pub fn is_plaintext_file(path: &Path) -> bool {
    if path.is_dir() {
        return false;
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if PLAINTEXT_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()) {
            return true;
        }
    }
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if PLAINTEXT_FILENAMES.contains(&name) {
            return true;
        }
    }
    // Sniff up to 512 bytes for null bytes to detect binary files
    if let Ok(bytes) = fs::read(path) {
        let sample_len = bytes.len().min(512);
        return !bytes[..sample_len].contains(&0);
    }
    false
}

/// Copies a plain UTF-8 string to the macOS system clipboard via `pbcopy`.
pub fn copy_text_to_clipboard(text: &str) -> Result<()> {
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to spawn pbcopy")?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }
    let status = child.wait()?;
    if !status.success() {
        return Err(anyhow!("pbcopy exited with status {}", status));
    }
    Ok(())
}

/// Reads plain UTF-8 text from the macOS system clipboard via `pbpaste`.
pub fn read_text_from_clipboard() -> Result<String> {
    let output = Command::new("pbpaste")
        .output()
        .context("Failed to run pbpaste")?;
    if !output.status.success() {
        return Err(anyhow!("pbpaste failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Reads file URL references from the macOS `NSPasteboard` (e.g. files copied via
/// Finder `Cmd+C`, Copy File, or Bucket Copy File).
pub fn read_files_from_pasteboard() -> Result<Vec<PathBuf>> {
    let jxa = r#"
ObjC.import('AppKit');
ObjC.import('Foundation');
function run() {
  var pb = $.NSPasteboard.generalPasteboard;
  var classes = $.NSArray.arrayWithObject($.NSURL.class);
  var options = $.NSDictionary.dictionaryWithObjectForKey(
    $(true),
    $.NSPasteboardURLReadingFileURLsOnlyKey
  );
  var urls = pb.readObjectsForClassesOptions(classes, options);
  if (!urls || urls.count == 0) return "";
  var out = [];
  for (var i = 0; i < urls.count; i++) {
    var u = urls.objectAtIndex(i);
    if (u.isFileURL) {
      out.push(ObjC.unwrap(u.path));
    }
  }
  return out.join("\n");
}
"#;

    let output = Command::new("osascript")
        .arg("-l")
        .arg("JavaScript")
        .arg("-e")
        .arg(jxa)
        .output()
        .context("Failed to read files from NSPasteboard")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("NSPasteboard read failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let paths: Vec<PathBuf> = stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect();
    Ok(paths)
}

/// Copies native file URL references to macOS `NSPasteboard` so files can be pasted
/// directly into Finder or web apps (e.g. Cmd+V upload to Imgur).
pub fn copy_files_to_pasteboard(paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Err(anyhow!("No files specified to copy"));
    }

    let jxa = r#"
ObjC.import('AppKit');
ObjC.import('Foundation');
function run(argv) {
  var pb = $.NSPasteboard.generalPasteboard;
  pb.clearContents;
  var urls = $.NSMutableArray.alloc.init;
  for (var i = 0; i < argv.length; i++) {
    urls.addObject($.NSURL.fileURLWithPath(argv[i]));
  }
  pb.writeObjects(urls);
}
"#;

    let mut cmd = Command::new("osascript");
    cmd.arg("-l").arg("JavaScript").arg("-e").arg(jxa);
    for p in paths {
        let abs = fs::canonicalize(p).unwrap_or_else(|_| p.clone());
        cmd.arg(abs);
    }
    let output = cmd.output().context("Failed to run osascript for NSPasteboard")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("NSPasteboard copy failed: {}", stderr.trim()));
    }
    Ok(())
}

pub fn copy_file_name(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("Invalid file name"))?
        .to_string();
    copy_text_to_clipboard(&name)?;
    Ok(name)
}

pub fn copy_file_contents(paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Err(anyhow!("No file selected"));
    }
    let mut combined = Vec::new();
    for p in paths {
        if !is_plaintext_file(p) {
            return Err(anyhow!(
                "'{}' is not a plaintext file",
                p.file_name().and_then(|n| n.to_str()).unwrap_or("file")
            ));
        }
        let content = fs::read_to_string(p)
            .with_context(|| format!("Failed to read '{}'", p.display()))?;
        combined.push(content);
    }
    copy_text_to_clipboard(&combined.join("\n"))?;
    Ok(())
}

pub fn copy_relative_paths(paths: &[PathBuf], base: Option<&Path>) -> Result<String> {
    let fallback_base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let root = base.unwrap_or(&fallback_base);
    let rels: Vec<String> = paths
        .iter()
        .map(|p| {
            let abs = fs::canonicalize(p).unwrap_or_else(|_| p.clone());
            let abs_root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
            abs.strip_prefix(&abs_root)
                .map(|r| r.display().to_string())
                .unwrap_or_else(|_| abs.display().to_string())
        })
        .collect();
    let joined = rels.join("\n");
    copy_text_to_clipboard(&joined)?;
    Ok(joined)
}

pub fn copy_absolute_paths(paths: &[PathBuf]) -> Result<String> {
    let abs: Vec<String> = paths
        .iter()
        .map(|p| {
            fs::canonicalize(p)
                .unwrap_or_else(|_| p.clone())
                .display()
                .to_string()
        })
        .collect();
    let joined = abs.join("\n");
    copy_text_to_clipboard(&joined)?;
    Ok(joined)
}

/// Moves `src` into `dest_dir`, creating `dest_dir` via `create_dir_all` (`mkdir -p`) if needed.
pub fn move_into_dir(src: &Path, dest_dir: &Path) -> Result<PathBuf> {
    if !src.exists() {
        return Err(anyhow!("Source does not exist: {}", src.display()));
    }
    if !dest_dir.exists() {
        fs::create_dir_all(dest_dir)
            .with_context(|| format!("Failed to create directory '{}'", dest_dir.display()))?;
    }
    let file_name = src
        .file_name()
        .ok_or_else(|| anyhow!("Source path has no file name"))?;
    let dest = dest_dir.join(file_name);

    if fs::rename(src, &dest).is_err() {
        // Cross-filesystem fallback: copy then remove
        copy_recursive(src, &dest)?;
        if src.is_dir() {
            fs::remove_dir_all(src)?;
        } else {
            fs::remove_file(src)?;
        }
    }
    Ok(dest)
}

/// Copies `src` into `dest_dir`, creating `dest_dir` via `create_dir_all` (`mkdir -p`) if needed.
pub fn copy_into_dir(src: &Path, dest_dir: &Path) -> Result<PathBuf> {
    if !src.exists() {
        return Err(anyhow!("Source does not exist: {}", src.display()));
    }
    if !dest_dir.exists() {
        fs::create_dir_all(dest_dir)
            .with_context(|| format!("Failed to create directory '{}'", dest_dir.display()))?;
    }
    let file_name = src
        .file_name()
        .ok_or_else(|| anyhow!("Source path has no file name"))?;
    let dest = dest_dir.join(file_name);
    copy_recursive(src, &dest)?;
    Ok(dest)
}

fn copy_recursive(src: &Path, dest: &Path) -> Result<()> {
    if src.is_dir() {
        fs::create_dir_all(dest)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let child_src = entry.path();
            let child_dest = dest.join(entry.file_name());
            copy_recursive(&child_src, &child_dest)?;
        }
    } else {
        fs::copy(src, dest)?;
    }
    Ok(())
}

pub fn open_in_editor(path: &Path, editor: &str) -> Result<()> {
    let trimmed = editor.trim();

    // If the configured editor is an explicit path to an executable (which may contain spaces),
    // execute it directly without splitting on whitespace.
    if Path::new(trimmed).exists() {
        Command::new(trimmed)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("Failed to launch editor '{}'", trimmed))?;
        return Ok(());
    }

    let mut parts = trimmed.split_whitespace();
    let bin = parts.next().unwrap_or("subl");
    let args: Vec<&str> = parts.collect();

    let spawn_res = Command::new(bin)
        .args(&args)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    if spawn_res.is_ok() {
        return Ok(());
    }

    // macOS fallback when `subl` is not symlinked into $PATH
    if bin == "subl" {
        let candidates = [
            "/Applications/Sublime Text.app/Contents/SharedSupport/bin/subl",
            "/Applications/Sublime Text 2.app/Contents/SharedSupport/bin/subl",
        ];
        for candidate in candidates {
            if Path::new(candidate).exists() {
                Command::new(candidate)
                    .args(&args)
                    .arg(path)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .with_context(|| format!("Failed to launch editor '{}'", candidate))?;
                return Ok(());
            }
        }
    }

    spawn_res.with_context(|| format!("Failed to launch editor '{}'", editor))?;
    Ok(())
}

/// Opens plaintext files in configured editor; otherwise launches default macOS app via `open`.
pub fn open_file_smart(path: &Path, editor: &str) -> Result<()> {
    if is_plaintext_file(path) {
        open_in_editor(path, editor)
    } else {
        open_with_default_app(&[path.to_path_buf()])
    }
}

pub fn open_with_default_app(paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Err(anyhow!("No paths to open"));
    }
    Command::new("open")
        .args(paths)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to execute macOS `open`")?;
    Ok(())
}

pub fn open_directory(path: &Path) -> Result<()> {
    let dir = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    Command::new("open")
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to open directory")?;
    Ok(())
}

pub fn reveal_in_finder(paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Err(anyhow!("No paths to reveal"));
    }
    Command::new("open")
        .arg("-R")
        .args(paths)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to reveal in Finder")?;
    Ok(())
}

pub fn quick_look(path: &Path) -> Result<()> {
    Command::new("qlmanage")
        .arg("-p")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to launch Quick Look (qlmanage)")?;
    Ok(())
}

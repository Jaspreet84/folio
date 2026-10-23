//! What following a link means, and the outside world: opening web links
//! in the browser, copying text to the clipboard, and OSC 8 link targets.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The result of following a link.
#[derive(Debug, PartialEq, Eq)]
pub enum LinkAction {
    /// A heading or footnote in the current document.
    Anchor(String),
    /// A Markdown file, opened in folio, optionally at a heading.
    File { path: PathBuf, fragment: Option<String> },
    /// A folder, opened in the file picker.
    Dir(PathBuf),
    /// A web or mail link, opened by the system.
    External(String),
    /// Something folio won't open; the text says why.
    Refused(String),
}

const MARKDOWN_EXTENSIONS: [&str; 7] = ["md", "markdown", "mdown", "mkd", "mkdn", "mdwn", "mdx"];

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| MARKDOWN_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// The URL scheme, if `url` has one. Single letters are not schemes, so a
/// Windows path like `C:/x` isn't mistaken for one.
fn scheme(url: &str) -> Option<&str> {
    let (s, _) = url.split_once(':')?;
    let mut chars = s.chars();
    let first_ok = chars.next()?.is_ascii_alphabetic();
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    (s.len() >= 2 && first_ok && rest_ok).then_some(s)
}

/// Decide what following `url` should do. `base` is the folder of the
/// current document; `None` (reading stdin) means the current folder.
///
/// Only web and mail links are handed to the system. Local files are opened
/// only if they are Markdown, so a document can't trick the reader into
/// launching a program.
pub fn classify(url: &str, base: Option<&Path>) -> LinkAction {
    let url = url.trim();
    if let Some(fragment) = url.strip_prefix('#') {
        return LinkAction::Anchor(percent_decode(fragment));
    }
    if let Some(s) = scheme(url) {
        return match s.to_ascii_lowercase().as_str() {
            "http" | "https" | "mailto" => LinkAction::External(url.to_string()),
            "file" => {
                let rest = url[s.len() + 1..].trim_start_matches("//");
                let rest = rest.strip_prefix("localhost").unwrap_or(rest);
                if !rest.starts_with('/') {
                    return LinkAction::Refused("Only local file links can be opened".into());
                }
                let (path, fragment) = split_fragment(rest);
                local(&path, base, fragment)
            }
            other => LinkAction::Refused(format!("folio doesn't open {other}: links")),
        };
    }
    let (path, fragment) = split_fragment(url);
    if path.is_empty() {
        return fragment.map_or(LinkAction::Refused("Empty link".into()), LinkAction::Anchor);
    }
    local(&path, base, fragment)
}

fn split_fragment(url: &str) -> (String, Option<String>) {
    let (path, fragment) = match url.split_once('#') {
        Some((p, f)) => (p, Some(percent_decode(f))),
        None => (url, None),
    };
    let path = path.split('?').next().unwrap_or("");
    (percent_decode(path), fragment.filter(|f| !f.is_empty()))
}

fn local(path: &str, base: Option<&Path>, fragment: Option<String>) -> LinkAction {
    let base = base.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let mut full = if Path::new(path).is_absolute() { PathBuf::from(path) } else { base.join(path) };
    // On GitHub, "/docs/x.md" means the repository root, not the disk root.
    if path.starts_with('/') && !full.exists()
        && let Some(root) = repo_root(&base) {
            full = root.join(path.trim_start_matches('/'));
        }
    if full.is_dir() {
        LinkAction::Dir(full)
    } else if !full.exists() {
        LinkAction::Refused(format!("File not found: {path}"))
    } else if is_markdown(&full) {
        LinkAction::File { path: full, fragment }
    } else {
        LinkAction::Refused(format!("Not a Markdown file: {path} (y copies the link)"))
    }
}

fn repo_root(start: &Path) -> Option<PathBuf> {
    let start = start.canonicalize().ok()?;
    start.ancestors().find(|d| d.join(".git").exists()).map(Path::to_path_buf)
}

/// The URL to put in an OSC 8 hyperlink, so clicking the text in the
/// terminal opens it. Anchors and unknown schemes get none.
pub fn osc8_target(url: &str, base: Option<&Path>) -> Option<String> {
    let url = url.trim();
    if url.starts_with('#') || url.is_empty() {
        return None;
    }
    if let Some(s) = scheme(url) {
        let known = ["http", "https", "mailto", "ftp", "file"];
        return known.contains(&s.to_ascii_lowercase().as_str()).then(|| url.to_string());
    }
    let (path, fragment) = split_fragment(url);
    let base = base.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let full = base.join(path);
    let full = full.canonicalize().unwrap_or(full);
    let full = full.to_string_lossy();
    if !full.starts_with('/') {
        return None;
    }
    let mut out = format!("file://{}", percent_encode(&full));
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(&percent_encode(&f));
    }
    Some(out)
}

/// Open a web or mail link with the system's handler.
pub fn open_external(url: &str) -> Result<(), String> {
    let programs: &[&str] = if cfg!(target_os = "macos") {
        &["open"]
    } else if cfg!(windows) {
        &["explorer"]
    } else {
        &["xdg-open", "wslview"]
    };
    for program in programs {
        match Command::new(program).arg(url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            Ok(mut child) => {
                std::thread::spawn(move || child.wait());
                return Ok(());
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("Can't run {program}: {e}")),
        }
    }
    Err("No program to open links (install xdg-utils)".into())
}

/// Copy text to the clipboard. On a local machine this uses the system
/// clipboard tool; over SSH, or when there is none, it asks the terminal
/// through OSC 52. Returns how it was copied.
pub fn copy(text: &str) -> io::Result<&'static str> {
    let remote = std::env::var_os("SSH_TTY").is_some() || std::env::var_os("SSH_CONNECTION").is_some();
    if !remote {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let x11 = std::env::var_os("DISPLAY").is_some();
        let tools: &[(&str, &[&str], bool)] = &[
            ("pbcopy", &[], cfg!(target_os = "macos")),
            ("wl-copy", &[], wayland),
            ("xclip", &["-selection", "clipboard"], x11),
            ("xsel", &["--clipboard", "--input"], x11),
        ];
        for (program, args, usable) in tools {
            if *usable && pipe_to(program, args, text).is_ok() {
                return Ok("clipboard");
            }
        }
    }
    let mut out = io::stdout();
    out.write_all(osc52(text).as_bytes())?;
    out.flush()?;
    Ok("terminal clipboard (OSC 52)")
}

fn pipe_to(program: &str, args: &[&str], text: &str) -> io::Result<()> {
    let mut child = Command::new(program).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    child.stdin.take().expect("piped stdin").write_all(text.as_bytes())?;
    let status = child.wait()?;
    if status.success() { Ok(()) } else { Err(io::Error::other(format!("{program} failed"))) }
}

pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn percent_decode(s: &str) -> String {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(hi << 4 | lo);
                i += 3;
                continue;
            }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch folder: a.md, img.png, sub/b.md, and a .git marker.
    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("folio-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        for f in ["a.md", "img.png", "sub/b.md"] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        dir
    }

    #[test]
    fn classifies_links() {
        let dir = scratch();
        let base = Some(dir.as_path());
        assert_eq!(classify("#Install%20guide", base), LinkAction::Anchor("Install guide".into()));
        assert_eq!(classify("https://example.com", base), LinkAction::External("https://example.com".into()));
        assert_eq!(classify("mailto:a@b.c", base), LinkAction::External("mailto:a@b.c".into()));
        assert!(matches!(classify("javascript:alert(1)", base), LinkAction::Refused(_)));
        assert_eq!(
            classify("sub/b.md#part", base),
            LinkAction::File { path: dir.join("sub/b.md"), fragment: Some("part".into()) }
        );
        assert_eq!(classify("sub", base), LinkAction::Dir(dir.join("sub")));
        assert!(matches!(classify("img.png", base), LinkAction::Refused(r) if r.starts_with("Not a Markdown")));
        assert!(matches!(classify("missing.md", base), LinkAction::Refused(r) if r.starts_with("File not found")));
        // Root-relative links resolve against the repository root.
        if cfg!(unix) {
            let sub = dir.join("sub");
            match classify("/a.md", Some(sub.as_path())) {
                LinkAction::File { path, .. } => assert!(path.ends_with("a.md")),
                other => panic!("{other:?}"),
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn builds_osc8_targets() {
        assert_eq!(osc8_target("https://x.y/z", None).as_deref(), Some("https://x.y/z"));
        assert_eq!(osc8_target("#top", None), None);
        assert_eq!(osc8_target("javascript:x", None), None);
        if cfg!(unix) {
            let t = osc8_target("my file.md#a b", Some(Path::new("/docs"))).unwrap();
            assert_eq!(t, "file:///docs/my%20file.md#a%20b");
        }
    }

    #[test]
    fn encodes_base64_and_percent() {
        assert_eq!(base64(b"hello"), "aGVsbG8=");
        assert_eq!(base64(b"hi"), "aGk=");
        assert_eq!(base64(b"abc"), "YWJj");
        assert_eq!(osc52("hi"), "\x1b]52;c;aGk=\x07");
        assert_eq!(percent_decode("a%20b%zz%"), "a b%zz%");
    }
}

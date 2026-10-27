//! Opening Markdown files from Finder on macOS.
//!
//! Finder can only hand files to an app, and folio is a terminal program. So
//! `folio --install-finder` builds a small app, Folio.app, on the user's Mac
//! with tools that ship with macOS: an AppleScript applet (`osacompile`) that
//! declares it can view Markdown. Finder then lists it under "Open With",
//! and once it is the default app for Markdown, double-clicking a file opens
//! it. The applet does one thing: it runs `folio --launch-terminal FILE`,
//! which opens a new terminal window running `folio FILE`.
//!
//! Everything that builds text (the AppleScript, the Info.plist entries, the
//! launch commands) is a plain function, tested on every platform. The steps
//! that run macOS tools are checked on real Macs by the CI workflow in
//! `.github/workflows/ci.yml`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// The bundle identifier of the generated app.
pub const BUNDLE_ID: &str = "io.github.jaspreet84.folio";

/// The app's name in Finder.
pub const APP_NAME: &str = "Folio";

/// The uniform type identifier for Markdown (a system type from macOS 27).
pub const MARKDOWN_UTI: &str = "net.daringfireball.markdown";

/// File extensions claimed as Markdown.
pub const MARKDOWN_EXTENSIONS: [&str; 6] = ["md", "markdown", "mdown", "mkd", "mkdn", "mdwn"];

/// Launch Services' registration tool.
const LSREGISTER: &str =
    "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

/// Asks NSWorkspace which app opens a file, sets the default, and finds apps.
const HELPER: &str = include_str!("finder-helper.js");

/// Terminals folio knows how to open a window in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Terminal {
    Terminal,
    #[value(name = "iterm", alias = "iterm2")]
    ITerm,
    Ghostty,
    Kitty,
    #[value(name = "wezterm")]
    WezTerm,
    Alacritty,
}

impl Terminal {
    pub const ALL: [Terminal; 6] =
        [Terminal::Terminal, Terminal::ITerm, Terminal::Ghostty, Terminal::Kitty, Terminal::WezTerm, Terminal::Alacritty];

    /// The name used on the command line (`--terminal ghostty`).
    pub fn name(self) -> &'static str {
        match self {
            Terminal::Terminal => "terminal",
            Terminal::ITerm => "iterm",
            Terminal::Ghostty => "ghostty",
            Terminal::Kitty => "kitty",
            Terminal::WezTerm => "wezterm",
            Terminal::Alacritty => "alacritty",
        }
    }

    pub fn from_name(name: &str) -> Option<Terminal> {
        let name = name.to_ascii_lowercase();
        Terminal::ALL.into_iter().find(|t| t.name() == name || (name == "iterm2" && *t == Terminal::ITerm))
    }

    /// The app's name, for messages.
    pub fn app_name(self) -> &'static str {
        match self {
            Terminal::Terminal => "Terminal",
            Terminal::ITerm => "iTerm2",
            Terminal::Ghostty => "Ghostty",
            Terminal::Kitty => "kitty",
            Terminal::WezTerm => "WezTerm",
            Terminal::Alacritty => "Alacritty",
        }
    }

    pub fn bundle_id(self) -> &'static str {
        match self {
            Terminal::Terminal => "com.apple.Terminal",
            Terminal::ITerm => "com.googlecode.iterm2",
            Terminal::Ghostty => "com.mitchellh.ghostty",
            Terminal::Kitty => "net.kovidgoyal.kitty",
            Terminal::WezTerm => "com.github.wez.wezterm",
            Terminal::Alacritty => "org.alacritty",
        }
    }

    /// The oldest version folio can start a command in. Ghostty before 1.4.0
    /// also opens the file as a second, broken tab.
    pub fn min_version(self) -> Option<&'static str> {
        (self == Terminal::Ghostty).then_some("1.4.0")
    }

    /// The terminal folio is running in. Each terminal's own variables come
    /// first: kitty and Alacritty don't set TERM_PROGRAM, and tmux replaces
    /// it. Anything unknown (VS Code, Warp, tmux alone) gets Terminal.app,
    /// which every Mac has.
    pub fn detect(env: &dyn Fn(&str) -> Option<String>) -> Terminal {
        let has = |k: &str| env(k).is_some_and(|v| !v.is_empty());
        let program = env("TERM_PROGRAM").unwrap_or_default();
        let term = env("TERM").unwrap_or_default();
        if has("KITTY_WINDOW_ID") || has("KITTY_PID") || term == "xterm-kitty" {
            Terminal::Kitty
        } else if has("ALACRITTY_WINDOW_ID") {
            Terminal::Alacritty
        } else if has("WEZTERM_PANE") || program == "WezTerm" {
            Terminal::WezTerm
        } else if has("GHOSTTY_RESOURCES_DIR") || program == "ghostty" {
            Terminal::Ghostty
        } else if has("ITERM_SESSION_ID") || program == "iTerm.app" {
            Terminal::ITerm
        } else {
            Terminal::Terminal
        }
    }
}

/// How to open a new terminal window running folio.
#[derive(Debug, PartialEq, Eq)]
pub enum Launch {
    /// Run `/usr/bin/open` with these arguments.
    Open(Vec<String>),
    /// Write `script` to an executable `.command` file and open it with the
    /// app `bundle_id`. Terminal.app and iTerm2 run `.command` files in a new
    /// window, and no permission to control them is needed.
    CommandFile { bundle_id: &'static str, script: String },
}

/// The launch for `terminal`, running `folio --paging always [FILE]`. The
/// pager is forced: a short document would otherwise be printed and folio
/// would exit, and some terminals close the window when it does.
pub fn launch_plan(terminal: Terminal, folio: &Path, file: Option<&Path>) -> Launch {
    let mut command = vec![folio.to_string_lossy().into_owned(), "--paging".into(), "always".into()];
    command.extend(file.map(|f| f.to_string_lossy().into_owned()));
    let open = |options: &[&str]| {
        let mut args: Vec<String> = ["-n", "-b", terminal.bundle_id(), "--args"].iter().map(|s| s.to_string()).collect();
        args.extend(options.iter().map(|s| s.to_string()));
        args.extend(command.iter().cloned());
        Launch::Open(args)
    };
    match terminal {
        Terminal::Terminal | Terminal::ITerm => {
            Launch::CommandFile { bundle_id: terminal.bundle_id(), script: command_script(&command) }
        }
        Terminal::Ghostty | Terminal::Alacritty => open(&["-e"]),
        // kitty keeps running without windows unless told otherwise.
        Terminal::Kitty => open(&["-o", "macos_quit_when_last_window_closed=yes"]),
        Terminal::WezTerm => open(&["start", "--"]),
    }
}

/// A `.command` script that deletes itself, then replaces itself with
/// `command`, so the terminal window runs folio directly.
pub fn command_script(command: &[String]) -> String {
    let quoted: Vec<String> = command.iter().map(|a| shell_quote(a)).collect();
    format!("#!/bin/sh\nrm -f \"$0\"\nexec {}\n", quoted.join(" "))
}

/// Quote `s` for a POSIX shell: wrap it in single quotes, and write each
/// single quote inside as '\''.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Quote `s` as an AppleScript string literal.
pub fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', r"\\").replace('"', "\\\""))
}

/// The applet's source. Finder calls `on open` with the files; opening the
/// app itself calls `on run`, which shows folio's file picker on the home
/// folder. Values are literals, not `property` variables, because a signed
/// applet can't rely on saving them.
pub fn applet_source(folio: &Path, terminal: Terminal) -> String {
    format!(
        r#"-- Folio: opens Markdown files from Finder in folio, in a terminal window.
-- Generated by `folio --install-finder`. Run that again to update it.

on open theItems
	repeat with anItem in theItems
		launchFolio(POSIX path of anItem)
	end repeat
end open

on run
	launchFolio(POSIX path of (path to home folder))
end run

on launchFolio(target)
	try
		do shell script (quoted form of {folio}) & " --launch-terminal --terminal {terminal} " & (quoted form of target)
	on error errMsg
		activate
		display alert "Folio couldn't open " & target message errMsg as critical
	end try
end launchFolio
"#,
        folio = applescript_string(&folio.to_string_lossy()),
        terminal = terminal.name(),
    )
}

/// `CFBundleDocumentTypes`: Folio can view Markdown. One entry claims the
/// Markdown type, the other the file extensions, because before macOS 27 a
/// `.md` file's type depends on which apps are installed. (An entry that has
/// content types ignores its extensions, so they must be separate.) Rank
/// "Alternate" lists Folio under "Open With" without taking files over.
pub fn document_types_xml() -> String {
    let extensions: String = MARKDOWN_EXTENSIONS.iter().map(|e| format!("<string>{e}</string>")).collect();
    format!(
        "<array>\
<dict><key>CFBundleTypeName</key><string>Markdown document</string>\
<key>CFBundleTypeRole</key><string>Viewer</string>\
<key>LSHandlerRank</key><string>Alternate</string>\
<key>LSItemContentTypes</key><array><string>{MARKDOWN_UTI}</string></array></dict>\
<dict><key>CFBundleTypeName</key><string>Markdown document (by extension)</string>\
<key>CFBundleTypeRole</key><string>Viewer</string>\
<key>LSHandlerRank</key><string>Alternate</string>\
<key>CFBundleTypeExtensions</key><array>{extensions}</array></dict>\
</array>"
    )
}

/// The folio path to store in the applet. Homebrew's `Cellar/folio/<version>`
/// path disappears on upgrade, so prefer the stable `<prefix>/bin/folio`.
pub fn stored_folio_path(exe: &Path, exists: &dyn Fn(&Path) -> bool) -> PathBuf {
    let text = exe.to_string_lossy();
    if let Some(at) = text.find("/Cellar/folio/") {
        let stable = PathBuf::from(format!("{}/bin/folio", &text[..at]));
        if exists(&stable) {
            return stable;
        }
    }
    exe.to_path_buf()
}

/// Whether dotted version `have` is at least `need` ("1.10.0" > "1.4.0").
pub fn version_at_least(have: &str, need: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect()
    };
    let (have, need) = (parse(have), parse(need));
    for i in 0..have.len().max(need.len()) {
        let (a, b) = (have.get(i).copied().unwrap_or(0), need.get(i).copied().unwrap_or(0));
        if a != b {
            return a > b;
        }
    }
    true
}

// ----- installing ---------------------------------------------------------

pub struct InstallOptions {
    /// Where to put Folio.app; `~/Applications` when `None`.
    pub app_dir: Option<PathBuf>,
    pub terminal: Terminal,
    pub make_default: bool,
}

pub struct Installed {
    pub app: PathBuf,
    pub folio: PathBuf,
    /// The terminal files open in (may differ from the one asked for).
    pub terminal: Terminal,
    /// The outcome of making Folio the default Markdown app, if asked.
    pub default: Option<Result<(), String>>,
    /// Things the reader should know.
    pub notes: Vec<String>,
}

/// Build Folio.app and register it with Launch Services.
pub fn install(options: &InstallOptions) -> Result<Installed> {
    check_macos()?;
    let mut notes = Vec::new();
    let exe = std::env::current_exe().context("can't find folio's own path")?;
    let exe = std::path::absolute(&exe).unwrap_or(exe);
    let folio = stored_folio_path(&exe, &|p: &Path| p.is_file());

    let mut terminal = options.terminal;
    let (_, term_version) = find_app(terminal.bundle_id())
        .with_context(|| format!("{} isn't installed; choose another with --terminal", terminal.app_name()))?;
    if let Some(need) = terminal.min_version()
        && !version_at_least(&term_version, need)
    {
        notes.push(format!(
            "{} {term_version} can't start a command from Finder (it needs {need}); using Terminal instead. \
             Run --install-finder again after updating it.",
            terminal.app_name()
        ));
        terminal = Terminal::Terminal;
    } else if terminal == Terminal::ITerm {
        notes.push("iTerm2 asks before running each file; you can tell it not to ask again.".into());
    }

    let dir = match &options.app_dir {
        Some(d) => d.clone(),
        None => home()?.join("Applications"),
    };
    std::fs::create_dir_all(&dir).with_context(|| format!("can't create {}", dir.display()))?;
    let app = dir.join(format!("{APP_NAME}.app"));
    if app.exists() {
        let _ = run_tool(LSREGISTER, &["-u".as_ref(), app.as_os_str()]);
        remove_app(&app)?;
    }

    let source = user_temp_dir().join(format!("folio-applet-{}.applescript", std::process::id()));
    std::fs::write(&source, applet_source(&folio, terminal))
        .with_context(|| format!("can't write {}", source.display()))?;
    let compiled = run_tool("/usr/bin/osacompile", &["-o".as_ref(), app.as_os_str(), source.as_os_str()]);
    let _ = std::fs::remove_file(&source);
    compiled?;

    let plist = app.join("Contents/Info.plist");
    let set = |key: &str, kind: &str, value: &str| {
        run_tool("/usr/bin/plutil", &["-replace".as_ref(), key.as_ref(), kind.as_ref(), value.as_ref(), plist.as_os_str()])
    };
    let version = env!("CARGO_PKG_VERSION");
    set("CFBundleIdentifier", "-string", BUNDLE_ID)?;
    set("CFBundleName", "-string", APP_NAME)?;
    set("CFBundleShortVersionString", "-string", version)?;
    set("CFBundleVersion", "-string", version)?;
    // A background app: no Dock icon while it hands the file to folio.
    set("LSUIElement", "-bool", "YES")?;
    // Replace the whole list: applets can carry a catch-all entry, which
    // would offer Folio for every kind of file.
    set("CFBundleDocumentTypes", "-xml", &document_types_xml())?;
    run_tool("/usr/bin/plutil", &["-lint".as_ref(), plist.as_os_str()])?;
    let exe_name = plist_value(&plist, "CFBundleExecutable")?;
    if !app.join("Contents/MacOS").join(&exe_name).is_file() {
        bail!("osacompile made an app without its program ({exe_name})");
    }

    // Editing Info.plist breaks the applet's signature, and macOS won't run
    // an app with a broken one, so sign it again (ad hoc). The script must
    // be read-only first, and stray file attributes stop codesign.
    let script = app.join("Contents/Resources/Scripts/main.scpt");
    if let Ok(meta) = std::fs::metadata(&script) {
        let mut perms = meta.permissions();
        perms.set_readonly(true);
        let _ = std::fs::set_permissions(&script, perms);
    }
    run_tool("/usr/bin/xattr", &["-cr".as_ref(), app.as_os_str()])?;
    run_tool("/usr/bin/codesign", &["--force".as_ref(), "--sign".as_ref(), "-".as_ref(), app.as_os_str()])?;
    run_tool("/usr/bin/codesign", &["--verify".as_ref(), "--strict".as_ref(), app.as_os_str()])?;

    // Register, watching whether that alone changed the default app: macOS
    // has been seen to make a newly registered viewer the default.
    let probe = probe_file()?;
    let before = default_app(&probe).unwrap_or_default();
    run_tool(LSREGISTER, &["-f".as_ref(), app.as_os_str()])?;
    let after = default_app(&probe).unwrap_or_default();
    if !options.make_default && after.ends_with(BUNDLE_ID) && !before.ends_with(BUNDLE_ID) {
        notes.push("macOS made Folio the default app for Markdown when it was registered.".into());
    }

    let default = options.make_default.then(|| set_default_app(&probe, &app).map_err(|e| format!("{e:#}")));
    Ok(Installed { app, folio, terminal, default, notes })
}

pub struct Uninstalled {
    pub app: Option<PathBuf>,
    pub notes: Vec<String>,
}

/// Unregister and delete Folio.app.
pub fn uninstall(app_dir: Option<&Path>) -> Result<Uninstalled> {
    check_macos()?;
    let dir = match app_dir {
        Some(d) => d.to_path_buf(),
        None => home()?.join("Applications"),
    };
    let app = dir.join(format!("{APP_NAME}.app"));
    let mut notes = Vec::new();
    if !app.exists() {
        return Ok(Uninstalled { app: None, notes });
    }
    if let Ok(probe) = probe_file()
        && default_app(&probe).is_ok_and(|d| d.ends_with(BUNDLE_ID))
    {
        notes.push(
            "Folio was the default app for Markdown. To choose another: Get Info on a .md file, \
             pick an app under Open with, then Change All."
                .into(),
        );
    }
    // lsregister -u can print an error about Spotlight yet still unregister.
    let _ = run_tool(LSREGISTER, &["-u".as_ref(), app.as_os_str()]);
    remove_app(&app)?;
    let temp = user_temp_dir();
    let _ = std::fs::remove_file(temp.join("folio-probe.md"));
    clean_command_files(&temp, std::time::Duration::ZERO);
    Ok(Uninstalled { app: Some(app), notes })
}

fn check_macos() -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("opening files from Finder needs macOS");
    }
    let out = Command::new("/usr/bin/sw_vers").arg("-productVersion").output().context("can't run sw_vers")?;
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !version_at_least(&version, "13") {
        bail!("opening files from Finder needs macOS 13 or later (this is {version})");
    }
    Ok(())
}

/// Delete an existing Folio.app, but only if it is the one folio built.
fn remove_app(app: &Path) -> Result<()> {
    let id = plist_value(&app.join("Contents/Info.plist"), "CFBundleIdentifier").ok();
    if id.as_deref() != Some(BUNDLE_ID) {
        bail!("{} exists and wasn't made by folio; not replacing it", app.display());
    }
    // The script was made read-only for signing; let it be deleted.
    let script = app.join("Contents/Resources/Scripts/main.scpt");
    if let Ok(meta) = std::fs::metadata(&script) {
        let mut perms = meta.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        let _ = std::fs::set_permissions(&script, perms);
    }
    std::fs::remove_dir_all(app).with_context(|| format!("can't remove {}", app.display()))
}

fn plist_value(plist: &Path, key: &str) -> Result<String> {
    let out = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(plist)
        .output()
        .context("can't run plutil")?;
    if !out.status.success() {
        bail!("{key} isn't in {}", plist.display());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Run the NSWorkspace helper with `args`. It runs from a file, as in CI:
/// `osascript -e` documents one line of script per option.
fn helper(args: &[&std::ffi::OsStr]) -> Result<String> {
    let script = user_temp_dir().join(format!("folio-helper-{}.js", std::process::id()));
    std::fs::write(&script, HELPER).with_context(|| format!("can't write {}", script.display()))?;
    let out = Command::new("/usr/bin/osascript")
        .args(["-l".as_ref(), "JavaScript".as_ref(), script.as_os_str()])
        .args(args)
        .stdin(Stdio::null())
        .output();
    let _ = std::fs::remove_file(&script);
    let out = out.context("can't run osascript")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The installed app with this bundle id: its path and version.
fn find_app(bundle_id: &str) -> Result<(PathBuf, String)> {
    let found = helper(&["app".as_ref(), bundle_id.as_ref()])?;
    match found.split_once('\t') {
        Some((path, version)) if !path.is_empty() => Ok((PathBuf::from(path), version.to_string())),
        _ => bail!("no app with bundle id {bundle_id}"),
    }
}

/// "<path>\t<bundle id>" of the app that opens `file`.
fn default_app(file: &Path) -> Result<String> {
    helper(&["get".as_ref(), file.as_os_str()])
}

fn set_default_app(probe: &Path, app: &Path) -> Result<()> {
    match helper(&["set".as_ref(), probe.as_os_str(), app.as_os_str(), "120".as_ref()])?.as_str() {
        "ok" => Ok(()),
        _ => bail!("macOS kept the current app (you chose Keep, or didn't answer)"),
    }
}

/// A Markdown file to ask Launch Services about.
fn probe_file() -> Result<PathBuf> {
    let probe = user_temp_dir().join("folio-probe.md");
    std::fs::write(&probe, "# folio\n").with_context(|| format!("can't write {}", probe.display()))?;
    Ok(probe)
}

/// The per-user temporary folder (mode 0700). Apps started by Finder may not
/// have TMPDIR set, so ask getconf.
fn user_temp_dir() -> PathBuf {
    Command::new("/usr/bin/getconf")
        .arg("DARWIN_USER_TEMP_DIR")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
        .filter(|p| p.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}

fn run_tool(program: &str, args: &[&std::ffi::OsStr]) -> Result<()> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("can't run {program}"))?;
    if !out.status.success() {
        let name = Path::new(program).file_name().map_or(program.into(), |n| n.to_string_lossy());
        bail!("{name} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from).context("HOME isn't set")
}

// ----- launching ----------------------------------------------------------

/// Where the launcher writes what it did, for troubleshooting.
pub fn log_path(home: &Path) -> PathBuf {
    home.join("Library/Logs/folio-finder.log")
}

/// Open a new terminal window running folio on `file` (or the file picker
/// on a folder). This is what Folio.app runs. It returns as soon as the
/// window is requested: the applet waits for it.
pub fn launch_terminal(terminal: Terminal, file: Option<&Path>) -> Result<()> {
    let folio = std::env::current_exe().context("can't find folio's own path")?;
    let plan = launch_plan(terminal, &folio, file);
    let result = run_plan(&plan);
    log_launch(terminal, &plan, &result);
    result
}

fn run_plan(plan: &Launch) -> Result<()> {
    let args: Vec<String> = match plan {
        Launch::Open(args) => args.clone(),
        Launch::CommandFile { bundle_id, script } => {
            let dir = user_temp_dir();
            clean_command_files(&dir, std::time::Duration::from_secs(600));
            let path = write_command_file(&dir, script)?;
            vec!["-b".into(), bundle_id.to_string(), path.to_string_lossy().into_owned()]
        }
    };
    // Detach all three streams: the applet's `do shell script` waits until
    // they close, so a terminal holding them would stall Finder's request.
    let out = Command::new("/usr/bin/open")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .context("can't run open")?;
    if !out.status.success() {
        bail!("open failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// Write a `.command` file readable and runnable only by the user. The
/// script deletes itself when it runs; the terminal opens it later, so the
/// launcher can't.
fn write_command_file(dir: &Path, script: &str) -> Result<PathBuf> {
    use std::io::Write;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let path = dir.join(format!("folio-{}-{stamp}.command", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o700);
    }
    let mut f = options.open(&path).with_context(|| format!("can't create {}", path.display()))?;
    f.write_all(script.as_bytes())?;
    Ok(path)
}

/// Delete `folio-*.command` files older than `age` that never ran.
fn clean_command_files(dir: &Path, age: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let old = e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|d| d >= age));
        if name.starts_with("folio-") && name.ends_with(".command") && old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Append one line about a launch to the log, keeping the log small.
fn log_launch(terminal: Terminal, plan: &Launch, result: &Result<()>) {
    use std::io::Write;
    let Ok(home) = home() else { return };
    let path = log_path(&home);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 256 * 1024) {
        let _ = std::fs::remove_file(&path);
    }
    let what = match plan {
        Launch::Open(args) => format!("open {}", args.join(" ")),
        Launch::CommandFile { bundle_id, script } => {
            format!("open -b {bundle_id} <command file: {}>", script.lines().last().unwrap_or_default())
        }
    };
    let tmpdir = if std::env::var_os("TMPDIR").is_some() { "set" } else { "unset" };
    let outcome = match result {
        Ok(()) => "ok".to_string(),
        Err(e) => format!("error: {e:#}"),
    };
    let stamp = crate::state::now();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{stamp} {} tmpdir={tmpdir} {what} -> {outcome}", terminal.name());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn quotes_for_the_shell() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("it's here"), r"'it'\''s here'");
        assert_eq!(shell_quote("$HOME `x` \"y\""), "'$HOME `x` \"y\"'");
    }

    #[test]
    fn quotes_for_applescript() {
        assert_eq!(applescript_string(r#"/a "b"\c"#), r#""/a \"b\"\\c""#);
    }

    #[test]
    fn command_script_runs_folio_and_cleans_up() {
        let script = command_script(&["/opt/homebrew/bin/folio".into(), "/Users/ann/Notes/It's Q3.md".into()]);
        assert_eq!(script, "#!/bin/sh\nrm -f \"$0\"\nexec '/opt/homebrew/bin/folio' '/Users/ann/Notes/It'\\''s Q3.md'\n");
    }

    #[test]
    fn detects_the_terminal_from_its_own_variables_first() {
        let detect = |pairs: &[(&str, &str)]| {
            let map: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            Terminal::detect(&|k: &str| map.get(k).cloned())
        };
        assert_eq!(detect(&[("TERM_PROGRAM", "iTerm.app")]), Terminal::ITerm);
        assert_eq!(detect(&[("ITERM_SESSION_ID", "w0t0p0"), ("TERM_PROGRAM", "tmux")]), Terminal::ITerm);
        assert_eq!(detect(&[("TERM_PROGRAM", "ghostty")]), Terminal::Ghostty);
        assert_eq!(detect(&[("GHOSTTY_RESOURCES_DIR", "/Applications/Ghostty.app/x"), ("TERM_PROGRAM", "tmux")]), Terminal::Ghostty);
        assert_eq!(detect(&[("WEZTERM_PANE", "0")]), Terminal::WezTerm);
        assert_eq!(detect(&[("TERM", "xterm-kitty")]), Terminal::Kitty);
        assert_eq!(detect(&[("KITTY_WINDOW_ID", "1"), ("TERM_PROGRAM", "tmux")]), Terminal::Kitty);
        assert_eq!(detect(&[("ALACRITTY_WINDOW_ID", "4")]), Terminal::Alacritty);
        assert_eq!(detect(&[("TERM_PROGRAM", "Apple_Terminal")]), Terminal::Terminal);
        assert_eq!(detect(&[("TERM_PROGRAM", "vscode")]), Terminal::Terminal);
        assert_eq!(detect(&[("TERM_PROGRAM", "WarpTerminal")]), Terminal::Terminal);
        assert_eq!(Terminal::from_name("iTerm2"), Some(Terminal::ITerm));
        assert_eq!(Terminal::from_name("nope"), None);
    }

    #[test]
    fn launches_each_terminal_with_paths_as_separate_arguments() {
        let folio = Path::new("/opt/homebrew/bin/folio");
        let file = Path::new("/Users/ann/My Notes/a b.md");
        let open = |t: Terminal| match launch_plan(t, folio, Some(file)) {
            Launch::Open(args) => args,
            other => panic!("{other:?}"),
        };
        let tail = ["/opt/homebrew/bin/folio", "--paging", "always", "/Users/ann/My Notes/a b.md"];
        let with = |head: &[&str]| head.iter().chain(tail.iter()).map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(open(Terminal::Ghostty), with(&["-n", "-b", "com.mitchellh.ghostty", "--args", "-e"]));
        assert_eq!(open(Terminal::Alacritty), with(&["-n", "-b", "org.alacritty", "--args", "-e"]));
        assert_eq!(
            open(Terminal::Kitty),
            with(&["-n", "-b", "net.kovidgoyal.kitty", "--args", "-o", "macos_quit_when_last_window_closed=yes"])
        );
        assert_eq!(open(Terminal::WezTerm), with(&["-n", "-b", "com.github.wez.wezterm", "--args", "start", "--"]));
        for t in [Terminal::Terminal, Terminal::ITerm] {
            match launch_plan(t, folio, Some(file)) {
                Launch::CommandFile { bundle_id, script } => {
                    assert_eq!(bundle_id, t.bundle_id());
                    assert!(script.ends_with("exec '/opt/homebrew/bin/folio' '--paging' 'always' '/Users/ann/My Notes/a b.md'\n"));
                }
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn applet_hands_files_to_folio_without_properties() {
        let src = applet_source(Path::new("/Users/me/.cargo/bin/folio"), Terminal::Kitty);
        assert!(src.contains(r#"(quoted form of "/Users/me/.cargo/bin/folio") & " --launch-terminal --terminal kitty " & (quoted form of target)"#));
        assert!(src.contains("on open theItems"));
        assert!(src.contains("display alert"));
        assert!(!src.lines().any(|l| l.trim_start().starts_with("property ")));
    }

    #[test]
    fn document_types_claim_markdown_by_type_and_by_extension() {
        let xml = document_types_xml();
        assert_eq!(xml.matches("<dict>").count(), 2);
        assert!(xml.contains(&format!("<key>LSItemContentTypes</key><array><string>{MARKDOWN_UTI}</string></array>")));
        assert!(xml.contains("<key>CFBundleTypeExtensions</key><array><string>md</string>"));
        assert_eq!(xml.matches("<string>Viewer</string>").count(), 2);
        assert_eq!(xml.matches("<string>Alternate</string>").count(), 2);
        assert!(!xml.contains("public.plain-text") && !xml.contains("<string>*</string>"));
        for tag in ["array", "dict", "key", "string"] {
            assert_eq!(xml.matches(&format!("<{tag}>")).count(), xml.matches(&format!("</{tag}>")).count(), "{tag}");
        }
    }

    #[test]
    fn stores_a_stable_homebrew_path() {
        let cellar = Path::new("/opt/homebrew/Cellar/folio/0.1.0/bin/folio");
        assert_eq!(stored_folio_path(cellar, &|p: &Path| p == Path::new("/opt/homebrew/bin/folio")), PathBuf::from("/opt/homebrew/bin/folio"));
        assert_eq!(stored_folio_path(cellar, &|_: &Path| false), cellar.to_path_buf());
        let cargo = Path::new("/Users/me/.cargo/bin/folio");
        assert_eq!(stored_folio_path(cargo, &|_: &Path| true), cargo.to_path_buf());
    }

    #[test]
    fn compares_versions() {
        assert!(version_at_least("1.4.0", "1.4.0"));
        assert!(version_at_least("1.10.0", "1.4.0"));
        assert!(!version_at_least("1.3.1", "1.4.0"));
        assert!(version_at_least("26.0.1", "13"));
        assert!(!version_at_least("12.7", "13"));
        assert!(!version_at_least("", "1.4.0"));
    }
}

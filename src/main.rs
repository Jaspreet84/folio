use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};

use folio::highlight::Highlighter;
use folio::pager::Pager;
use folio::theme::{self, Depth, Theme};
use folio::{ansi, doc, layout};

/// Read Markdown in the terminal.
#[derive(Parser)]
#[command(name = "folio", version)]
struct Args {
    /// Markdown file to read. A directory opens its README. Reads stdin when
    /// no file is given or the file is "-".
    file: Option<PathBuf>,

    /// When to page the output. "auto" pages when output is a terminal and
    /// the document is longer than one screen.
    #[arg(long, value_enum, default_value_t = When::Auto)]
    paging: When,

    /// When to use colors. "auto" respects NO_COLOR.
    #[arg(long, value_enum, default_value_t = When::Auto)]
    color: When,

    /// Color theme. "auto" asks the terminal for its background color.
    #[arg(long, value_enum, default_value_t = ThemeChoice::Auto)]
    theme: ThemeChoice,

    /// Maximum reading width in columns (default 100; 80 when not printing
    /// to a terminal).
    #[arg(long, short = 'w')]
    width: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum When {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ThemeChoice {
    Auto,
    Dark,
    Light,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("folio: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    let (name, source) = read_input(args.file.as_deref())?;
    let stdout_tty = io::stdout().is_terminal();

    let no_color_env = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let color = match args.color {
        When::Always => true,
        When::Never => false,
        When::Auto => stdout_tty && !no_color_env,
    };
    let depth = if color { theme::detect_depth() } else { Depth::None };
    let dark = match args.theme {
        ThemeChoice::Dark => true,
        ThemeChoice::Light => false,
        ThemeChoice::Auto => !color || !stdout_tty || theme::detect_dark(),
    };
    let theme = Theme::new(dark, depth);
    let document = doc::parse(&source);
    let mut hl = Highlighter::new();

    let page = match args.paging {
        When::Always => {
            if !stdout_tty {
                bail!("--paging always needs a terminal on stdout");
            }
            true
        }
        When::Never => false,
        When::Auto => stdout_tty,
    };

    let (term_w, term_h) = if stdout_tty {
        ratatui::crossterm::terminal::size().map(|(w, h)| (usize::from(w), usize::from(h))).unwrap_or((80, 24))
    } else {
        let cols = std::env::var("COLUMNS").ok().and_then(|c| c.parse().ok()).unwrap_or(80);
        (cols, 24)
    };
    let max_width = args.width.unwrap_or(if stdout_tty { 100 } else { 80 }).max(10);

    if page {
        // Like `less -F`: a document that fits on one screen is just printed.
        if args.paging == When::Auto {
            let rendered = layout::layout(&document, max_width.min(term_w), &theme, &mut hl);
            if rendered.lines.len() < term_h {
                return print(&rendered, color);
            }
        }
        let mut terminal = ratatui::try_init().context("can't set up the terminal")?;
        // Alternate scroll mode: the mouse wheel sends arrow keys, so wheel
        // scrolling works without capturing the mouse (selection still works).
        let _ = io::stdout().write_all(b"\x1b[?1007h").and_then(|()| io::stdout().flush());
        let result = Pager::new(name, source, document, theme, hl, max_width).run(&mut terminal);
        let _ = io::stdout().write_all(b"\x1b[?1007l").and_then(|()| io::stdout().flush());
        ratatui::restore();
        result.context("terminal error")
    } else {
        let rendered = layout::layout(&document, max_width.min(term_w), &theme, &mut hl);
        print(&rendered, color)
    }
}

fn print(rendered: &layout::Rendered, color: bool) -> Result<()> {
    let text = ansi::render(rendered, color);
    let mut out = io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        // A closed pipe (`folio x.md | head`) is not an error.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        other => other.context("can't write output"),
    }
}

fn read_input(path: Option<&Path>) -> Result<(String, String)> {
    let read_stdin = || -> Result<(String, String)> {
        let mut bytes = Vec::new();
        io::stdin().read_to_end(&mut bytes).context("can't read stdin")?;
        Ok(("stdin".into(), String::from_utf8_lossy(&bytes).into_owned()))
    };
    match path {
        Some(p) if p == Path::new("-") => read_stdin(),
        Some(p) => {
            let file = if p.is_dir() { find_readme(p)? } else { p.to_path_buf() };
            let bytes = std::fs::read(&file).with_context(|| format!("can't read {}", file.display()))?;
            let name = file.file_name().map_or_else(|| file.display().to_string(), |n| n.to_string_lossy().into_owned());
            Ok((name, String::from_utf8_lossy(&bytes).into_owned()))
        }
        None if io::stdin().is_terminal() => {
            bail!("no file given\n\nUsage: folio FILE\n       command | folio\n\nRun `folio --help` for options.")
        }
        None => read_stdin(),
    }
}

fn find_readme(dir: &Path) -> Result<PathBuf> {
    for name in ["README.md", "readme.md", "Readme.md", "README.markdown", "README", "index.md"] {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    bail!("{} is a directory with no README.md", dir.display())
}

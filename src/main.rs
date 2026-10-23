use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};

use folio::highlight::Highlighter;
use folio::pager::{Options, Page, Pager, Start};
use folio::theme::{self, Depth, Theme};
use folio::{ansi, layout, links, state};

/// Read Markdown in the terminal.
#[derive(Parser)]
#[command(name = "folio", version)]
struct Args {
    /// Markdown file to read, or a folder to pick a file from. With no
    /// argument, reads stdin if something is piped in, or else shows the
    /// Markdown files in the current folder. "-" reads stdin.
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

    /// Don't reload the document when its file changes.
    #[arg(long)]
    no_watch: bool,
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
    let mut theme = Theme::new(dark, depth);
    theme.hyperlinks = color && theme::detect_hyperlinks();
    let mut hl = Highlighter::new();

    let page_mode = match args.paging {
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
    let options = Options { max_width, watch: !args.no_watch, state_file: state::default_path() };

    let folder = match args.file.as_deref() {
        Some(p) if p.is_dir() => Some(p.to_path_buf()),
        None if io::stdin().is_terminal() => Some(PathBuf::from(".")),
        _ => None,
    };
    let page = match (folder, args.file.as_deref()) {
        (Some(dir), _) if page_mode => {
            let dir = dir.canonicalize().with_context(|| format!("can't open {}", dir.display()))?;
            return run_pager(Start::Picker(dir), theme, hl, options);
        }
        (Some(dir), _) => {
            let readme = find_readme(&dir)?;
            Page::open(&readme).with_context(|| format!("can't read {}", readme.display()))?
        }
        (None, Some(p)) if p != Path::new("-") => {
            Page::open(p).with_context(|| format!("can't read {}", p.display()))?
        }
        (None, _) => {
            let mut bytes = Vec::new();
            io::stdin().read_to_end(&mut bytes).context("can't read stdin")?;
            Page::from_text("stdin".into(), String::from_utf8_lossy(&bytes).into_owned())
        }
    };

    let width = max_width.min(term_w);
    if page_mode {
        // Like `less -F`: a document that fits on one screen is just printed.
        if args.paging == When::Auto {
            let rendered = layout::layout(page.document(), width, &theme, &mut hl);
            if rendered.lines.len() < term_h {
                return print(&rendered, color, &theme, page.dir());
            }
        }
        run_pager(Start::Page(page), theme, hl, options)
    } else {
        let rendered = layout::layout(page.document(), width, &theme, &mut hl);
        print(&rendered, color, &theme, page.dir())
    }
}

fn run_pager(start: Start, theme: Theme, hl: Highlighter, options: Options) -> Result<()> {
    let mut terminal = ratatui::try_init().context("can't set up the terminal")?;
    // Alternate scroll mode: the mouse wheel sends arrow keys, so wheel
    // scrolling works without capturing the mouse (selection still works).
    let _ = io::stdout().write_all(b"\x1b[?1007h").and_then(|()| io::stdout().flush());
    let result = Pager::new(start, theme, hl, options).run(&mut terminal);
    let _ = io::stdout().write_all(b"\x1b[?1007l").and_then(|()| io::stdout().flush());
    ratatui::restore();
    result.context("terminal error")
}

fn print(rendered: &layout::Rendered, color: bool, theme: &Theme, base: Option<&Path>) -> Result<()> {
    let urls: Vec<Option<String>> = if theme.hyperlinks {
        rendered.links.iter().map(|u| links::osc8_target(u, base)).collect()
    } else {
        Vec::new()
    };
    let text = ansi::render(rendered, color, &urls);
    let mut out = io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        // A closed pipe (`folio x.md | head`) is not an error.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        other => other.context("can't write output"),
    }
}

fn find_readme(dir: &Path) -> Result<PathBuf> {
    for name in ["README.md", "readme.md", "Readme.md", "README.markdown", "README", "index.md"] {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    bail!("{} is a folder with no README.md (in a terminal, folio lists its Markdown files)", dir.display())
}

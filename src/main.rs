use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser, ValueEnum};

use folio::finder::{self, Terminal};
use folio::highlight::Highlighter;
use folio::pager::{Options, Page, Pager, Start};
use folio::theme::{self, Depth, Theme};
use folio::{ansi, layout, links, state};

const EXAMPLES: &str = "\
Examples:
  folio README.md             Read a file
  folio .                     Pick a Markdown file in this folder
  folio docs/                 Pick a Markdown file in docs/
  curl -s URL | folio         Read from a pipe
  folio --paging never x.md   Print instead of paging, like cat

While reading, press ? to see the keys.";

/// Read Markdown in the terminal.
#[derive(Parser)]
#[command(name = "folio", version, about, after_help = EXAMPLES)]
struct Args {
    /// Markdown file to read, or a folder to pick a file from ("." for the
    /// current folder). Reads stdin when something is piped in or the file
    /// is "-".
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

    /// Add Folio to Finder's "Open With" menu for Markdown files, so they
    /// open in folio in a new terminal window (macOS).
    #[arg(long, help_heading = "Finder (macOS)", conflicts_with = "uninstall_finder")]
    install_finder: bool,

    /// Remove Folio from Finder (macOS).
    #[arg(long, help_heading = "Finder (macOS)")]
    uninstall_finder: bool,

    /// The terminal Finder opens files in (default: the one you run
    /// --install-finder from).
    #[arg(long, value_enum, value_name = "NAME", help_heading = "Finder (macOS)")]
    terminal: Option<Terminal>,

    /// With --install-finder: also make Folio the default app for Markdown,
    /// so double-clicking a file opens it.
    #[arg(long, requires = "install_finder", help_heading = "Finder (macOS)")]
    make_default: bool,

    /// Where to put Folio.app (default ~/Applications).
    #[arg(long, value_name = "DIR", help_heading = "Finder (macOS)")]
    app_dir: Option<PathBuf>,

    /// Used by Folio.app: open FILE in folio in a new terminal window.
    #[arg(long, hide = true)]
    launch_terminal: bool,
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
            // Errors can quote file names; never let one send escape
            // sequences to the terminal.
            eprintln!("folio: {}", folio::doc::clean(&format!("{e:#}")));
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    if args.launch_terminal {
        return finder::launch_terminal(args.terminal.unwrap_or(Terminal::Terminal), args.file.as_deref());
    }
    if args.install_finder {
        return install_finder(&args);
    }
    if args.uninstall_finder {
        let done = finder::uninstall(args.app_dir.as_deref())?;
        match done.app {
            Some(app) => println!("Removed {}.", app.display()),
            None if done.notes.is_empty() => println!("Folio.app isn't installed."),
            None => {}
        }
        for note in done.notes {
            println!("{note}");
        }
        return Ok(());
    }
    // Nothing to read: no file and nothing piped in. Show how to use folio
    // instead of waiting on the keyboard or scanning the current folder.
    if args.file.is_none() && io::stdin().is_terminal() {
        let color = match args.color {
            When::Auto => clap::ColorChoice::Auto,
            When::Always => clap::ColorChoice::Always,
            When::Never => clap::ColorChoice::Never,
        };
        return match Args::command().color(color).print_help() {
            // `folio | head -1` closes the pipe early; that's not an error.
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
            other => other.context("can't write the help"),
        };
    }
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
        // Some terminals (and serial consoles) report 0x0; treat that as unknown.
        ratatui::crossterm::terminal::size()
            .ok()
            .filter(|&(w, h)| w > 0 && h > 0)
            .map_or((80, 24), |(w, h)| (usize::from(w), usize::from(h)))
    } else {
        let cols = std::env::var("COLUMNS").ok().and_then(|c| c.parse().ok()).unwrap_or(80);
        (cols, 24)
    };
    let max_width = args.width.unwrap_or(if stdout_tty { 100 } else { 80 }).max(10);
    let options = Options { max_width, watch: !args.no_watch, state_file: state::default_path() };

    let folder = args.file.as_deref().filter(|p| p.is_dir()).map(Path::to_path_buf);
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

fn install_finder(args: &Args) -> Result<()> {
    let terminal = args.terminal.unwrap_or_else(|| Terminal::detect(&|k: &str| std::env::var(k).ok()));
    let options = finder::InstallOptions { app_dir: args.app_dir.clone(), terminal };
    let done = finder::install(&options)?;
    let by_hand = "Get Info on a .md file, choose Folio under Open with, then Change All.";
    println!("Installed {}", done.app.display());
    println!("  Opens files in {} with {}.", done.terminal.app_name(), done.folio.display());
    for note in &done.notes {
        println!("  {note}");
    }
    println!();
    println!("In Finder, right-click a Markdown file and choose Open With > Folio.");
    if done.is_default {
        println!("Folio is the default app for Markdown, so double-clicking a file opens it.");
    } else if args.make_default {
        println!();
        println!("Making Folio the default app for Markdown. macOS may ask you to confirm;");
        println!("the dialog can open behind this window. Waiting up to a minute...");
        io::stdout().flush().ok();
        match finder::make_default(&done.app) {
            Ok(()) => println!("Done: double-clicking a Markdown file now opens it in folio."),
            Err(e) => {
                println!("Folio isn't the default ({}).", folio::doc::clean(&format!("{e:#}")));
                println!("To do it by hand: {by_hand}");
            }
        }
    } else {
        println!("To open Markdown files with a double-click, run this again with --make-default,");
        println!("or do it by hand: {by_hand}");
    }
    println!();
    println!("If you move folio, run --install-finder again. Remove it with --uninstall-finder.");
    Ok(())
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

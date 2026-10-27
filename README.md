# folio

Read Markdown in the terminal. folio lays out headings, lists, tables and
code blocks for the width of your terminal, and gives you the tools you need
for long documents and folders of them: a contents pane, jumping between
headings, search, links you can follow and come back from, marks, and a
memory of where you stopped reading.

Phases 1 and 2 of [PLAN.md](PLAN.md) are done. Targets macOS and Linux
terminals.

## Build

```sh
cargo build --release
./target/release/folio README.md
```

## Use

```sh
folio .                     # pick a Markdown file in the current folder
folio FILE.md               # read a file
folio docs/                 # pick a file in a folder
curl -s URL | folio         # read from stdin
folio --paging never FILE   # print, like cat
folio --color never FILE    # plain text, no escape codes
folio --width 80 FILE       # narrower reading width (default 100)
folio --theme light FILE    # force the light theme (default: ask the terminal)
folio --no-watch FILE       # don't reload when the file changes
```

Documents that fit on one screen are printed instead of paged. Open files
reload by themselves when they change on disk.

## Keys

| Keys | Action |
|---|---|
| `j` `k` `↓` `↑`, mouse wheel | Scroll one line |
| `space` `f` / `b` | Page down / up |
| `d` / `u` | Half page down / up |
| `g` `G` | Top / bottom |
| `50%` | Go to a percentage |
| `]]` / `[[` (or `}` / `{`) | Next / previous heading |
| `t` | Contents pane |
| `Ctrl-P` | Jump to a heading by name |
| `/`, `n` / `N` | Search, next / previous match |
| `Tab` / `Shift-Tab` | Next / previous link or code block |
| `Enter` | Follow the link, or copy the code block |
| `y` | Copy the link or code block |
| `H` / `L` | Back / forward |
| `m a` / `' a` | Set mark `a` / go to it (`A`-`Z` work across files) |
| `o` | File picker |
| `r` / `R` | Raw Markdown / reload |
| `Esc` | Clear focus and search highlights |
| `?` | Help |
| `q` | Quit |

A number before a motion repeats it: `5j`, `3]]`.

Links to other Markdown files open in folio; web links open in your browser.
Local files that aren't Markdown are never opened, so a document can't make
you launch a program by accident.

## Open Markdown files from Finder (macOS)

```sh
folio --install-finder                  # add Folio to Finder's Open With menu
folio --install-finder --make-default   # also open .md files with a double-click
folio --install-finder --terminal kitty # choose the terminal (default: the one you're in)
folio --uninstall-finder
```

This builds a small app, `~/Applications/Folio.app`, using tools that come
with macOS (`--app-dir DIR` puts it somewhere else). Right-click a Markdown
file and choose **Open With > Folio**, and it opens in folio in a new
terminal window. Opening Folio itself asks you to choose a file. If folio
can't start, the window stays open to show why.

- **Terminals:** Terminal, iTerm2, Ghostty (1.4 or later), kitty, WezTerm
  and Alacritty. iTerm2 asks before running each file until you tell it not
  to.
- **Double-click:** `--make-default` asks macOS to make Folio the default
  app for `.md` and `.markdown` files. macOS 26.4 and later ask you to
  confirm, and the dialog can open behind other windows. You can also do it
  by hand: Get Info on a `.md` file, choose Folio under Open with, then
  Change All.
- **Moving folio:** the app remembers where folio is, so run
  `--install-finder` again if you move or reinstall it. Homebrew upgrades
  are fine.
- **Troubleshooting:** each launch is logged to
  `~/Library/Logs/folio-finder.log`.

## Where folio keeps things

Reading positions and marks go in `~/.local/state/folio/state.json` on Linux
and `~/Library/Application Support/folio/state.json` on macOS (or
`$XDG_STATE_HOME/folio/`, or the file named by `FOLIO_STATE_FILE`). The file
holds the paths of the last 500 files you read; delete it to forget them.

## Test

```sh
cargo test
```

Layout snapshots live in `tests/snapshots`. After an intended rendering
change, review and accept them with `cargo insta review` (or rerun with
`INSTA_UPDATE=always`).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution you submit for
inclusion in folio, as defined in the Apache-2.0 license, is dual-licensed
as above, without any additional terms or conditions.

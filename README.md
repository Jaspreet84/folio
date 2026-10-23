# folio

Read Markdown in the terminal. folio lays out headings, lists, tables and
code blocks for the width of your terminal, and gives you the tools you need
for long documents: a contents pane, jumping between headings, search, and a
status bar that shows where you are.

Phase 1 of [PLAN.md](PLAN.md). Targets macOS and Linux terminals.

## Build

```sh
cargo build --release
./target/release/folio README.md
```

## Use

```sh
folio FILE.md               # read a file
folio docs/                 # read a folder's README
curl -s URL | folio         # read from stdin
folio --paging never FILE   # print, like cat
folio --color never FILE    # plain text, no escape codes
folio --width 80 FILE       # narrower reading width (default 100)
folio --theme light FILE    # force the light theme (default: ask the terminal)
```

Documents that fit on one screen are printed instead of paged.

## Keys

| Keys | Action |
|---|---|
| `j` `k` `↓` `↑` `Enter`, mouse wheel | Scroll one line |
| `space` `f` / `b` | Page down / up |
| `d` / `u` | Half page down / up |
| `g` `G` | Top / bottom |
| `50%` | Go to a percentage |
| `]]` / `[[` (or `}` / `{`) | Next / previous heading |
| `t`, `Tab` | Contents pane |
| `Ctrl-P` | Jump to a heading by name |
| `/`, `n` / `N` | Search, next / previous match |
| `Esc` | Clear search highlights |
| `r` | Raw Markdown on / off |
| `?` | Help |
| `q` | Quit |

A number before a motion repeats it: `5j`, `3]]`.

## Test

```sh
cargo test
```

Layout snapshots live in `tests/snapshots`. After an intended rendering
change, review and accept them with `cargo insta review` (or rerun with
`INSTA_UPDATE=always`).

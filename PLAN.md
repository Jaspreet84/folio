# folio: a Markdown reader for the terminal

`folio` renders Markdown files in a Mac or Linux terminal and lets you read
long documents comfortably: move by section, search, see where you are, and
see tables and diagrams laid out properly instead of as raw pipes and fences.

## Why build it

Existing tools each cover part of the job:

- **glow** (Go) looks good and has a file browser, but wide tables break badly,
  there are no diagrams, and you can't jump between sections.
- **frogmouth** (Python/Textual) has a contents sidebar and history, but no
  images or diagrams.
- **mdcat** and **md-tui** (Rust) can show images, but navigation is basic.

None of them handles wide tables, diagrams, and navigation of long documents
together. That combination is the reason for this tool.

## Stack

| Concern | Choice | Why |
|---|---|---|
| Language | Rust | One static binary for Mac and Linux; starts in under 20 ms |
| Parser | `comrak` | Full GFM tree: tables, task lists, footnotes, alerts, math, front matter |
| TUI | `ratatui` + `crossterm` | Mature, fast, works over SSH and in tmux |
| Code highlighting | `syntect` + `two-face` | bat's syntax and theme set; pure-Rust regex (no C dependency) |
| Text width | `unicode-width`, `unicode-segmentation` | CJK and emoji keep tables aligned |
| Light/dark detection | `terminal-colorsaurus` | Asks the terminal for its background color (OSC 11) |
| Snapshot tests | `insta` | Layout output is plain text, so snapshots are easy to review |

## Architecture

```
file / stdin
   │
   ▼
parse (comrak) ──► document model (src/doc.rs)
   │
   ▼
layout(doc, width, theme) ──► lines[] + index (headings, search text, source lines)
   │
   ▼
pager (ratatui): viewport, input, search, contents pane, status bars
   or
print (ANSI or plain text) when output is not a terminal
```

The key decision: layout is a pure function from (document, width, theme) to
styled lines. The pager only scrolls those lines. This makes several things
simple:

- **Resize:** run layout again and keep the reader's place by pinning the
  scroll position to a source line and an offset inside it, not to a line
  number.
- **Search and contents:** both read from the index that layout builds.
- **Tests:** the output is text, so snapshot tests and invariants ("no line is
  wider than the width") are easy to write.

Search runs over *flows*: each paragraph, table cell or code line is one flow,
with its wrapped pieces joined back together. A phrase that wraps across two
lines is still found.

Slow work (images, diagrams) will run in background jobs in Phase 3. Layout
reserves a placeholder of the right size and swaps it in when ready.

## Rendering rules

- **Headings:** H1 and H2 get a rule underneath; each level has its own color.
- **Paragraphs:** wrapped to a maximum reading width (default 100 columns),
  centered on wide terminals.
- **Lists:** hanging indents, a different bullet per depth, right-aligned
  numbers, ☐/☑ for task items.
- **Code blocks:** rounded box with the language in the top border, syntax
  highlighting, long lines wrapped with a `↪` marker.
- **Tables:** the browser "auto" layout:
  1. Each column gets a minimum (longest word, capped at 20) and a maximum
     (whole cell on one line).
  2. If every column fits at its maximum, use that.
  3. Otherwise give every column its minimum and share the rest in proportion
     to how much each column wants.
  4. If the minimums still don't fit, shrink columns down to a floor of 6,
     breaking long words.
  5. If even that doesn't fit, switch to a stacked "record" view
     (`Header: value` per row).
  Rows get separators when any row wraps to more than one line.
- **Quotes and alerts:** colored left bar; GitHub alerts (`> [!NOTE]`) get a
  text label so meaning doesn't depend on color alone.
- **Links:** underlined; the URL is shown dimmed after the text (until Phase 2
  adds OSC 8 links and link focus).
- **Images:** a placeholder with the alt text (real images in Phase 3).
- **Footnotes, front matter, description lists, math, HTML:** footnotes are
  listed at the end with `[n]` markers; front matter is a dim metadata block;
  math is shown as source; HTML tags are stripped and their text is kept.

## Keys (Phase 1)

| Keys | Action |
|---|---|
| `j`/`k`, `↓`/`↑`, `Enter`, mouse wheel | Scroll one line |
| `space`/`f`/`PgDn`, `b`/`PgUp` | Page down / up |
| `d` / `u` | Half page down / up |
| `g`/`Home`, `G`/`End` | Top / bottom |
| `N%` (e.g. `50%`) | Jump to a percentage |
| `]]` / `[[` | Next / previous heading |
| `t` | Contents pane (open, focus, close) |
| `Ctrl-P` | Contents pane with a filter for jumping to a heading |
| `/`, `n`/`N` | Search (smart case, live highlights, "3/17" count) |
| `r` | Toggle raw Markdown |
| `?` | Help |
| `q`, `Ctrl-C` | Quit |

A number before a motion repeats it (`5j`, `3]]`).

The mouse is not captured, so text selection works normally. Wheel scrolling
works through the terminal's "alternate scroll" mode, which turns wheel events
into arrow keys.

## Command line

```
folio [FILE]                # read a file (a directory opens its README)
cat FILE | folio            # read from stdin
folio --paging never FILE   # print rendered output, like cat
folio --color never FILE    # no colors
folio --width 80 FILE       # set the reading width
folio --theme light FILE    # force a theme (default: ask the terminal)
```

When output is not a terminal, folio prints the rendered document once.
When the document fits on one screen, it is printed instead of paged (like
`less -F`), unless `--paging always` is set.

Colors: truecolor when `COLORTERM` says so, otherwise the nearest of the 256
xterm colors. `NO_COLOR` is respected.

## Build order

1. **Core reader** (this phase): parsing, layout, pager, code highlighting,
   tables that fit and wrap, search, contents pane, resize handling, raw view,
   pipe mode, snapshot tests.
2. **Navigation:** OSC 8 links, a focus model (`Tab` moves between links,
   code blocks, tables and diagrams; `Enter` activates), back/forward history,
   opening a folder with a file picker, marks, remembered positions per file,
   watch mode, copying code blocks (OSC 52), TOML config with key remapping.
3. **Rich content:** images (Kitty, iTerm2, Sixel, half-block fallback),
   Mermaid (native text renderer for flowcharts and sequence diagrams; `mmdc`
   when installed), Graphviz, math as Unicode, a full-screen table viewer with
   frozen header and first column, folding sections and `<details>`.
4. **Polish:** speed on very large files (lazy layout per block), packaging
   with `cargo-dist` (GitHub releases, Homebrew tap, static musl builds),
   man page and docs.

## Phase 1 status (2026-10-23)

Done and tested on Linux (WSL Ubuntu, tmux 3.4) and Windows:

- All rendering rules above, the keys above, pipe mode, stdin input,
  printing short documents instead of paging.
- 31 tests: unit tests, 16 layout snapshots, the width invariant at every
  width from 20 to 140, search mapping, and keeping the reader's place across
  a relayout.
- The pager was driven through tmux (`send-keys`, `capture-pane`): heading
  jumps, contents pane, Ctrl-P filter, search with live count, raw view,
  resize, help, clean exit, 256-color fallback, piped stdin.

Measured on Linux (release build):

| Case | Time |
|---|---|
| Print a document with no highlighted code | ~6 ms |
| Print a document with highlighted code | ~160 ms (loading bat's syntax set) |
| Lay out and print a 1.4 MB document (50,000 lines) | ~0.4 s |

Known gaps, to pick up in later phases:

- Loading the full syntax set costs ~150 ms. Phase 4: load a smaller set
  first, or highlight in the background (Phase 3 adds background jobs).
- Links show their URL in dim text; OSC 8 links come in Phase 2.
- `<details>` content is always shown; folding comes in Phase 3.
- Light-theme detection and the colors were not checked on a real Mac.

## Testing

- Snapshot tests of layout output at widths 40, 80 and 120 (`tests/`).
- Invariant tests: no rendered line is wider than the layout width, at every
  width from 20 to 140, for every fixture.
- Unit tests for wrapping, table column widths and search mapping.
- Manual terminal check list: iTerm2, Terminal.app, Ghostty, kitty, WezTerm,
  Alacritty, GNOME Terminal, Konsole, plus tmux and SSH.

Development happens on Windows with WSL Ubuntu. The pager is tested in WSL
through tmux (`send-keys` and `capture-pane`). A final visual check needs a
real Mac.

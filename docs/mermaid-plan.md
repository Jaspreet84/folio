# Mermaid diagrams in folio: implementation plan

Date: 2026-10-23, updated 2026-10-28. Status: revision 2. Milestones M0 to M2 are built, two rounds of review and fixes have followed, and the owner decisions in section 12 are taken; see "Implementation status" below. Line numbers in `file:line` references are from 2026-10-23 and will drift as the code changes.

This plan replaces the Phase 3 line in PLAN.md, "Mermaid (native text renderer for flowcharts and sequence diagrams; `mmdc` when installed)", with a design that can be built. It starts from the "text-first" proposal, which both first-round reviewers ranked first.

Revision 2 answers the second review:

- No drawing may show a connection that is not in the source. Section 6 adds the rules that prevent false connections, and checks that enforce them.
- "Drawn" now means "fits the width and passes the checks", not only "fits the width".
- The headline numbers are measured again under that definition, on a held-out sample that played no part in building the prototype.
- Diagrams with subgraphs get an outline that lists their groups until frames are built. Frames are now their own milestone with their own gate.

Appendix A maps each review item to the place where this revision handles it.

**How claims are marked**

- **[V n]**: checked on 2026-10-23 in source *n*. The sources are listed at the end.
- **[M n]**: measured on 2026-10-23 (M1 to M11) or 2026-10-28 (M12 and M13, folio's own code). Appendix B gives the method and where the scripts are.
- **[U]**: unverified. This covers estimates, inferences and design guesses.
- `file:line`: folio's code on 2026-10-23.

---

## Implementation status (2026-10-28)

**Built.** Milestones M0, M1 and M2 are built on the `mermaid` branch, and two rounds of review and fixes followed. The work is not merged into master yet. Readers now get:

- flowcharts without subgraphs drawn, using every ladder step (decision 10);
- flowcharts with subgraphs as outlines that list their groups (frames are M3b);
- sequence diagrams as transcripts (drawing them is M4);
- the source with a "Not drawn: …" note for other types, unsupported syntax and unclosed fences;
- on a focused diagram: `s` switches between drawing and source, `c` copies what is shown, and `y` and `Enter` copy the source.

**Not built yet.**

- M3: the ASCII charset, BT/RL mirroring, the document cold-start budget with idle drawing, and segment splitting. M3's other item, setting the work limit, is done (6.6).
- M3b (subgraph frames), M4 (sequence drawing), M5 (viewer), M6 and M7.
- From the M0 to M2 lists: the engine seam and stub engine (4.4; `diagram::render` calls the flowchart code directly, and `Request` has no budget yet), the mermaid-ascii fixture parse test (10.2), and the style snapshots (10.6; a test checks each role's theme style instead).

**Owner decisions taken.** The numbers are those in section 12. Where the plan recommended a choice, that choice was taken.

1. The open frame.
2. Too wide: outline or transcript, never a clipped drawing. The viewer comes in M5, so until then `Enter` on a diagram copies its source, as it does on a code block.
3. Fences: `mermaid` and `{mermaid}`, in any case. Not `mmd`.
4. `PLAN.md:64` is reworded: no background jobs, a work limit per diagram, and the document budget later. PLAN.md's Phase 3 line now says `mmdc` is used only when installed and enabled.
5. M7 comes later, after M5.
6. The decision shape `╱───╲ < > ╲───╱`, as in the mock-ups.
8. Height guard: not built. It belongs with the viewer (M5).
9. `TERM=linux`: no change.
10. Ladder C. A left-to-right diagram tries normal, compact, LR labels 16 and LR labels 8, then turned, turned compact, labels 16 and labels 8.
11. Outlines are accepted when the contents pane is open.
13. No ambiguous-width option.
15. The gate thresholds as written.

Decisions 7, 12 and 14 are not needed yet.

**Changes from the plan.** Each item names the section that now describes it.

- `regex` is now a direct dependency. It was already in the dependency tree through syntect and fancy-regex 0.11, so no new crate is compiled (5.1).
- `--diagrams` takes `draw` and `source`. `ascii` comes with M3 (8.1).
- `s` and `c` were built in M0 instead of M5, because `Pager::relayout()` made them cheap. The `s` toggles are kept per document (8.2).
- `BlockKind::Diagram` has a `lang` field (the fence's first word), not `info` (8.1).
- Edges between the same two nodes: the prototype drew only the last one, so `A-->B; B-->A` showed a one-way arrow. folio draws them all (6.2 H).
- Size limits: a source over 64 KiB still gets its outline, outlines stop at 2,000 rows, and the parser and layout have memory guards (6.1, 6.5, 6.6).

From the second review round:

- Flowchart syntax: a subgraph named in another subgraph's body is nested there; quoted edge text may hold link characters; shape data may be single-quoted or run over several lines; `v`, `^`, `>` and `<` after `flowchart` are directions; front-matter titles may go on over several lines (3.2, 3.3).
- Sequence syntax: keywords in any case, `autonumber` that takes effect where it stands, `autonumber off`, `par_over`, `link` names with spaces, `wrap:` prefixes dropped, and `box` groups kept with their members (3.4, 6.5).
- Lanes into one node must ask for the same stroke as well as the same head (6.1 F).
- An edge label no longer ends a bus, so a labelled branch of a fan-out gets its own port (6.2 H).
- A straight line with heads at both ends always shows a line cell between them (6.2 K).
- Lane labels never cover another line. A lane without a head ends on its target's border. Loops and ports keep off a cylinder's rule and a subroutine's bars. Lines touch a bare text node's text (6.2 L, M).
- The text-over-line check is stricter, the connectivity check uses the drawn ends of each edge and reads through edge labels, and a zero-width character takes no cell (6.3).
- The work counter now tracks time, and the work limit is 500,000 (6.6).
- Outlines list subgraphs in tree order, and wrapped rows hang 4 columns (6.5).
- Focus is kept by content, not by source line (8.3).
- A fence ended by its quote or list item counts as closed, and a fence name may have one pair of braces, no more (8.1).
- Diagram titles are found by search (8.4). A sequence diagram's outline is called a transcript in the footer and in the copy message (8.2).

**Gate results** (section 11). Held-out sample, 6,496 blocks; Rust release build; measured after both review rounds [M12].

| Gate | Needs | Prototype | folio |
|---|---|---|---|
| 1. Flowcharts parse | ≥ 95% | 94.9% | 95.4% (5,371 of 5,632) |
| 2a. Drawn at 78, no subgraphs | ≥ 88% | 92.9% | 94.9% (4,402 of 4,638) |
| 2b. Drawn at 78, all parsed (subgraphs → outline) | ≥ 75% | 80.4% | 82.0% |
| 3. Fitting drawings that fail a check | < 0.5% | 0.2% | 0.1% (11 of 18,504) |
| 3. Crashes | 0 | 9 | 0 |
| 3. Same output twice | yes | — | yes |
| 4. p99, all ladder steps | ≤ 20 ms | 132 ms (Python) | 6.0 ms (p50 1.4 ms, max 23.5 ms) |
| Over the work limit or the size caps (M3 gate, 6.6) | ≤ 0.2% | — | 0.1% (5 diagrams, shown as outlines) |
| 5. 50-diagram readability review | 40 of 50 | — | not done |

- Gate 3 counts differently in the two columns. The prototype's figure counts only the first step that fits at 78 columns. folio's counts every ladder step that fits.
- Gate 5 needs a person and the other tools, so it is still to do.
- Sequence diagrams parse at 98.6% on the held-out sample (417 of 423; prototype 97.4%).
- The steps used at 78 columns are listed in section 7.4.
- On the tuning sample, 98.7% of flowcharts parse, and 89.5% of those without subgraphs are drawn at 78 columns. 21 of 35,668 fitting drawings fail a check. 98.0% of sequence diagrams parse. 16 diagrams are over the work limit or the size caps.

**Agreement with the prototype** [M13]. When the port was compared with the prototype, every parse result matched on all 23,946 blocks, except for deliberate changes and differences in the width tables. Drawings matched too, except where Python and the `unicode-width` crate measure a label differently, and except for the deliberate change to edges between the same two nodes.

The review fixes changed more on purpose. Drawings now differ from the prototype's for diagrams with lane labels, lanes of different strokes into one node, labelled branches of a fan-out, LR self-loop labels, lanes without a head, two-headed straight lines, bare text nodes, loops and ports on cylinders or subroutines, zero-width characters, reversed edges in a shared port, and edges between the same two nodes. The parser changes in sections 3.2 to 3.4 also differ from the prototype on purpose. How many diagrams these changes affect was not measured again.

**Known gaps** (left for later):

- The connectivity check can count a false pair when lines end on adjacent border cells of one box side. This was seen only in random graphs with three or more kinds of line between two nodes. A wrong count only moves the diagram on to the next step.
- Circle and double circle (drawn like a stadium) and hexagon (drawn like a decision) do not get the "simplified" note. Section 3.3 lists them for M3b.
- Two identical back edges, where one becomes a lane, are drawn twice. The drawing is correct, but redundant.
- Until M5, an outline shown because no drawing fits has no note in its bottom rule. "Enter: full drawing" comes with the viewer.
- Statement splitting finds `accTitle:` and `accDescr:` lines only in that exact case. The sequence parser now reads keywords in any case, so `ACCTITLE: a; b` is still split at the `;`.

---

## 1. Summary

**What we will build.** folio will draw Mermaid diagrams itself, as Unicode text, inside its existing pure layout function.

- Flowcharts come first, then sequence diagrams, then state diagrams.
- Every other diagram type shows its source, with a one-line reason.
- Every drawing is checked before it is shown. Its lines must join exactly the pairs of nodes that the source links (section 6.3).
- The renderer adds no crates, runs no external programs and makes no terminal queries. It behaves the same over SSH, in tmux, in print mode and without color.

**Three outcomes, never an error box.** Every `mermaid` block ends up as one of these:

1. **Drawn.** A drawing that fits the width and passes the checks.
2. **Outline.** A text outline (flowchart) or a numbered transcript (sequence diagram). It is shown when no drawing fits and passes, and it always fits.
3. **Source.** The block's source, with a short note. This is used when the diagram type or one of its statements is not supported, or when the diagram is empty.

Nothing is ever clipped. `Enter` on a focused diagram opens the full drawing in a viewer that can be panned.

**Why folio writes its own renderer.**

- **folio needs cells it can style and check.** Every cell must be tagged with a role (border, label, edge, arrowhead or frame) and with the bus that drew it. Theme styles, search positions, focus, the width rule and the connectivity checks all depend on those tags. Every existing crate returns finished strings instead.
- **merman-ascii 0.9.2 refuses too many real diagrams.** It is the best existing text renderer, but it refused 2,286 of 12,705 real flowcharts (18.0%) [M4]. Among them was the common "retry" loop used in mock-up 1 below [M6].
- **It is also large.** It adds 4.4 to 6.2 MB to a stripped binary [M5].
- **The other crates cannot fit sequence diagrams to a width.**
  - mmdflux 2.6.1 has no width fitting in a released version [V18].
  - mermaid-text uses a fixed layout for sequence diagrams [V19].

**Evidence that the design works.** A throwaway Python prototype of the algorithms in section 6 was run over two samples [M7]:

- the 17,450 Mermaid blocks it was tuned on;
- a held-out sample of 6,496 further blocks that played no part in building it.

The prototype now includes the two routing fixes in section 6.2 and the checks in section 6.3. On the held-out sample:

- **Flowcharts.**
  - 94.9% parse.
  - At 78 columns, 92.9% of parsed flowcharts without subgraphs are drawn and pass every check.
  - Flowcharts with subgraphs get an outline until frames are built. They are 13.4% of the held-out sample and 20.4% of the tuning sample. Under that rule, 80.4% of all parsed flowcharts are drawn in M2.
- **Sequence diagrams.**
  - 97.4% parse.
  - At 78 columns, 78.6% are drawn and pass every check.

**What the earlier numbers missed.** Revision 1 counted a diagram as drawn whenever it fit the width. I applied the new checks to the prototype as it was reviewed, at 78 columns, and found two problems [M7]:

- 10.9% of the flowchart drawings that fit had a defect. Almost all were false connections: lines that let a reader trace a link that is not in the source.
- About half of the sequence drawings that fit drew a message across another participant's lifeline as a junction.

The fixes in section 6 remove nearly all of these. Defects remain in 0.2% of fitting flowchart drawings, and those drawings move to the next ladder step.

**Effort.** 48 to 60 working days for flowcharts (including subgraph frames), sequence diagrams and the viewer. Plan for 60 [U]. Readers get labeled source boxes after milestone M0, and readable outlines after M1, before the layout engine lands. Section 11 lists what still ships if later milestones slip.

### 1.1 What readers will see at 80 columns

Both mock-ups are real output of the prototype with the fixes in section 6, at layout width 80 [M9]. They are not hand-drawn. In the pager on an 80-column terminal, the content width is 78 (`pager.rs:345`). Mock-up 1 is the same at 78, and the same with or without the fixes.

**Mock-up 1: flowchart.**

Source:
```
flowchart TD
    A[Receive order] --> B{In stock?}
    B -->|yes| C[Ship it]
    B -->|no| D[Back-order]
    C --> F([Done])
    D -.-> E[(Orders DB)]
    D -->|retry| B
```
Output:
```
── mermaid · flowchart ─────────────────────────────────────────────────────────
                         ┌───────────────┐
                         │ Receive order │
                         └───────┬───────┘
                                 ▾
                           ╱───────────╲
                           < In stock? >◂─────────────────┐
                           ╲─────┬─────╱                  │
                                 │                        │
                         ┌───────┴───────┐                │
                        yes              no               │
                         ▾               ▾                │
                    ┌─────────┐   ┌─────────────┐         │
                    │ Ship it │   │ Back-order  ├─ retry ─┘
                    └────┬────┘   └──────┬──────┘
                         │               ┆
                         ▾               ▾
                     ╭───────╮     ╭───────────╮
                     ( Done  )     ├───────────┤
                     ╰───────╯     │ Orders DB │
                                   ╰───────────╯
────────────────────────────────────────────────────────────────────────────────
```

What this shows:

- The decision box shape.
- A labeled fan-out sharing one tee (`┬`) and one track. Both branches come from the same node, so the shared line is correct.
- A dotted edge (`┆`).
- A stadium and a cylinder shape.
- The retry loop drawn in its own side lane, with its label on the exit stub.

The drawing is 39 columns wide. merman-ascii 0.9.2 refuses this exact source with "ASCII rendering does not support `routes crossing reserved graph geometry` for `flowchart` yet" [M6].

**Mock-up 2: sequence diagram.**

Source:
```
sequenceDiagram
    participant U as User
    participant W as Web app
    participant DB as Database
    U->>W: Log in
    W->>DB: Find user
    DB-->>W: User row
    W->>W: Check hash
    alt valid
        W-->>U: Session cookie
    else invalid
        W-->>U: 401
    end
```
Output:
```
── mermaid · sequence ──────────────────────────────────────────────────────────
                 ┌───────┐         ┌─────────┐    ┌───────────┐
                 │ User  │         │ Web app │    │ Database  │
                 └───┬───┘         └────┬────┘    └─────┬─────┘
                     │                  │               │
                     │ Log in           │               │
                     ├─────────────────▸│               │
                     │                  │               │
                     │                  │ Find user     │
                     │                  ├──────────────▸│
                     │                  │               │
                     │                  │ User row      │
                     │                  │◂┄┄┄┄┄┄┄┄┄┄┄┄┄┄┤
                     │                  │               │
                     │                  ├─┐ Check hash  │
                     │                  │◂┘             │
                     │                  │               │
                   ┌─── alt [valid] ──────┐             │
                   │ │ Session cookie   │ │             │
                   │ │◂┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┤ │             │
                   │ │                  │ │             │
                   ├┄┄┄ else [invalid] ┄┄┄┤             │
                   │ │ 401              │ │             │
                   │ │◂┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┤ │             │
                   │ │                  │ │             │
                   └──────────────────────┘             │
                     │                  │               │
────────────────────────────────────────────────────────────────────────────────
```

What this shows:

- Lifeline spacing is computed from the labels, so no label crosses a lifeline.
- Replies are dotted.
- The self-message takes 2 rows.
- The `alt` frame has its title in the top border and a dashed `else` divider.
- Frame borders, like messages, are drawn in front of the lifelines they cross. No `┼` appears, so nothing suggests that a lifeline takes part. Revision 1 drew these crossings as `┼`.

The drawing is 46 columns wide. Section 7 shows the same diagrams at 40 columns.

---

## 2. Goals and non-goals

### Goals

1. **Every `mermaid` block shows something useful wherever folio runs:**
   - the pager and print mode;
   - 40 to 200+ columns, including with the contents pane open;
   - light and dark themes, 256 colors, truecolor and no color;
   - SSH and tmux.
2. **Correct drawings.** The lines join exactly the pairs of nodes that the source links.
   - A reader who traces any line from a node reaches only that node's real neighbours.
   - `┼`, `├`, `┤`, `┬` and `┴` appear only where the lines of one bus meet.
   - A crossing of two unrelated lines never looks like a junction.
   - In sequence diagrams, a message never looks attached to a lifeline it only passes over.
3. **Readable drawings:**
   - straight edges where possible;
   - no line through a label;
   - arrowheads that touch exactly one box;
   - edge labels that clearly belong to one edge;
   - loops drawn as loops, not refused.
4. **Fit folio's model.**
   - Layout stays a pure function (`layout.rs:1-6`) of document, width, theme and diagram settings. The settings are `--diagrams` and the per-diagram `s` toggles. The cache never changes a result.
   - Snapshot tests and the "no line wider than the width" check at every width from 20 to 140 keep working (`tests/render.rs:34-49`).
   - Search, focus, copy, raw view and print mode work for diagrams as they do for code blocks.
5. **Never lose information.** A diagram folio cannot draw correctly becomes a complete outline, a transcript or the source. It is never clipped and never half drawn. A flowchart with subgraphs gets an outline that lists every group and its members until frames are drawn (M3b).
6. **No new dependencies.** No new crates, no C code, no external programs.

### Non-goals for this plan

- **Matching Mermaid's own layout.** Mermaid 12 lays flowcharts out with ELK and uses a new default look [V1]. Pixel parity is not a sensible goal for a character grid.
- **Applying `style`, `classDef`, `class`, `linkStyle` colors or `click` actions.** These are parsed and ignored (section 3.6).
- **Images, `mmdc` and remote renderers** (Kroki, mermaid.ink). Section 9 sets the rules for adding images later.
- **Rarer diagram types:** gantt, pie, mindmap, ER, C4 and the 2025–26 beta types. They show their source.

---

## 3. Diagram types, order and syntax

### 3.1 Which types, in what order

Shares come from the 17,450 unique Mermaid blocks of the tuning sample. They were extracted from public Markdown on GitHub, collected through Sourcegraph search [M3].

| Type (keyword) | Share | Plan |
|---|---|---|
| flowchart (`flowchart`, `graph`) | 72.8% | M1–M3b |
| sequence (`sequenceDiagram`) | 12.7% | M4 |
| class (`classDiagram`) | 4.7% | later (decision 7) |
| state (`stateDiagram`, `stateDiagram-v2`) | 3.8% | M6 |
| ER, pie, mindmap, gantt | 0.8–0.9% each | source |
| everything else | 0.7% or less each | source |

Coverage:

- Flowcharts and sequence diagrams together are 85.5% of diagrams.
- Adding class and state brings this to 94.0%.

Within flowcharts [M3, M7]:

- **Direction:** `LR` 38.7%, `TD` 43.0%, `TB` 18.1%, `BT` and `RL` 0.1% each. Left-to-right diagrams are as common as top-down ones, so width is the main problem.
- **Subgraphs:** 20.4% of parsed flowcharts in the tuning sample and 13.4% in the held-out sample.
- **Presentational lines:** 13.7% use `style`, `classDef`, `class`, `linkStyle` or `click`.

**The held-out sample is a different mix.** It comes from different searches: README files, architecture decision records, a set of selected repositories and one test query. In it, 86.7% of blocks are flowcharts and 54.8% of those are `LR` [M7]. Differences between the two samples therefore reflect content as well as overfitting.

### 3.2 Rules shared by all types

**Fence options (MyST and Sphinx).**

- MyST directives may start with an option block: "a set of lines prefixed with `:`" [V30].
- sphinxcontrib-mermaid documents its Markdown options as a `---` front-matter block inside the fence. Its keys are `name`, `alt`, `align`, `caption`, `zoom`, `fullscreen`, `config` and `title` [V23].
- Its setting `myst_fence_as_directive = ["mermaid"]` makes plain ` ```mermaid ` fences directives too [V23].

So folio does two things:

- It strips leading `:key: value` lines from every Mermaid fence before the preamble step. A diagram's first statement is its type keyword, so such a line is never Mermaid [U: inferred from V4].
- It ignores unknown front-matter keys.

**Preamble** [V1, V4, V5]:

- Skip blank lines.
- A `---` … `---` front-matter block must start on the first line. Keep its `title:` and show it in the top rule: `── mermaid · flowchart · Release flow ──`. If there is no `title:` but there is a `caption:`, show the caption [U: design choice].
  - The block is read as YAML, as far as the title needs: quoted values, comments, block scalars (`title: >-`) and an indented block all work. Only keys at the indent of the opening `---` count, so keys under `config:` are skipped.
  - A plain or quoted `title:` or `caption:` value may go on over more deeply indented lines, or start on the line after the key. Indented lines that look like a mapping, a list or a comment are not part of a plain value, and a key whose value is a mapping or a list gives no title.
  - A front-matter block with no closing `---` sends the diagram to source.
  - A sequence diagram's own `title` statement replaces the front-matter title, as in Mermaid (section 3.4).
- Skip `%%{ … }%%` directives, which may span lines and are deprecated since v10.5.0 [V5].
- Skip whole-line `%%` comments.
- As a lenient extra, strip a trailing `%% …` outside quotes.

**Keyword.**

- `flowchart` or `graph`, plus an optional `TB|TD|BT|RL|LR`. `graph` is documented as an alternative to `flowchart` [V1].
  - Mermaid's arrow forms work here too: `v` is top-down, `^` bottom-up, `>` left to right and `<` right to left. The `direction` statement does not accept them.
- `flowchart-elk`, treated as `flowchart`. The documentation says "The `flowchart-elk` diagram type still works, but is no longer needed" [V1]. Neither sample uses it [M7].
- `sequenceDiagram`.
- `stateDiagram` or `stateDiagram-v2`.
- Anything else becomes `Source("<keyword> diagrams aren't drawn yet")`.

**Statements.** Split on newlines, then on `;`. Where a `;` splits depends on the diagram type:

- **Sequence diagrams** split at every `;`, as Mermaid does, except the `;` that ends an entity code.
- **Flowcharts** don't split inside quotes, brackets, `>…]` shapes, `|…|` edge text, or edge text written inside a link (`-- … -->`, `== … ==>`, `-. … .->`).
- The text of an `accTitle:` or `accDescr:` line runs to the end of the line.
- Entity codes are protected first, because `;` ends a statement and literal semicolons are written `#59;` [V2]. Protected are `#word;` (numeric or named, such as `#59;` and `#quot;`) and `&name;` for the names the decoder knows. The prototype needed this, because unquoted sequence messages such as `I #9829; you` contain `;` [M1].
- In flowcharts, a Markdown string (``"`…`"``) may run over several lines, as in Mermaid, but only when a later line closes it. A stray quote therefore can't swallow the rest of the diagram.
- In flowcharts, `@{ … }` shape data may also run over several lines, but only when a later line holds a `}`. Such data is read as YAML pairs (section 3.3), so a `}` far below can't silently swallow statements.

**Label decoding** (one function for every label), in this order:

- `<br>`, `<br/>` and `<br />` become line breaks.
- Other HTML tags are stripped.
- Icon prefixes are dropped: `fa:`, `fab:`, `fas:`, `far:`, `fal:` and `fak:` (as in `fa:fa-car`).
- Markdown strings ``"`…`"`` keep their newlines and drop their emphasis markers (`**`, `*`, `_`) [V1].
- Entity codes (`#35;`, `#quot;`, `&lt;`) are decoded last, all in one pass, so what they produce is never decoded again: `#38;lt;` shows `&lt;` [V1]. Entity names are case-sensitive, as in HTML (`rArr` is ⇒ and `rarr` is →), and cover HTML 4's names.
- Tabs become spaces. Other control characters become U+FFFD. `doc::clean` (`doc.rs:334-343`) already does this for the whole block, and labels are cleaned again.

**Wrapping labels.** A label may break between two wide characters (section 6.2 G). A grapheme counts as wide only when its first character is 2 columns wide, so an Indic cluster that is 2 columns wide only because of its marks is never split.

### 3.3 Flowchart subset

```
stmt      := chain | subgraph-open | 'end' | 'direction' DIR | ignored
chain     := group (link group)*                     group := node ('&' node)*
node      := ID shape? (':::' CLASS)?  |  ID '@{' key ':' value (',' key ':' value)* '}'
value     := '"' text '"' | "'" text "'" | text up to ',' or a line break   -- '' is a quote in '…'
shape     := '[' t ']' | '(' t ')' | '([' t '])' | '[[' t ']]' | '[(' t ')]' | '((' t '))'
           | '(((' t ')))' | '>' t ']' | '{' t '}' | '{{' t '}}'
           | '[/' t '/]' | '[\' t '\]' | '[/' t '\]' | '[\' t '/]'
t         := '"' text '"' | '"`' markdown '`"' | text up to the closer
link      := (ID '@')? HEAD? STROKE HEAD? ('|' t '|')?           -- A -->|x| B
           | HEAD? ('--' | '==' | '-.') t CLOSER                  -- A -- x --> B, A-.x.->B
STROKE    := '-'{2,} | '='{2,} | '-' '.'+ '-' | '~'{3,}      HEAD := '<' | '>' | 'o' | 'x'
subgraph-open := 'subgraph' (ID (WS? '[' t ']')? | '"' t '"' | words)
ignored   := ('classDef'|'class'|'style'|'linkStyle'|'click'|'accTitle'|'accDescr') rest
           | 'title' rest                                         -- lenient; see below
           | ID '@{' key ':' value … '}'    -- whole statement; edge settings or an edge id
```

**Rules from Mermaid's documentation** [V1]:

- **Lowercase `end`.** Lowercase `end` as node text breaks a flowchart, so folio treats it as unsupported.
- **The `o` and `x` trap.** A link takes a following `o` or `x` as its head, so `A---oB` is a circle edge.
- **Last label wins.** "If this is done several times, it is the last text found for the node that will be used" [V1]. The prototype already does this, and a unit test (`last_label_wins`) pins it.
- **Link length.**

  | | Length 1 | Length 2 | Length 3 |
  |---|---|---|---|
  | Normal | `---` | `----` | `-----` |
  | Normal with arrow | `-->` | `--->` | `---->` |
  | Thick | `===` | `====` | `=====` |
  | Thick with arrow | `==>` | `===>` | `====>` |
  | Dotted | `-.-` | `-..-` | `-...-` |
  | Dotted with arrow | `-.->` | `-..->` | `-...->` |

  folio uses the length as the minimum number of ranks the edge spans.
- **Groups.** `A & B --> C & D` is four edges.
- **Invisible links.** `~~~`, or a longer run of `~`, only affects position.
- **Subgraph direction.** "If any of a subgraph's nodes are linked to the outside, subgraph direction will be ignored" [V1].
- **Shapes by name.** `@{ shape: … }` dates from v11.3.0. Its documented names map to the nearest classic shape:
  - `rect`, `rounded`, `stadium`, `subproc`, `cyl`, `circle`, `dbl-circ`, `diamond`, `hex`, `lean-r`, `lean-l`, `trap-b` and `trap-t` each get their classic shape;
  - `text` has no border;
  - any other name becomes a rectangle and marks the drawing "simplified".
- **Collapsed subgraphs.** `@{ view: collapsed }` on a subgraph (v11.17.0+) falls back to source in v1.

**Edge text without spaces.** `A -.tests.-> B` and `A --text--> B` are valid Mermaid and common. The first prototype parser required a space after `--`, which sent 57+ real flowcharts to source. The final grammar accepts both forms [M2].

**Rules added while porting (M1):**

- **Edge text after a head.** `o-- text --o`, `x-- text --x` and `<-- text -->` are one edge with text.
- **Edge text after `-.`** may start with `o` or `x` (`-.optional.->`). A dotted link needs a `-` after its dots, so the letter can't be a head.
- **`direction` and `title`.** `direction` is a keyword only before `TB`, `TD`, `BT`, `RL` or `LR`; otherwise it is a node id. As a lenient extra, a `title …` line is ignored, but `title --> x` and `title[…]` are nodes.
- **`id@{ … }` statements.** A statement is skipped only when the whole statement is one `id@{…}` whose keys are edge settings (`animate`, `animation`, `curve`), or whose id is an edge id. Any other `id@{…}` is a node with a shape. `view: collapsed` sends the diagram to source.

**Rules added in review (2026-10-28):**

- **Quoted edge text.** Edge text written inside a link (`A -- "git push --force" --> B`) and `|…|` text are read with quotes in mind, so quoted text may hold `--`, `==`, `.-` and `|`. A quote that nothing closes is read as text, and then the first `|` ends `|…|` text, as before.
- **Shape data values.** A value may be in double quotes, in single quotes (`''` is one quote, as in YAML), or plain, up to a comma or a line break. One pair of quotes is removed, and line breaks inside quotes become spaces. A `}` inside double quotes does not end the data.
- **Shape data over several lines** is YAML: pairs, one per line or separated by commas, and `#` comments. Any other text there sends the diagram to source with 'unexpected text "…" in @{ }'. Without a later `}`, the reason is still 'unterminated @{'.
- **Subgraph titles.** In `subgraph id [title]`, the title may be a Markdown string over several lines. The id is kept, so edges to `id` attach to the subgraph.

**Subgraphs before frames exist (M0 to M3).** A flowchart that contains any `subgraph` is parsed in full but is not drawn. It gets the outline (section 6.5), which lists each subgraph's title and members, with the bottom rule "subgraphs aren't drawn yet". The prototype could lay these diagrams out without frames, but that loses the grouping, which goal 5 forbids.

- **Membership follows Mermaid.** A subgraph takes every node its body names, unless a subgraph that closed earlier already took it. Inner subgraphs close first, so they win. Naming a node at the top level, before or after a subgraph, changes nothing.
- **Subgraphs are members too.** Naming a subgraph's id in another subgraph's body nests it there, as Mermaid does, and the first subgraph to close that names it wins. A reopened subgraph moves to its new parent only until that happens. (Before the second review, the last parent always won.) Nesting never makes a cycle. The checks that prevent cycles have a step limit (6.6).
- **Order.** The parser stores subgraphs in tree order: each subgraph is followed by those nested in it, and siblings keep their declaration order. Every index into the subgraph list (a node's subgraph, an edge end's subgraph and a subgraph's parent) is renumbered to match. Frames (M3b) can rely on this order.

**Parsed and silently ignored:**

- `style`, `classDef`, `class`, `:::`, `linkStyle` and `click`;
- edge ids and `animate`;
- `accTitle` and `accDescr`, including a multi-line `accDescr { … }`;
- front-matter `config` and any unknown front-matter key.

**Drawn with "simplified" in the bottom rule** (once frames land in M3b):

- **Subgraph `direction`.** Ignored; members are laid out in the parent's direction. The prototype found this in 235 of 12,492 parsed tuning flowcharts [M2].
- **An edge to or from a subgraph.** It attaches to the first member that has no edge from inside the subgraph, searching nested subgraphs too (304 tuning flowcharts) [M2]. The edge remembers the subgraph (`Edge::from_cluster`, `to_cluster`), so the outline shows the subgraph's name for that end. This is built; it only affects outlines until frames are drawn.
- **Shape approximations.** Circle and double circle are drawn like a stadium; hexagon is drawn like the decision shape. These shapes are drawn in M2, but without the "simplified" note (a known gap).

### 3.4 Sequence subset

```
stmt := ('participant'|'actor') NAME ('@{' … '}')? ('as' TEXT)?
      | NAME ARROW ('+'|'-')? NAME ':' TEXT?
      | 'Note' ('left of'|'right of'|'over') NAME (',' NAME)? ':' TEXT
      | ('loop'|'alt'|'opt'|'par'|'par_over'|'critical'|'break'|'rect') TEXT?  …  'end'
      | ('else'|'and'|'option') TEXT?
      | ('activate'|'deactivate'|'destroy') NAME | 'autonumber' (NUM NUM? | 'off')?
      | 'create' ('participant'|'actor') NAME ('as' TEXT)?
      | 'box' TEXT? … 'end' | ('link'|'links') NAME ':' REST
      | 'title' ':'? TEXT
ARROW := '->>' | '-->>' | '->' | '-->' | '-x' | '--x' | '-)' | '--)' | '<<->>' | '<<-->>'
```

- **Arrows.** The arrow table is documented [V2]:
  - `->` and `-->` have no arrowhead;
  - `-x` and `--x` end in a cross;
  - `-)` and `--)` are async;
  - `<<->>` and `<<-->>` (v11.0.0+) have heads at both ends.
- **`end` in text.** The word `end` in sequence text must be wrapped in `()`, `[]` or `{}` [V2].
- **Unsupported arrows.** Half-arrows and central connections (v11.12.3+) [V2] fall back to source. A `()` next to an arrow is read as a central connection.
- **Names with spaces.** Participant names may contain spaces (`participant Edge Function`). Rejecting them sent 28 real diagrams to source in the first prototype run, so the final grammar accepts them [M2]. `activate`, `deactivate` and `destroy` take such names too.
- **Names in messages** follow Mermaid's rule for an actor name: no `+`, `<`, `>`, `,`, `;` or `:`, and no `-` at the start or end.
- **`title`.** A `title` statement is kept and shown in the top rule. It replaces a front-matter title. Of several, the last one that is not empty wins.
- **Keywords in any case.** Every keyword is read in any case, as Mermaid's lexer reads them (`Note` already was). So a statement that starts with a sender named `Loop` or `Opt` opens a block. Mermaid reads it the same way, so such diagrams fail in Mermaid too. Names that only start with a keyword, such as `Endpoint` or `Boxer`, stay names.
- **`link` and `links`** take participant names with spaces. As in Mermaid, a link to an unknown participant adds nothing, and a name that isn't valid (`link A->>B: x`) sends the diagram to source.
- **`wrap:` and `nowrap:`.** A leading `wrap:`, `nowrap:`, `:wrap:` or `:nowrap:` (lower case only, as in Mermaid) is dropped from message, note, block, divider, alias and box text. It is kept in a `title`.
- **`box`.** The title drops a leading word, or a leading `rgb()`, `rgba()`, `hsl()` or `hsla()` call, only if CSS reads it as a colour. Otherwise the whole text is the title. A box's members are the participants declared inside it. A participant declared in two boxes sends the diagram to source, as Mermaid fails on it.
- **`autonumber`** takes effect where it stands, and `autonumber off` hides the numbers from there. Section 6.5 gives the numbering rule.
- **Approximated, with a "simplified" note:**
  - `box` groups are drawn without their background (the transcript lists each box and its members);
  - `create` places the participant in the header;
  - activations deeper than one level are drawn as one.

### 3.5 State subset (M6)

These sections of the state-diagram syntax are documented [V3]:

- `[*]` start and end markers;
- transitions with labels;
- composite states `state X { … }`;
- `<<choice>>`, `<<fork>>` and `<<join>>`;
- notes;
- concurrency (`--`);
- `direction`.

A state diagram becomes a `FlowGraph`:

| State syntax | Drawn as |
|---|---|
| state | rounded box |
| `[*]` | a small `●` box [U: glyph choice] |
| composite state | subgraph frame (needs M3b) |
| choice | decision shape |
| fork and join | short bar |

### 3.6 What happens to unsupported syntax

**Strictness.**

- Any statement outside the subset sends the whole block to source with a dim note, for example "Not drawn: unexpected text `-|\`".
- folio never guesses a statement into a node. mermaid-ascii does exactly that with unparsed lines.

**Fallback rates** [M7]:

| Type | Tuning: sent to source | Held-out: sent to source | Main reasons |
|---|---|---|---|
| Flowcharts | 207 of 12,706 (1.6%) | 279 of 5,632 (5.0%) | prose that is not Mermaid; brackets inside unquoted labels such as `E[List[Result]]`; unterminated shapes. In the held-out sample, 158 of the 279 are node ids containing `#` or `++` (`C#[C#]`, `C++[C++]`); whether Mermaid accepts such ids is unverified (Q3). |
| Sequence diagrams | 58 of 2,221 (2.6%) | 11 of 423 (2.6%) | statements outside the subset |

The prototype also crashed on 2 tuning flowcharts and 9 held-out flowcharts (`KeyError`) [M7]. These were bugs to fix in the port, and folio has no crashes on the held-out sample [M12] (section 6.1 E). The corpus gate requires zero crashes.

**Empty and degenerate diagrams.** Each case has a fixed outcome:

| Input | Outcome |
|---|---|
| empty fence, or only blank lines, comments, directives, MyST options or front matter | Source, "Empty diagram" |
| keyword only (`flowchart TD`, `sequenceDiagram`) | Source, "Empty diagram" |
| flowchart with only `style`, `classDef`, `class`, `linkStyle`, `click`, `accTitle` or `accDescr` lines | Source, "Nothing to draw" [U: whether Mermaid draws a node named only in a `style` line is unverified; folio does not] |
| only invisible links (`A ~~~ B`) | Drawn: the boxes, placed by the links, with no lines |
| a single node | Drawn: one box |
| participants but no messages | Drawn: header boxes and short lifelines |

The samples contain 26 parsed flowcharts with exactly one node and 150 with no visible edge [M7].

**Required fixtures:**

- `style`, `classDef`, `class`, `linkStyle` and `click` lines never become stray nodes.
- A `pie` block falls back to source.
- A syntax error falls back to source.
- Every row of the degenerate-case table above.

---

## 4. Architecture

### 4.1 Pipeline

```
fenced code block, first info word "mermaid" or "{mermaid}"
        │  doc.rs: BlockKind::Diagram { lang, source, closed }
        ▼
layout.rs: Layouter::diagram(d, width)
        │  diagram::render(source, Request { width, charset, budget }, &mut Diagrams)
        ▼
┌─ src/diagram/ ───────────────────────────────────────────────────┐
│ pre.rs       MyST options, front matter, %%{init}%%, comments,   │
│              keyword, statement split                            │
│ flow/parse   seq/parse    ──►  IR: FlowGraph, SeqDiagram         │
│ flow/prep    cycles, ranks, chains, order, lanes (once/diagram)  │
│ ladder.rs    normal → compact → … → labels 8 (order: decision 10)│
│ flow/draw    sizes, ports, placement, channel router, lanes      │
│ seq/draw     column solver, rows, frames, activations            │
│ canvas.rs    cells with role and owning bus, glyph tables        │
│ check.rs     text-over-line, cell check, connectivity trace      │
│ outline.rs   flowchart outline, sequence transcript (always fit) │
└──────────────────────────────────────────────────────────────────┘
        ▼
Outcome::Drawn(Drawing) | Outcome::Outline(Drawing) | Outcome::Source(reason)
        ▼
layout.rs: roles → Theme styles → RLines, Flows (search), Target (focus)
```

The `diagram` module depends on neither ratatui nor `Theme`. It deals only in roles, so it can be tested on its own.

### 4.2 Files

Line counts are estimates [U], guided by the prototype's sizes:

- parsers: about 300 lines of Python;
- flowchart layout, routing and painting: about 700, plus about 150 for the fixes;
- sequence diagrams: about 400;
- shared code: about 400;
- checks: about 150.

| File | Contents | Lines [U] |
|---|---|---|
| `src/diagram/mod.rs` | `render`, `Request`, `Outcome`, `Drawing`, `Role`, `Work`, `Diagrams` (settings, toggles, caches), the ladder, the `Engine` trait | 400 |
| `src/diagram/pre.rs` | MyST options, preamble, statement splitting, label decoding | 280 |
| `src/diagram/flow/parse.rs` | flowchart subset parser → `FlowGraph` | 550 |
| `src/diagram/flow/prep.rs` | cycles, ranks, chains, ordering, lane selection | 450 |
| `src/diagram/flow/draw.rs` | sizes, ports, placement, router, lanes, self-loops | 1,100 |
| `src/diagram/flow/cluster.rs` | subgraph frames (M3b) | 400 |
| `src/diagram/seq/*.rs` | parser, column solver, rows, frames | 800 |
| `src/diagram/canvas.rs` | cell grid, owners, masks, glyph tables (Unicode and ASCII), shapes, mirroring | 450 |
| `src/diagram/check.rs` | text-over-line, cell check, connectivity trace, sequence junction check | 250 |
| `src/diagram/outline.rs` | outline (with subgraph membership) and transcript | 200 |
| `src/viewer.rs` | full-screen pan viewer | 250 |
| tests | unit, corpus-parse, property, snapshot and style tests | 1,800 |

### 4.3 Public API

```rust
pub enum Charset { Unicode, Ascii }
pub struct Request { pub width: usize, pub charset: Charset, pub budget: Budget }
pub enum Budget { Document(Arc<Work>), Unbounded }  // see 6.6

pub enum Outcome {
    Drawn(Arc<Drawing>),    // fits request.width and passed the checks
    Outline(Arc<Drawing>),  // outline or transcript; always fits
    Source(String),         // why it was not drawn
}

pub struct Drawing {
    pub kind: Kind,                          // Flowchart, Sequence, State
    pub step: Step,                          // which ladder step produced it
    pub rows: Vec<Vec<(Role, String)>>,      // runs of same-role text
    pub width: usize,                        // measured with folio's text::width
    pub labels: Vec<LabelRef>,               // (group, row, col, text) for search
    pub notes: Vec<Note>,  // TurnedToFit, DrawnTopDown, Simplified(..), SubgraphsNotDrawn, Deferred, TooLarge
}

#[derive(Clone, Copy)]
pub enum Role { Border, NodeText, Edge, EdgeLabel, Arrow, Frame, FrameTitle, Note }
pub enum Step { Normal, Compact, LrLabels16, LrLabels8, Turned, TurnedCompact, Labels16, Labels8 }

pub fn render(source: &str, req: &Request, dg: &mut Diagrams) -> Outcome;
pub fn render_full(source: &str, charset: Charset, dg: &mut Diagrams) -> Outcome; // viewer, 4× work limit
```

### 4.4 Engine seam

```rust
pub type Prepared = Arc<dyn Any + Send + Sync>;   // engine-owned IR, downcast by the engine

pub trait Engine: Send + Sync {
    fn prepare(&self, src: &str, work: &mut Work) -> Result<Prepared, NotDrawn>;
    fn draw(&self, prep: &Prepared, step: Step, charset: Charset, work: &mut Work) -> Result<Drawing, NotDrawn>;
    fn outline(&self, prep: &Prepared, width: usize) -> Drawing;
}
```

- **Prepared IR is passed in, not the source.** `Diagrams` caches the `Prepared` value for each source and passes it to every `draw`. The engine never parses twice and needs no interior mutability.
- **Work.** `Work` is an operation counter (section 6.6). When it runs out, `draw` returns `NotDrawn::OverBudget`.
- `NativeEngine` is folio's renderer.
- `StubEngine` returns canned drawings. It tests folio's own logic on its own: ladder selection, the frame, flows, focus, toggles and the width rule.
- The seam leaves room for plan B (ascii-dag, section 5.2) or a later image engine. Neither would touch `layout.rs`.
- Every row a drawing returns is measured again with `text::width`, whatever engine produced it. The checks in section 6.3 run on every drawing, whatever engine produced it.

### 4.5 Intermediate representation

```rust
pub struct FlowGraph {
    pub dir: Dir,                    // TD (= TB), BT, LR, RL
    pub title: Option<String>,
    pub nodes: Vec<Node>,            // declaration order = NodeId
    pub edges: Vec<Edge>,            // declaration order
    pub clusters: Vec<Cluster>,      // subgraphs; tree via `parent`
    pub simplified: Vec<Note>,
}
pub struct Node { pub id: String, pub label: String, pub shape: Shape, pub cluster: Option<u32> }
pub enum Shape { Rect, Round, Stadium, Subroutine, Cylinder, Decision, Slant(char, char), Bare }
pub struct Edge { pub from: u32, pub to: u32, pub stroke: Stroke, pub heads: (Head, Head),
                  pub label: Option<String>, pub min_len: u8 }
pub enum Stroke { Solid, Dotted, Thick, Invisible }
pub enum Head { None, Arrow, Circle, Cross }

pub struct SeqDiagram { pub participants: Vec<Participant>, pub events: Vec<Event>,
                        pub autonumbers: Vec<(usize, Autonumber)>,  // (events before it, statement)
                        pub boxes: Vec<SeqBox> }
pub struct Autonumber { pub start: Option<u64>, pub step: Option<u64>, pub visible: bool }
pub struct SeqBox { pub title: String, pub members: Vec<usize> }
pub enum Block { Loop, Alt, Opt, Par, ParOver, Critical, Break, Rect }
pub enum Event {
    Message { from: u32, to: u32, stroke: Stroke, head: SeqHead, both: bool, label: String, act: Option<bool> },
    Note { at: NotePlace, who: Vec<u32>, label: String },
    Open { kind: Block, label: String }, Divider { kind: Block, label: String }, Close,
    Activate(u32), Deactivate(u32), Destroy(u32),
}
```

- **`autonumber`.** `SeqDiagram` also keeps the older `autonumber: Option<(u32, u32)>`, the last numbered statement's start and step. Only the comparison with the prototype reads it.
- **Lookups.** Ids are looked up through a `HashMap<String, u32>` that is only probed, never iterated.
- **Order.** Everything that affects output is a `Vec` in declaration order.
- **Why this matters.** Rust's default `HashMap` is randomly seeded and iterates in arbitrary order [V11], and snapshots need identical output on every run.

### 4.6 The three outcomes

| Outcome | When | What the reader sees |
|---|---|---|
| Drawn | type supported, every statement parsed, no subgraph (until M3b), and some ladder step fits **and passes the checks** | the drawing between the top and bottom rules; notes ("drawn top-down to fit", "simplified") in the bottom rule |
| Outline | no step fits and passes; the diagram has subgraphs (until M3b); it is over the size caps or its work limit; or the document's cold-start budget is used up (pager only, filled in later; section 6.6) | outline or transcript wrapped like prose; the bottom rule says why: "Enter: full drawing", "subgraphs aren't drawn yet", "too large to draw here" or "drawing…" |
| Source | unsupported type or statement, unbalanced `subgraph`/`end`, not a diagram, or an empty diagram | today's code box (`code()`, `layout.rs:445-508`), plus one dim note line "Not drawn: <reason>" (section 8.2) |

---

## 5. Library choices

### 5.1 What folio uses

All of these were already in folio's dependency tree, so no new crate is compiled. `regex` came in through syntect and fancy-regex 0.11; the parsers made it a direct dependency in `Cargo.toml`.

| Crate | Version | License | Used for |
|---|---|---|---|
| `regex` | 1.13.1 | MIT OR Apache-2.0 | the preamble, label decoding and the flowchart and sequence parsers |
| `unicode-width` | 0.2.2 (2025-10-06) | MIT OR Apache-2.0 [V16] | label and grapheme widths. `width()` counts East Asian Ambiguous characters as 1 column; `width_cjk()` counts them as 2 [V10]. |
| `unicode-segmentation` | 1.13.3 | MIT OR Apache-2.0 [V16] | grapheme clusters for measuring and wrapping |
| `insta` | 1.49.0 (2026-10-03) | Apache-2.0 [V16] | snapshot tests (dev-dependency) |
| `comrak` | 0.57.0 | BSD-2-Clause [V16] | `NodeCodeBlock` has `fenced`, `info`, `literal` and `closed`; indented blocks always report `closed = true` [V12] |

### 5.2 Considered and rejected

**merman / merman-ascii 0.9.2** (2026-10-21)

- License MIT OR Apache-2.0; MSRV 1.95 [V16].
- `merman-core`'s non-optional dependencies include `cssparser ^0.38`, `lol_html ^3.0.1`, `url`, `json5`, `euclid` and `htmlize` [V17].
- `lol_html` 3.0.2, published today, needs `cssparser ^0.36` and `selectors ^0.37` [V17]. cssparser and selectors are MPL-2.0; lol_html is BSD-3-Clause [V16].
- Measured on this machine:
  - It refused 18.0% of real flowcharts: 1,200 for "routes crossing reserved graph geometry", 372 for "mixed-stroke route junctions" and 315 for "unroutable graph edges" [M4].
  - It refuses mock-up 1 and the plain `A-->B; B-->C; B-->D; D-->B` loop [M6].
  - It adds 4.4 to 6.2 MB to the binary [M5].
  - It returns strings, not cells with roles.
- Use: a side-by-side visual reference during development only.

**mmdflux 2.6.1** (2026-08-09)

- License MIT [V16]. Dependencies: pest, serde, serde_json, thiserror, tracing and unicode-width [V17].
- Its width fitting (`render_diagram_fitted`) exists only on `main`; "fitted" does not appear in the 2.6.1 item list [V18].
- Its README says "Sequence diagrams are measured but not compacted" [V18].
- Use: a visual reference, through its CLI.

**mermaid-text 0.57.0** (2026-07-21)

- License MIT; MSRV 1.92. Dependencies: `ascii-dag ^0.9.1`, `chrono` and `unicode-width` [V16, V17].
- Its source says "Sequence diagrams have a fixed layout; no compaction pass" [V19]. Inside folio this would not break the width rule, because folio measures again and falls back to the transcript. It would only limit how many sequence diagrams are drawn.
- It returns a `String`.

**ascii-dag 0.11.0** (2026-09-06; the previous release, 0.10.3, was 2026-08-26)

- License MIT OR Apache-2.0; zero dependencies; MSRV 1.92 [V16, V17].
- It returns positioned nodes and routed edges, and accepts custom node sizes (`CustomNode { width, height, … }`) [V20].
- It measures text per Unicode scalar and drops combining marks from painted labels [V20].
- **Clusters.** It supports clusters. The README has a "Subgraphs (clusters)" section with `put_nodes(..).inside(sg)` and nesting through `put_subgraphs(..).inside(..)` [V20]. Its `docs/layout.md` says [V28]:
  - "Boxes affect layout, not just paint: members are kept together during crossing reduction, borders reserve padding, and unaffiliated nodes are pushed clear of a box's projected envelope."
  - A box grows to fit its label, "capped at 40 characters".
- **Frame positions.** `LayoutIR::subgraphs()` is documented as "Get all laid-out subgraph bounding boxes" [V29].
- **Not documented.** How edges that cross a cluster boundary are routed [V28]. Its quality on the corpus is unmeasured [U].
- **Role: plan B, in two places.**
  - For steps C to I (ranking, ordering, placement) if folio's own placement fails the M2 checkpoint.
  - For subgraph frames if folio's own frames fail the M3b gate. folio would read node and frame boxes from the IR, pass its own node sizes, draw titles itself, and keep its own router, painter and checks.

**ELK `OrthogonalRoutingGenerator`**

- License EPL-2.0, with GPL-3.0 as a secondary license [V13].
- It credits Sander's *Layout of directed hypergraphs with orthogonal hyperedges* and Di Battista et al. [V13].
- folio reimplements the idea from the papers and from this description. It copies no code.

**mermaid-ascii (Go) 1.6.1** (2026-09-08)

- License MIT; 1,612 stars; last push 2026-10-19 [V21].
- **Fixture counts, corrected.** Each fixture file holds the input, a `---` line, then the expected output.
  - There are 142 graph fixture files (`ascii` 92, `extended-chars` 47, `multibyte` 3) and 96 sequence fixture files (`sequence` 57, `sequence-ascii` 39).
  - All 47 `extended-chars` names also exist in `ascii`, and all 39 `sequence-ascii` names exist in `sequence`. Most pairs hold the same input rendered in two charsets; `ampersand_lhs.txt` is identical in both.
  - After removing duplicates by input text, there are **96 unique graph inputs and 60 unique sequence inputs, 156 in all** [V21, checked today].
- Role: copy those 156 inputs into folio's parser tests, keeping the MIT notice. They test parsing breadth only. Coverage of real-world syntax still comes from the corpus gate.

### 5.3 Only if a crate is ever added

Add `cargo-deny` 0.20.2 (license allowlist and advisories) and `cargo-about` 0.9.2 (third-party notices) to CI. Both are MIT OR Apache-2.0 [V16]. They are not needed while the plan adds no crates. If plan B is taken, ascii-dag would be the first added crate.

---

## 6. Layout and routing algorithms

Every step below runs in the prototype unless it says otherwise. Where a detail differs from the text-first proposal, the prototype showed the change was needed, and the reason is given. The flowchart steps (6.1 to 6.3, without frames) are ported to Rust in M2. Where folio differs from the prototype, the text says so.

### 6.1 Flowcharts: steps that run once per diagram

Layout uses two abstract axes: **rank**, the flow direction, and **order**, across it.

- TD maps rank to rows. LR maps rank to columns.
- **BT and RL.**
  - In M2, BT diagrams are drawn as TD and RL diagrams as LR, with "drawn top-down" or "drawn left to right" in the bottom rule. Edges and arrows stay correct; only the direction on screen changes. This is built.
  - An LR or RL diagram that a ladder step draws top-down gets "drawn top-down to fit" instead.
  - When a drawing has several notes, the bottom rule joins them with "; ", for example "drawn left to right; simplified".
  - In M3, the finished geometry is mirrored before painting, and the note goes away. Text is re-placed, never reversed.
  - BT and RL together are 0.2% of flowcharts (42 in both samples) [M7].

**A. Prepare.**

- Expand `&` groups and chains into separate edges.
- Set self-loops aside.
- Keep invisible edges for ranking only.

**B. Break cycles.**

- Run an iterative depth-first search from the nodes in declaration order, following out-edges in declaration order.
- An edge to a node still on the stack is a **back edge**. It is reversed for ranking and ordering.
- The result is deterministic and keeps the author's main flow. dagre's alternative, `acyclicer: greedy` [V14], can reverse a different edge from the one the author meant as "going back".

**C. Rank.**

- **Label ranks.** If any visible edge has a label, every minimum length is doubled (factor f = 2). Node ranks are then even, and odd ranks hold edge labels.
- **Longest path.** Run Kahn's algorithm with a min-heap on declaration index, and set `rank(v) = max(rank(u) + min_len)`.
- **Pull sources down.** Each source with out-edges moves to `min(rank(w) − min_len)`.
- **Normalize** so the lowest rank is 0.
- **Guard.** Minimum lengths are clamped to 1 to 2^40, so a hand-built or extreme graph can't overflow the arithmetic.
- **Later upgrade.** Network simplex is dagre's default ranker [V14]. Use it only if long edges become a problem.

**D. Chains.**

- Each edge becomes a chain: a node, then dummies, then a node.
- The first dummy after the upper end is a **label element** when the edge has a label. Its size is the size of the wrapped label.
- Because labels are layout elements, they cannot collide with nodes or with each other.
- **Guards** (folio only). The diagram is too large, and gets the outline, when the chains would hold more than 300 elements that can't become lanes, when more than 15,000 elements would be built, or when there are more than 60,000 ranks. These limits only stop memory use from growing without bound; the size caps in section 6.6 apply after them.

**E. Order.**

1. **Starting order.** Breadth-first from the sources in declaration order. Place each node, its chains' dummies, then its targets. When a node is reached only through invisible edges, its chains' dummies are placed right after it too. The prototype never placed them, which made it crash (section 3.6).
2. **Sweeps.** Up to 8 barycenter sweeps, alternating down and up:
   - each element's key is the mean position of its neighbours in the fixed rank;
   - an element with no neighbours keeps its current position;
   - each rank is sorted with a stable sort;
   - keep the ordering with the fewest crossings, and stop after 4 sweeps without improvement.
3. **Transpose.** Swap adjacent elements while that reduces crossings, for at most 20 passes.

The papers behind these phases are those dagre cites: Jünger and Mutzel; Barth, Mutzel and Jünger; Sander; Forster [V14].

**F. Lanes for back edges.**

- **Candidates.** Back edges, sorted by rank span and then by declaration order.
- **A candidate gets its own lane when all of these hold:**
  - its source v is the last element of its rank;
  - its target u is the last element of its rank;
  - the "last element" checks ignore dummies of other candidates;
  - neither v nor u has a self-loop;
  - v sends no other lane and receives none;
  - u sends no lane;
  - the edge has no head at its source (folio only). A lane draws a head only at its target, so a back edge with a head at its source stays in the channels.
- **Several lanes may arrive at the same node.** They merge at its single arrowhead. This is a correct fan-in, because every lane in it ends at that node. The prototype needed this for review loops such as `B -->|changes| A` together with `C -->|fail| A` [M1]. In folio, lanes into one node must also ask for the same stroke and the same head, because they share the last part of their line. A lane with another stroke or head is rejected and drawn as a reversed edge in the channels. Before the second review only the head was compared, so a dotted lane could arrive on a solid line.
- **Repeat until stable.** Rejecting a candidate puts its dummies back, which can invalidate others.
- **Accepted lanes.** Their dummies are removed.
- **Rejected back edges** stay as reversed edges routed through the channels, with `▴` at the real target. dagre handles back edges this way.

### 6.2 Flowcharts: steps that run once per ladder step

None of these steps depends on the terminal width (section 8.7).

**G. Sizes.**

- **Labels.** Wrap labels to the step's label width: 28, 16 or 8.
  - Words up to 16 columns are never broken. Without this rule, the prototype printed "Authoriz/e" and "OrderCre/ated" at width 8 [M1].
  - **New:** a line may break between two wide characters (CJK ideographs, kana, hangul), as CJK text has no spaces. Without this, a Japanese label of up to 8 characters could not wrap at labels 8.
  - In folio's `wrap_label`, a grapheme counts as wide only when its first character is 2 columns wide. A Devanagari or Myanmar cluster can be 2 columns wide because of its marks, but it is part of a word and is never split.
  - Measured on flowcharts whose source contains CJK letters (3,137 tuning, 539 held-out, without subgraphs), allowing these breaks raised the drawn share at 51 columns from 62.4% to 70.2% (tuning) and from 73.3% to 81.8% (held-out). At 78 columns it rose from 86.0% to 89.3% and from 91.1% to 94.2%. For CJK sequence diagrams at 78 columns, it rose from 54.2% to 61.1% (tuning, 961) [M7].
- **Boxes.**
  - Inner width = label width + 2. In TD it is rounded up to odd, so a single tee sits exactly in the middle.
  - Height = label lines + 2, +1 for a cylinder.
  - A TD self-loop needs 2 inner rows. On a cylinder they come below the rule, so the box gets one more row (folio).
  - In LR, box height is odd.
  - A bare text node (folio) is exactly its text: no border, no padding, and no rounding to an odd size.
- **Label elements.**
  - In TD, their width is the label width and their rank thickness is the number of label lines.
  - In LR, their thickness is the label width plus one space on each side.

**H. Ports.**

- On each side of a node, group the links by (stroke, head at that end). Each group shares one port, which makes a **bus**.
- **New rule: a port carries one bus only.**
  - A bus is either a fan-out (one port on the upper node, several lower ends) or a fan-in (several upper ends, one port on the lower node). It is never both.
  - On a node's upper side, a link that belongs to another node's fan-out bus gets a port of its own, even when its stroke and head match the other arriving links.
  - To find the bus, trace the link back through its edge's dummies and its label to the node it leaves. An edge label does not end the bus: it sits on the line, and a reader reads straight through it. So a labelled branch of a fan-out gets its own port at its target too. Before the second review, a label ended the bus, and `A-->C; A-->|x| D; B-->D` drew a line from B that could be traced to C.
- Revision 1 let any links with the same stroke and head share a port. That joined two buses, so a reader could trace a path that is not in the source. The reviewer's `A-->C; A-->D; B-->D; B-->E` showed it. With the new rule, D gets two ports [M9]:
  ```
      ┌───┐    ┌───┐
      │ A │    │ B │
      └─┬─┘    └─┬─┘
        │        │
    ┌───┴──┐ ┌───┴──┐
    ▾      ▾ ▾      ▾
  ┌───┐   ┌───┐   ┌───┐
  │ C │   │ D │   │ E │
  └───┘   └───┘   └───┘
  ```
- **Edges that share a link** (folio only). Several edges can join the same two nodes on adjacent ranks with no labels. The prototype kept only the last of them, so `A-->B; B-->A` drew a one-way arrow. folio merges them in declaration order:
  - edges with the same stroke share one line when their heads agree at each end (no head goes with any head, and a head goes with itself). The line's ends carry every head the edges ask for: `A-->B; B-->A` gives `◂──▸`;
  - an edge with another stroke, or another head at the same end, gets a line of its own with its own ports.
- **The port rule, extended** (folio only). A line with heads at both ends never shares a port, because a shared port would falsely connect it to the other lines there. The lines of one link keep the same order at both ends, so they don't cross.
- Order the groups by the mean order position of their far ends.
- Spread the ports over the side's interior cells, never the corners. A single port sits in the middle. In folio, ports also keep off a cylinder's rule and a subroutine's bars, and on a bare text node they use only cells of the text (step M).
- Widen the box when the groups do not fit. A bare text node can't be widened, so that step fails instead (step M).

**I. Placement across the flow** (the order axis).

- **Separation.** Elements have left and right extents. The gap between neighbours a and b is `hi(a) + gap + lo(b) + 1`, with:

  | Between | TD gap | LR gap |
  |---|---|---|
  | node and node | 3 normal, 1 compact | 1 |
  | anything and a label | 2 | 1 |
  | anything else | 1 | 1 |

- **Starting point.** Pack each rank tightly from the left.
- **Priority sweeps.** Run 3 rounds, each a down sweep (align with upper neighbours) and an up sweep (align with lower neighbours), then one final down sweep.
- **Within a rank:**
  - process elements by priority: dummies and labels first, then nodes by their number of neighbours;
  - move each one toward the median port position of its neighbours;
  - push lower-priority elements aside, but never move an element already processed.
- **Labels align with their lower neighbour** in both sweeps. This places `yes` and `no` directly above their targets, with the fan-out on one track above them (mock-up 1). In the prototype's first version, labels followed the upper neighbour, and the drawing needed a second set of jogs below the labels [M1].
- **Why not Brandes–Köpf.** The text-first proposal named a simplified Brandes–Köpf placement. This priority method reproduced the intended mock-ups and is simpler. Brandes–Köpf stays a possible upgrade; note that a 2020 erratum reports two flaws in the original algorithm [V15].

**J. Placement along the flow.**

- Each rank's thickness is its thickest element. Nodes are aligned to the start of the band.
- Each channel's thickness comes from step K. In LR, a channel may be widened further to make room for a lane label (step M).

**K. Channel routing** (between rank r and rank r+1, in the style of ELK's router [V13]).

1. **Pieces.** One piece per chain link, from a top port column to a bottom port column.
2. **Segments.**
   - Pieces that share a top port form one segment (a fan-out bus).
   - Single pieces that share a bottom port form one segment (a fan-in bus).
   - Every piece belongs to exactly one segment, and each segment is drawn as one bus (step L).
   - A segment whose tops and bottoms are all one column is straight and needs no track.
3. **Constraints between overlapping segments** (spans within one column of each other):
   - **Hard:** if a top of A equals a bottom of B, A must lie above B. Otherwise the two vertical lines would overlap.
   - **Soft:** otherwise, cost(A above B) = number of tops of B strictly inside A's span + number of bottoms of A strictly inside B's span. These are the crossings that order creates. Prefer the cheaper order.
4. **Break cycles** by removing the lightest soft edge. Number the tracks by longest path. Segments that do not overlap share a track.
   - The prototype searched for the next cycle from the start after each removal. folio carries on the depth-first search from where it stands, which removes the same edges without repeating the search. A test checks that both remove the same edges on 400 random graphs.
5. **Channel height:**
   - a stem row, if any segment needs a track or any piece has an arrowhead at its upper end;
   - plus one row per track;
   - plus a bottom row, if any piece ends at a node.
6. **Minimum heights:**
   - at least 1 row between two TD ranks that both hold nodes or labels;
   - at least 2 rows plus the stem when a straight dotted or thick piece ends at a node, so its style shows. Without this, the `-.->` edge in mock-up 1 had no visible stroke cell [M1];
   - the same when a straight piece with heads at both ends, of any stroke, ends at a node (folio). One line cell then always shows between the heads: `◂─▸`, `o─o` and `✕─✕` in compact LR, and `▴`, `│`, `▾` in TD. Without it, the heads touched (`◂▸`, `oo`, or `▴` directly above `▾`);
   - in LR, at least 4 columns (normal) or 2 (compact).
7. **Hard conflicts.** Hard constraints that form a cycle, or two segments that each need to be above the other, cannot be met without splitting a segment with an extra jog, as ELK does [V13].
   - Revision 1's prototype silently dropped a hard constraint, which drew overlapping lines.
   - Now the router counts these conflicts and the ladder step fails, so the diagram moves to the next step or the outline.
   - At 78 columns this affected 19 of 8,722 fitting tuning drawings before the port rule and 3 after it [M7].
   - M3 adds segment splitting.

**L. Painting a piece.**

- **Path.** An orthogonal path from the upper element's anchor to the lower element's anchor: (out-anchor, top) → (track, top) → (track, bottom) → (in-anchor, bottom). Straight pieces skip the track.
- **Owner.** Every line cell records which bus painted which direction bits (section 6.3).
- **Arrowheads.**
  - At a node, the arrowhead goes in the last cell before the border, and the border stays plain.
  - A reversed edge gets `▴` in the stem row.
  - A link without a head ends on the border, which turns into `┴`. A lane without a head does the same at its target (`│ A ├┄┄┐` in TD, `└─┰─┘` in LR).
  - A bare text node has no border, so a line without a head stops in the cell next to the text, pointing at it.
- **Labels.** Pieces stop one cell short of a label's text.
- **Dummies.** Dummies in a thick rank get a vertical run through it.
- **New rule: crossings.**
  - Where the lines of two different buses meet at right angles, only the line along the flow is drawn in that cell (the stem or drop: vertical in TD, horizontal in LR). The track across the flow is left out of that one cell: `──│──`.
  - A box-drawing `─` fills its cell's width, while `│` sits in the middle of its cell. The track therefore shows a gap of about half a cell on each side of the crossing [U: depends on the font; part of the visual review, risk 13].
  - No new glyphs are needed.
  - Revision 1 drew every crossing as `┼`, which reads as a junction.

  The reviewer's `A-->C; B-->D; A-->E; B-->F; C-->F; D-->E`, with both new rules [M9]:
  ```
  ┌───┐   ┌───┐
  │ A │   │ B │
  └─┬─┘   └─┬─┘
    │       │
    │   ┌───┴───────┐
    ├───│───────┐   │
    │   ▾       ▾   │
    │ ┌───┐   ┌───┐ │
    │ │ D │   │ C │ │
    │ └─┬─┘   └─┬─┘ │
    │   │       │   │
    └┐ ┌┘       └┐ ┌┘
     ▾ ▾         ▾ ▾
    ┌───┐       ┌───┐
    │ E │       │ F │
    └───┘       └───┘
  ```
  A's track passes behind B's drop to D. E and F each get two ports, because each pair of arriving links comes from different buses.

**M. Lanes and self-loops.**

- **Lanes are painted last, shortest span first.**
  - In TD, a lane leaves v's right side at its middle row, runs up a lane column, and enters u's right side next to `◂`.
  - The lane column is 2 cells right of anything drawn in the rows it spans, and far enough right for the label on the exit stub (`├─ retry ─┘`).
  - In TD, the label goes on the exit stub past every line the stub crosses, and the lane column moves right to make room: `│ C ├──│─ retry ─┘`. So the label never covers another lane's line.
  - In LR, lanes run below the drawing and enter u from the bottom with `▴`.
  - In LR, the label goes on the lane's own run, between its two corners, with a line cell at each end. It takes the first place where it covers no other line, since a lane painted later may cross the run. When placing along the flow (step J), the channel before v's rank is widened until the label fits there. If there is still no room, the step counts the label as dropped and the ladder moves on. On the held-out sample, no fitting drawing fails this way.
  - A lane without a head ends on its target's border, which becomes a tee (step L).
  - Lanes follow the crossing rule like any other bus.
- **Self-loops in TD** use this pattern, and the loop counts toward the node's right extent:
  ```
  │ Retry ├─┐ again
  │       │◂┘
  ```
- **Self-loops in LR** are built in M2 (they were not prototyped). They use the TD pattern turned: out of the bottom border, round, and back up into it, with the label to the right. The rank is widened to hold the label and 2 blank cells after it, so at least 3 blank cells separate the label from the next channel's first line. With less, `└┘ retry on failure ├─▸` read as the label of the next edge. Revision 1's prototype dropped them silently, and its checked version failed the "nothing dropped" check on every LR step. That affected 53 parsed flowcharts (37 tuning, 16 held-out) [M7].
- **One loop per node** (folio). A self-loop is drawn in its own stroke, with its own head. Several self-loops on one node share one loop when one loop can show them all: the same stroke, heads that agree, the same label or none, and no head where the loop leaves. Otherwise the step counts them as dropped, and the diagram gets the outline.
- **Shapes with inner lines** (folio). Loops and ports keep off a cylinder's rule and a subroutine's inner bars. Box borders have no owner, so a line that met them would form a `┼` that no check sees.
  - In LR, ports on a cylinder skip the rule row. In TD, ports on a subroutine skip the bar columns. Widening a box for more ports uses each shape's own margin.
  - A TD self-loop on a cylinder uses the rows below the rule, and the box gets a row for it.
  - An LR self-loop on a subroutine leaves to the right of the bar.
  - A TD lane leaves a cylinder from a row below the rule.
- **Bare text nodes** (`shape: text`, folio). A bare node is exactly its text, with no border and no padding.
  - Ports use only cells of the text. Lines and lanes leave from and end beside the text, and a line without a head stops next to it (step L).
  - A bare node that would need widening for more port groups counts as dropped, since there is nothing to widen. So does a self-loop on a bare node, since there is no border to loop on. Either way the ladder moves on.

**N. Clusters** (subgraph frames, milestone M3b; not prototyped).

- **Ordering.** Members of a cluster stay together in every rank: sort inside each cluster first, then sort sibling blocks by their mean key, innermost first.
- **Separation.** A cluster needs 2 cells on each side per nesting level. Its x-span is the union over the ranks it covers.
- **Channels.** Add one row per nesting level where a cluster starts or ends. The title goes in the top border, truncated with `…` to fit.
- **Edges across a frame** leave a gap in the frame. The gap follows the crossing rule: the edge is drawn, and the frame line is left out of that cell.
- **Checks.** Each frame must contain exactly its members and no other node.
- **Plan B.** If M3b's gate fails, evaluate ascii-dag's cluster layout (section 5.2) for up to 5 days.
- Until M3b, subgraph diagrams get the outline (section 3.3).

### 6.3 Canvas, glyphs and shapes

**Cell layers.** The canvas keeps three layers: text, glyphs and lines.

- **Text wins over glyphs, and glyphs win over lines.**
- **Node interiors are filled with blank text cells.** A line that wrongly crosses a box is therefore detected, not hidden.

**Cell owners.**

- Each line cell records which bus drew which direction bits. A bus is one of: a segment in a channel, a lane group arriving at one node, a self-loop, or a dummy's run through a rank.
- Two owners count as the same bus when they share a bus or an edge. This allows an edge's dummy run to continue its own channel piece.
- **Junction glyphs** (`┼ ├ ┤ ┬ ┴`) come only from the bits of one bus.
- **At a crossing of two buses,** only the bits of the bus in front are drawn: the line along the flow in flowcharts, and the horizontal line in sequence diagrams (section 6.4).

**Checks.** These run on every drawing, in release builds too, as part of the ladder. A step that fails a check is treated like a step that does not fit, and the ladder moves on.

1. **Text over line.** No text cell may cover a line or a box border. This includes the blank cells around a label and the second cell of a wide character. The exception is text placed over a line on purpose: frame titles, `else` titles and lane labels. Such text may cover only the line of the bus it was placed for, never another bus's line or a box border. (Before the second review, every cell under such text was exempt, so a lane label could hide another lane.)
2. **Cell check.** A cell drawn by two different buses must be a clean right-angle crossing. Any other sharing is a defect: two lines along the same cell, or a line ending on another bus's line.
3. **Connectivity trace.**
   - Follow the visible line bits cell to cell to find each connected net.
   - For a net that contains edges with sources S and targets T, every pair in S × T must be an edge of the source, in either direction. Any other pair is a false connection.
   - This is the check the reviewer asked for. It traces the painted canvas, not the router's intentions.
   - In folio, a line shared by several edges (step H) counts every edge it stands for, so `◂──▸` is a link in both directions.
   - In folio, S and T are each edge's drawn ends, its upper and lower node, not its declared source and target. These differ for a back edge reversed for ranking. With the declared ends, `X --- B` was rejected where `B --- X` drew the same picture and passed.
   - In folio, the trace reads straight through an edge label: the line ends just above and below the label count as joined. So a merge that a reader would trace through a label is caught.
4. **Hard conflicts** in the router (step K.7).
5. **Nothing dropped.** In the prototype this covered only LR self-loops. folio draws those (step M), so here it covers what folio can't place: self-loops on one node that can't share a loop, an LR lane label with no room on its run, and a bare text node that would need widening or has a self-loop.

**Measured with the checks** (at 78 columns, flowcharts without subgraphs, step chosen by fit alone) [M7]:

| | Tuning, prototype as reviewed | Tuning, with port and crossing rules | Held-out, as reviewed | Held-out, with rules |
|---|---|---|---|---|
| fitting drawings | 8,722 | 8,726 | 4,304 | 4,303 |
| with any defect | 952 (10.9%) | 20 (0.2%) | 469 (10.9%) | 10 (0.2%) |
| false connection | 871 | 8 | 424 | 4 |
| bad cell sharing | 313 | 3 | 180 | 0 |
| hard conflict | 19 | 3 | 3 | 0 |
| dropped LR self-loop | 11 | 11 | 5 | 5 |
| text over line | 0 | 0 | 1 | 1 |

With the rules, a drawing that passes may still contain clean crossings. These appear in 8.7% (tuning) and 8.9% (held-out) of fitting drawings [M7]. The few remaining defects are prototype bugs. They are never shown, because the check moves the diagram to the next step, but the gate tracks their count (section 11).

In folio (held-out sample, after both review rounds), 11 of 18,504 fitting drawings fail a check (0.1%) [M12]. That count covers every ladder step that fits at 78 columns, not only the first one, so it is not directly comparable with the table above.

**Line cells.**

- Each line cell holds light and heavy direction bits (N, S, W, E) and a dotted flag per owner.
- The glyph is looked up in a table built from the Unicode names of U+2500–U+257F. The table includes mixed weights such as `┠` and `┨`.
- A single-direction stub is drawn as a full line.
- Dashed glyphs (`┄ ┆`) are used only for straight cells whose front owner is dotted. Dotted junctions are drawn solid.

**Glyphs** (from the East Asian Width table [V8]):

| Element | Unicode | ASCII (`--diagrams ascii`) | Width class |
|---|---|---|---|
| lines and junctions | `─ │ ┌ ┐ └ ┘ ├ ┤ ┬ ┴ ┼` | `- \| +` | Ambiguous (U+2500–254B) |
| thick | `━ ┃` and the mixed junctions | `= \|` | Ambiguous |
| dotted | `┄ ┆` | `. :` | Ambiguous |
| arrowheads | `▸ ◂ ▴ ▾` | `> < ^ v` | **Neutral** (U+25B4, U+25B8, U+25BE, U+25C2) |
| async heads | `▹ ◃` | `> <` | Neutral |
| circle and cross heads | `o ✕` | `o x` | `✕` (U+2715) is Neutral |
| rounded corners | `╭ ╮ ╰ ╯` | `+` | Ambiguous (U+2550–2573) |
| decision corners | `╱ ╲` with `<`, `>` sides | `/ \ < >` | Ambiguous |
| frame rules | `── mermaid · flowchart ──` | `-- mermaid - flowchart --` | `─` and `·` (U+00B7) are Ambiguous |

**Why these arrowheads.**

- The large triangles `▲ ▼ ▶ ◀` are Ambiguous [V8].
- `▶` and `◀` also have emoji variation sequences [V9], so some terminals may draw them as wide emoji.
- The box-drawing lines themselves are Ambiguous and have no Neutral alternative. folio already draws code boxes and tables with them (`layout.rs:467`, `layout.rs:605`).

**What `--diagrams ascii` does and does not fix.**

- It replaces every Ambiguous glyph that folio adds for a diagram, including the frame rules and the `·` in the title. Folio's own drawing then has no Ambiguous characters.
- It cannot fix labels. Greek (U+0391–03A9, U+03B1–03C9) and Cyrillic (U+0401, U+0410–044F, U+0451) letters are also Ambiguous [V8].
- It cannot fix the rest of folio either (code boxes, tables, rules).
- On a terminal that draws Ambiguous characters two columns wide, the complete fix is a folio-wide option that measures them as 2 with `width_cjk()` [V10] (decision 13).

**Shapes.**

| Mermaid | Drawn as |
|---|---|
| `[a]` | `┌─┐ │a│ └─┘` |
| `(a)` | `╭─╮ │a│ ╰─╯` |
| `([a])`, `((a))`, `(((a)))` | `╭─╮ (a) ╰─╯` |
| `[[a]]` | `┌┬─┬┐ ││a││ └┴─┴┘` |
| `[(a)]` | `╭─╮ ├─┤ │a│ ╰─╯` |
| `{a}`, `{{a}}` | `╱─╲ <a> ╲─╱`; extra text rows use `│` sides |
| `[/a/]`, `[\a\]`, `[/a\]`, `[\a/]`, `>a]` | a box with the slant characters at the ends of the text rows |

**Text.**

- Labels are measured by grapheme cluster. A wide cluster takes 2 cells, and no line can land on the second cell.
- A cluster of no width, such as U+200B from `&#8203;` or a soft hyphen from `&shy;`, joins the cell before it, or is left out at the start of a label. So the cells match the columns a terminal prints, and boxes stay square.
- The left padding of a centered label is `(inner − width) / 2`.

### 6.4 Sequence diagram layout

**Columns.** Every spacing rule has the form `c_j − c_i ≥ d` with i < j, so one left-to-right pass gives the narrowest result. The rules:

| Between | Rule |
|---|---|
| neighbouring boxes | half-widths + gap (3 normal, 1 compact) + 1 |
| a message between i and j | label width + 3. A message that skips participants may cover their lifelines on its label row only; text wins there. |
| a self-message on i | label width + 6 to the next lifeline (or extends the right edge) |
| a `right of` note | note width + 3 to the next lifeline |
| a `left of` note | note width + 3 from the previous lifeline |
| an `over A` note | centered on A |
| an `over A,B` note | spans A to B plus 2 columns on each side |
| frames | span the lifelines their contents touch, plus 2 columns per nesting level on each side; frame borders must not touch outside lifelines |

**Frame titles** (`alt [valid]`, `else [invalid]`) start 2 columns right of the frame's first lifeline.

- In the normal and compact steps, a title must end before the next lifeline.
- In the narrow steps, a title may run over lifelines (text wins), as at 40 columns in section 7.3.

The prototype measured why: requiring titles to fit in every step was the widest constraint for many real diagrams [M1].

**Rows:**

| Element | Rows |
|---|---|
| header boxes | label lines + 2 |
| message | label lines + 1 arrow row (+1 spacer row in the normal step) |
| self-message | max(2, label lines) |
| note | lines + 2 |
| frame open, divider, close | 1 each |
| destroy | 1 |

No boxes are repeated at the bottom.

**Glyphs and crossings:**

- A message starts with a tee on its source lifeline and ends with its head next to its target lifeline. Replies are dotted (`┄`).
- **New rule: messages, frame borders and `else`/`and` dividers are drawn in front of every lifeline they cross.**
  - A lifeline shows a tee only where a message meets one of its own two lifelines.
  - `┼` never appears in a sequence diagram.
  - Revision 1 drew `A->>C` across B's lifeline as `├───────┼──────▸│`. It is now `├──────────────▸│` [M9].
- **Check:** on a message's row, the cells between its two ends must hold no vertical line bits.
- **Why the rule matters:** 63.3% of tuning sequence diagrams and 52.9% of held-out ones have a message that skips a lifeline. In revision 1's prototype, 49.5% and 44.9% of fitting drawings at 78 columns showed such a message as a junction [M7].
- An active lifeline is heavy (`┃`), and messages attach to it with `┠` or `┨`.
- `autonumber` prefixes each label with its number.
- A lifeline stops at a note that covers it (`┴` on top, `┬` below) and continues after it.

**Known prototype bugs.** Text over a line remains in 2.7% (tuning) and 4.2% (held-out) of fitting sequence drawings at 78 columns. Bad cell sharing remains in 1.7% and 0.9% [M7]. These diagrams fall to a later step or the transcript, and they are bugs to fix in M4.

### 6.5 Outline and transcript

Both sit between the same two rules as a drawing. The top rule says `mermaid · flowchart (outline)` or `mermaid · sequence (transcript)`. Unlike drawings, they are not centered.

**Flowchart outline.**

- **Subgraphs first.** If the diagram has subgraphs, one line per subgraph comes first, in tree order: each subgraph is followed by those nested in it, and siblings keep their declaration order (section 3.3). For example, `Group one: Receive order, Ship it`. Nested subgraphs are indented 2 columns per level.
- **Then the edges.** One line per visible edge, in declaration order: `Receive order ─▸ In stock?`, `In stock? ─yes─▸ Ship it`, `Back-order ┄▸ Orders DB`.
  - The line is `─`, `┄` or `━` for a solid, dotted or thick edge. The heads are `▸` and `◂` for arrows, `o` for a circle and `✕` for a cross.
  - An end that named a subgraph shows the subgraph's name, not the member the edge was attached to (section 3.3).
- **Then the nodes that no visible edge links.** This includes nodes linked only by invisible links, which the prototype left out.
- Lines wrap with a 4-column hanging indent. A nested line starts only 2 columns deeper, so a wrapped row can't be mistaken for one.

**Sequence transcript.**

- **Participants.** When some participant has no line of its own (no message, note, `create` or `destroy`), the first line lists them all: `Participants: API, Bob (actor), Client`. So no participant is lost. A participant named in a box line (below) counts as having a line.
- **Boxes.** Then one line per `box`: `box Frontend services: Web, Mobile`. A box without a title gives `box: API`, and one without members gives `box Title`. A box with neither gets no line.
- **Messages** are numbered 1, 2, 3 when the diagram has no `autonumber`. Otherwise the numbers follow Mermaid:
  - the count starts at 1 with a step of 1, and runs through every message, whether its number is shown or not;
  - each `autonumber` statement takes effect where it stands: a start sets the next number (and the step back to 1), a step sets the step, and 0 counts as left out;
  - numbers are shown only after an `autonumber` and before an `autonumber off`.

  The arrow shows the stroke and the head: `─▸` and `┄▸` for arrows, `──` and `┄┄` for no head, `─✕` for a cross, `─▹` for async, and `◂─▸` for `<<->>`.
- `Note right of API: …` lines.
- `create participant X` (or `create actor X`) and `destroy X` lines, where they happen. Activations get no line.
- Block headers (`alt valid`, `else invalid`, `end`) sit at the block's level, with their contents indented 2 columns.

**Always fits.**

- A line starts at most a third of the width in, and its wrapped rows at most 2 columns past that. Deep `alt`/`loop` nesting inside a list or quote, where the inner width can be as small as 8 (`MIN_INNER`, `layout.rs:17`), stops indenting there.
- Lines are wrapped with the diagram module's `pre::wrap`, which breaks words longer than a line, as folio's existing wrap does (`text.rs:153`). A word broken this way is not joined back together with a space.
- **Length.** An outline or transcript stops after 2,000 rows and ends with a line such as "900 more lines not shown".

Both are ordinary prose flows for search.

### 6.6 Determinism, limits and speed

**Determinism.**

- Only `Vec` iteration feeds the output. Ties are broken by declaration order.
- No randomness.
- The work limits below never depend on the clock, so output is the same on every run and machine.

**Speed measured in the prototype** (single process, Python, with the fixes and checks; 1,500 random parsed flowcharts; parse, preparation and every ladder step) [M8]:

- p50 17.0 ms, p90 42.9 ms, p95 54.2 ms, p99 131.6 ms, max 789 ms.
- **The slowest diagrams are wide, not just big.**
  - The slowest of all took 10.3 s. It has 66 nodes, 250 edges, 2,732 dummies and a widest rank of 101 elements.
  - Two diagrams with **no dummies** took 2.5 s and 2.1 s. Each has one rank of 78 to 82 nodes.
  - So the cost tracks the widest rank (elements in one rank, dummies included) and the total number of elements more than the node count.
  - Across all 17,836 parsed flowcharts, the widest rank has p50 3, p99 12, p99.9 29 and max 101. The element count has p99 60, p99.9 177 and max 2,798.
- Rust should be much faster [U]. The gate below is what counts. Measured in folio after both review rounds, all ladder steps of one diagram take p50 1.4 ms, p99 6.0 ms and at most 23.5 ms on the held-out sample [M12].

**Limits per diagram.**

- **Size caps.** 150 nodes, 300 edges, 40 participants, 500 events or 64 KiB of source. Then, after preparation:
  - a widest rank of at most 40 elements;
  - at most 300 elements in all.

  9 and 4 of 17,836 parsed flowcharts exceed these two caps [M8] [U: tune on Rust timings]. Over a cap, the outline is shown with "too large to draw here".
- **Large sources** (folio). A source over 64 KiB is still parsed, up to 256 KiB, so it gets its outline with "too large to draw here". A larger source isn't read at all and shows its source with the same reason.
- **Parser guard** (folio). Groups multiply: `A & B & … --> …` makes one edge per pair, and each edge copies the label. So the flowchart parser stops with "too large to draw here" (the source is shown) when a statement would take the diagram past 5,000 edges, or past 256 KiB of label text copied into edges. This guards memory, not time.
- **Nesting guard** (folio). The checks that keep subgraphs from nesting in themselves (section 3.3) stop after 2,000,000 steps, with the same result. Real diagrams need a few hundred; thousands of deeply nested subgraphs could otherwise take seconds.
- **Work counter.**
  - `Work` counts inner-loop steps: crossing counting, transpose, placement pushes, the router's constraint pairs, cycle breaking and track pass, every canvas cell written and every owner scanned, the checks (each cell and owner, and the owner pairs in shared cells), and rendering.
  - Because cells and owners are counted, the count tracks time: about 15,000 to 29,000 steps per millisecond in a release build. Before the second review, painting, cycle breaking and the checks were mostly not counted. The rate then differed about sevenfold between diagrams, and a dense diagram inside the size caps took 56 to 75 ms per ladder step without reaching the limit.
  - All ladder steps of one diagram share one limit.
  - When it runs out, the diagram gets the outline with "too large to draw here".
  - The limit is `WORK_LIMIT = 500_000` (`src/diagram/mod.rs`). A diagram that hits it has spent roughly 17 to 33 ms; the plan aimed for about 20 ms. The corpus's p99.9 diagram needs about 400,000. On the held-out sample, 5 diagrams (0.1%) are over the limit or the size caps after preparation [M12]. This limit was planned for M3 and was set after the second review. It replaces M2's provisional 50,000,000.
- **Viewer.** `render_full` gets 4 times the work limit. If that runs out, the viewer shows the outline and says so. Pressing `Enter` therefore cannot freeze the pager for longer than the limit allows [U: target ≤ 100 ms].

**Limit per document (pager cold start; M3).**

- PLAN.md's budget is "starts in under 20 ms" (`PLAN.md:24`).
- A document can hold many diagrams. In the Sourcegraph samples, the number of Mermaid fences per file is p50 1, p90 4, p99 12 and max 101; 0.11% of files have 30 or more [M10]. These are lower bounds, since search results may not return every fence in a file [U].
- **The rule** [U: budget value]:
  - In the pager, the first layout of a document gives all its diagrams together a shared budget of about 15 ms of work.
  - Diagrams past the budget show their outline with the note "drawing…".
  - The pager then draws them in reading order, starting at the screen, one per idle turn of its event loop. The loop polls every 500 ms today (`pager.rs:32`, `pager.rs:315`) and polls with no wait while drawings are pending.
  - After each one it lays out again, keeping the reader's place (`anchor_at`/`line_for`) and focus (section 8.3).
- **The `less -F` check** (`main.rs:157-163`) uses the same budget. If any diagram was deferred, folio opens the pager instead of printing, so printed output never contains "drawing…". The `Diagrams` cache from that check is passed into the pager, as the `Highlighter` already is (`main.rs:166`), so no work is repeated.
- **Print mode, pipes and tests** have no document budget, only the per-diagram limits, so their output stays complete and deterministic.
- **This replaces revision 1's fallback.** That fallback ("move slow cases to Phase 3's job queue") relied on a job system that folio does not have (`PLAN.md:64`). This plan needs no background threads. Decision 4 reworded `PLAN.md:64` to match.

**Gate (M3).** p99 ≤ 20 ms for all ladder steps of one diagram in a release build, on the held-out set; and no more than 0.2% of diagrams hit the work limit [U: thresholds]. Both already hold: p99 6.0 ms, and 0.1% over the work limit or the size caps [M12].

---

## 7. Fitting the width

### 7.1 The ladder

Each step is a complete render plus the checks. Ranking, ordering and lane selection are reused across steps. The first step whose widest row fits the width **and** passes the checks wins.

| Step | Flowchart | Sequence |
|---|---|---|
| 1 Normal | node gap 3 (TD), channel 4 (LR), labels 28 | box gap 3, spacer rows, labels 28, strict titles |
| 2 Compact | node gap 1, channel 2 | box gap 1, no spacer rows |
| 3 Turned | LR/RL only: steps 1–2 drawn top-down; the bottom rule says "drawn top-down to fit" | — |
| 4 Labels 16 | compact, TD | compact, labels and names at 16, relaxed titles |
| 5 Labels 8 | compact, TD | compact, labels at 8 |
| 6 Outline | outline | transcript |

**Ladder order for LR diagrams (decision 10).** The ladder above (A) turns an LR diagram top-down before it tries narrower labels in LR. That changes the author's layout often, and turned chains get tall: 7 boxes become 27 rows (section 7.3). Two alternatives were measured on held-out LR flowcharts without subgraphs (2,672) [M7]:

| Ladder | LR steps before turning | At 78: keep LR | At 78: drawn top-down | At 78: drawn | Median height vs the normal LR drawing | At 100: keep LR |
|---|---|---|---|---|---|---|
| A (revision 1) | normal, compact | 930 (34.8%) | 1,598 | 94.6% | ×2.38 | 1,290 (48.3%) |
| B | normal, compact, labels 16 | 1,155 (43.2%) | 1,426 | 96.6% | ×2.00 | 1,566 (58.6%) |
| C | normal, compact, labels 16, labels 8 | 1,415 (53.0%) | 1,189 | 97.5% | ×1.52 | 1,875 (70.2%) |

- "Drawn top-down" counts every step that draws an LR diagram top-down, including labels 16 and 8.
- The tuning sample points the same way: at 78 columns, A keeps 24.6% of LR diagrams in LR and C keeps 45.1% [M7].
- For all flowcharts without subgraphs, the drawn share at 78 columns is 92.9% (A), 94.0% (B) and 94.5% (C) on the held-out sample.
- TD diagrams use the same steps under every ladder.
- **Decided: C** (decision 10), as recommended. It keeps the author's direction more often, draws more diagrams and makes them shorter. folio's LR ladder is normal, compact, LR labels 16, LR labels 8, turned, turned compact, labels 16 and labels 8. The visual review of LR drawings with 8-column labels is still to do (gate 5).

**Never clip.**

- Print and piped output never clip.
- The pager shows the outline rather than a clipped drawing. A clipped sequence diagram would drop participants.
- `Enter` always shows the full drawing, within the viewer's work limit. This comes with the viewer in M5; until then `Enter` copies the source.

### 7.2 Which width applies

- **Layout width** (tests and print mode) is `min(--width, terminal)`. `--width` defaults to 100 on a terminal and 80 when piped (`main.rs:133`, `main.rs:156`).
- **The pager's content width** is the terminal width − 2, capped at the reading width of 100 (`pager.rs:345`). That gives 38 at a 40-column terminal, 78 at 80 and 100 at 120.
- **With the contents pane open** (`t`) on a terminal 70 or more columns wide, the pane takes `w / 3` columns, clamped to 24–36, plus one separator column (`pager.rs:335-338`). The content width becomes `w − pane − 3`: **51 at 80 columns**, 64 at 100 and 81 at 120.
  - At 51 columns, 67.5% of held-out flowcharts without subgraphs and 45.6% of sequence diagrams are drawn. At 78 columns the figures are 92.9% and 78.6% [M7].
  - Opening the pane on an 80-column terminal therefore turns about a quarter of flowcharts and a third of sequence diagrams into outlines, and changes the document's line count.
  - The cache makes the switch instant. The reader's place is kept by the existing anchor, and focus by section 8.3.
  - Decision 11 accepts this.
- **The open frame** (a labeled top rule and a bottom rule, like the Footnotes separator at `layout.rs:305-316`) gives the drawing the whole width. A code box would cost 4 columns.

### 7.3 Example diagrams

Output of the prototype with the fixes, ladder A [M9]. Sizes are width × height.

| Diagram | Natural | 38 (pager, 40-col terminal) | 51 (80-col terminal, contents pane open) | 78 (pager, 80-col terminal) | 100 (pager, 120-col terminal) |
|---|---|---|---|---|---|
| mock-up 1 (TD, decision, loop) | 39×20 | compact 37×20 | normal | normal | normal |
| mock-up 2 (3 participants, `alt`) | 46×26 | transcript; at 40: labels 8, 40×21 | normal | normal | normal |
| `flowchart LR` with 7 boxes | 90×3 | turned 13×27 | turned 13×27 | compact 78×3 | normal |
| TD decision with 3 labeled branches | 65×14 | labels 8, 37×18 | labels 8, 37×18 | normal | normal |
| TD, 7 nodes, long labels, loop | 57×23 | labels 16, 38×26 | labels 16, 38×26 | normal | normal |
| sequence, 4 participants, note, activation, `loop` | 79×32 | transcript | transcript | labels 16, 75×25 | normal |
| sequence, 6 participants, long messages | 144×34 | transcript | transcript | labels 8, 76×47 | labels 8 (labels 16 at 118: 108×33) |

All of these pass the checks at every width shown.

**Mock-up 1 at 38 columns** (compact):
```
── mermaid · flowchart ───────────────
    ┌───────────────┐
    │ Receive order │
    └───────┬───────┘
            ▾
      ╱───────────╲
      < In stock? >◂────────────────┐
      ╲─────┬─────╱                 │
            │                       │
     ┌──────┴──────┐                │
    yes            no               │
     ▾             ▾                │
┌─────────┐ ┌─────────────┐         │
│ Ship it │ │ Back-order  ├─ retry ─┘
└────┬────┘ └──────┬──────┘
     │             ┆
     ▾             ▾
 ╭───────╮   ╭───────────╮
 ( Done  )   ├───────────┤
 ╰───────╯   │ Orders DB │
             ╰───────────╯
──────────────────────────────────────
```

**An LR chain turned top-down at 38 columns** (excerpt; the full drawing is 27 rows):
```
── mermaid · flowchart ───────────────
              ┌───────┐
              │ Lint  │
              └───┬───┘
                  ▾
              ┌───────┐
              │ Test  │
              └───┬───┘
                  ▾
                 …
             ┌─────────┐
             │ Deploy  │
             └─────────┘
───────────── drawn top-down to fit ──
```

**Mock-up 2 at 38 columns** (transcript):
```
── mermaid · sequence (outline) ──────
1. User ─▸ Web app: Log in
2. Web app ─▸ Database: Find user
3. Database ┄▸ Web app: User row
4. Web app ─▸ Web app: Check hash
alt valid
  5. Web app ┄▸ User: Session cookie
else invalid
  6. Web app ┄▸ User: 401
end
─────────────── Enter: full drawing ──
```

**Mock-up 2 at 40 columns** (labels 8, relaxed titles). The `else` title runs over the Web app lifeline on its row; this is the accepted compromise of the narrow steps. Frame borders are drawn in front of the lifelines:
```
── mermaid · sequence ──────────────────
┌───────┐      ┌─────────┐ ┌───────────┐
│ User  │      │ Web app │ │ Database  │
└───┬───┘      └────┬────┘ └─────┬─────┘
    │ Log in        │            │
    ├──────────────▸│            │
    │               │ Find       │
    │               │ user       │
    │               ├───────────▸│
    │               │ User row   │
    │               │◂┄┄┄┄┄┄┄┄┄┄┄┤
    │               ├─┐ Check    │
    │               │◂┘ hash     │
  ┌─── alt [valid] ───┐          │
  │ │ Session       │ │          │
  │ │ cookie        │ │          │
  │ │◂┄┄┄┄┄┄┄┄┄┄┄┄┄┄┤ │          │
  ├┄┄┄ else [invalid] ┤          │
  │ │ 401           │ │          │
  │ │◂┄┄┄┄┄┄┄┄┄┄┄┄┄┄┤ │          │
  └───────────────────┘          │
    │               │            │
────────────────────────────────────────
```

### 7.4 Real diagrams

**What "drawn" means from now on.** A diagram counts as drawn at a width only if some ladder step meets all of these:

- it fits the width;
- it has no text-over-line cells;
- it has no bad cell sharing;
- it has no false connection;
- it has no hard conflict;
- it drops nothing.

All figures below use that definition. Revision 1 checked only the width, and only counted violations for the normal step.

**Two samples** [M7]:

- **Tuning:** the 17,450 blocks the prototype was developed on. The grammar was revised after looking at failures in it, so its numbers are optimistic.
- **Held-out:** 6,496 blocks from four other Sourcegraph result files collected earlier today (README files, architecture decision records, selected repositories, a test query). Any block also in the tuning sample was removed. No held-out diagram was looked at while building the prototype; this revision looked only at aggregate counts.
- **Gates are measured on the held-out sample only** (section 11). If it is ever used for tuning, a fresh held-out sample must be collected before the gate (risk 14).

**Flowcharts** (prototype with the fixes, ladder A):

- Tuning: 12,706 flowcharts. 207 went to source, 5 were over the size caps, 2 crashed and 12,492 parsed (98.3%); 2,551 of these have subgraphs.
- Held-out: 5,632 flowcharts. 279 went to source, 9 crashed and 5,344 parsed (94.9%); 717 of these have subgraphs.

| Width | Tuning, no subgraphs (9,941) | **Held-out, no subgraphs (4,627)** | Held-out, subgraph subset (717), frames not drawn | **Held-out, all parsed, subgraphs → outline (5,344)** | Tuning, all parsed, subgraphs → outline (12,492) |
|---|---|---|---|---|---|
| 38 | 47.9% | 49.6% | 30.7% | 42.9% | 38.1% |
| 51 (contents pane open at 80) | 67.0% | 67.5% | 51.5% | 58.5% | 53.3% |
| 78 | 87.7% | 92.9% | 71.3% | 80.4% | 69.8% |
| 100 | 93.2% | 97.1% | 80.2% | 84.1% | 74.1% |
| 118 (full width of a 120-col terminal) | 95.5% | 98.2% | 84.2% | 85.0% | 76.0% |
| 158 (full width of a 160-col terminal) | 98.2% | 99.2% | 88.0% | 85.9% | 78.1% |

- **The subgraph subset.** The "frames not drawn" column is an upper bound for what M3b can reach, since frames only add width. It is not shown to readers until frames exist.
  - In the tuning sample, 61.9% of subgraph flowcharts would be drawn at 78 columns even without frames, and 38.1% would still get the outline.
- **What the old figures hid.** In the prototype as reviewed, the old "fits" definition gave 87.7% (tuning) and 93.0% (held-out) for flowcharts without subgraphs at 78 columns. Under the checks, the figures fall to 78.3% and 83.0%. The two routing rules bring them back to 87.7% and 92.9% (section 6.3).
- **Which steps were used at 78** (held-out, no subgraphs, ladder A): normal 2,209, compact 104, turned 1,372 + 37, labels 16 385, labels 8 191, outline 329.
- **The same in folio** (ladder C, after both review rounds; 4,638 flowcharts) [M12]: normal 2,223, compact 105, LR labels 16 210, LR labels 8 288, turned 1,023, turned compact 30, labels 16 346, labels 8 177, outline 236.
- **Natural width** (tuning, normal step, source direction): median 90, p90 208.
- **Natural height:** median 19, p90 47. 8.6% are taller than 48 rows.
- **merman-ascii, for comparison:**
  - it refused 18.0%;
  - only 48.6% of all flowcharts were 74 columns or narrower, even with its compact profile and a top-down rewrite;
  - 38.4% of the flowcharts it drew were taller than 48 rows [M4].

**Sequence diagrams:**

- Tuning: 2,221 diagrams; 58 went to source and 2,163 parsed (97.4%).
- Held-out: 423 diagrams; 11 went to source and 412 parsed (97.4%).

| Width | Tuning, old "fits" definition, prototype as reviewed | Tuning, checks, with the fixes | **Held-out, checks, with the fixes** |
|---|---|---|---|
| 38 | 12.8% | 12.7% | 11.2% |
| 51 | 27.7% | 27.0% | 45.6% |
| 78 | 67.3% | 65.1% | 78.6% |
| 100 | 86.8% | 83.3% | 89.8% |
| 118 | 94.3% | 90.2% | 92.5% |
| 158 | 99.3% | 94.8% | 95.1% |

- Under the checks, the prototype as reviewed would draw only 32.9% (tuning) and 43.9% (held-out) at 78 columns, because of message junctions.
- The gap at wide widths between the old and new columns comes from the text-over-line bugs in section 6.4.
- **Natural width:** median 91.
- **Natural height:** median 32. 18.7% are taller than 48 rows.
- **merman-ascii, for comparison:** 21.8% of sequence diagrams were 74 columns or narrower. It cannot wrap message labels [M4].

**What this means.**

- Most real diagrams do **not** fit at 80 columns in their normal form. The ladder is what makes them fit.
- At 40 columns, most readers get outlines and transcripts. That is the right result: those always fit and keep every fact.
- With the contents pane open on an 80-column terminal, about a third of flowcharts and half of sequence diagrams become outlines.

### 7.5 The full-screen viewer

- **What it is.** `Enter` on a focused diagram opens a modal overlay. Like `help`, it is not a new `Screen`, so it never enters the history.
- **What it shows.** The normal step at unbounded width, in the source's own direction, drawn with `render_full`.
  - It works even when the document shows an outline.
  - A subgraph diagram before M3b shows its source in the viewer, since a drawing without frames would lose the grouping.
- **Bounded.** `render_full` has 4 times the normal work limit (section 6.6). If that runs out, the viewer shows the outline with "too large to draw".
- **Drawing.** Rows go through a new `text::slice`. A header shows the size and the visible range: `132×48  cols 0–78`.
- **Keys:**
  - `h j k l`, the arrows, `PgUp`/`PgDn`, `0`/`$` and `g`/`G` pan;
  - `s` switches between drawing and source;
  - `y` copies the source;
  - `c` copies the drawing as text;
  - `Enter`, `q` and `Esc` close.
- **Watch reloads.** The viewer finds its diagram again by its key (section 8.2) or closes with a message.

### 7.6 Using more than the reading width

- **The problem.** The pager caps content at 100 columns even on a 160-column terminal.
- **The data** (drawn share at 100, 118 and 158 columns) [M7]:

  | Sample | 100 | 118 | 158 |
  |---|---|---|---|
  | Held-out flowcharts, no subgraphs | 97.1% | 98.2% | 99.2% |
  | Tuning flowcharts, no subgraphs | 93.2% | 95.5% | 98.2% |
  | Held-out sequence diagrams | 89.8% | 92.5% | 95.1% |
  | Tuning sequence diagrams | 83.3% | 90.2% | 94.8% |

  The gains on the held-out sample are smaller than revision 1 suggested.
- **The work.** It needs an x-offset on `RLine`, which touches drawing, search highlights, focus painting and the OSC 8 re-print.
- **Proposal.** Do it as M7, after M5, for diagrams and wide tables together (decision 5). PLAN.md now leaves it to Phase 4.

---

## 8. Integration with folio

### 8.1 Changes by file

This table describes what M0 to M2 changed. Line references point to the code before the change (2026-10-23). Items for later milestones are marked with the milestone.

| File | Change |
|---|---|
| `src/doc.rs` | New `BlockKind::Diagram { lang: String, source: String, closed: bool }` (enum at `doc.rs:21-37`). `lang` is the fence's first info word, as for code blocks. In the `NodeValue::CodeBlock` arm (`doc.rs:186-190`), a **fenced** block becomes a `Diagram` when its first info word, with at most one surrounding `{…}` pair removed, is `mermaid` in any case (`is_mermaid`). So `{Mermaid}` is a diagram, and `mermaid}`, `{{mermaid}}` and `{}` are code blocks. `mmd` is not accepted (decision 3). `~~~` fences need nothing extra. An unclosed fence is still a `Diagram`, so a half-typed diagram under watch mode shows its source with "Not drawn: the code fence isn't closed", not an error. A fence counts as closed when comrak closed it, or when any non-blank source line comes after its last line. So a fence ended by its quote or list item, with text after it, is drawn. Only a fence cut off by the end of the document is unclosed. Trailing blank lines are trimmed from the source of a fence with no closing line. |
| `src/layout.rs` | `TargetKind::Diagram(u32)` (`layout.rs:57-61`), indexing a new `Rendered::diagrams: Vec<DiagramRef>`. A `DiagramRef` holds the source, the toggle key (8.2), the kind, how the diagram is shown (`Shown::Drawing`, `Outline` or `Source`), the shown text for `c`, and a note for the footer. Like `links`, this index is global, so `nest()` (`layout.rs:106-140`) needs no renumbering. `layout()` (`layout.rs:201`) gains `dg: &mut Diagrams`. The `Layouter` struct (`layout.rs:286-291`) gains `dg`, `diagrams` and a count of the diagrams seen per source hash. A new arm in `block()` (`layout.rs:326`) calls the new `Layouter::diagram()`, which handles `--diagrams source`, the reader's toggle, an unclosed fence, and otherwise calls `diagram::render`. `Layouter::framed()` draws the rules and the rows. **The width assertions are in `Layouter::framed`**, as `debug_assert!`s that the drawing and every row are at most the `width` passed in. They cannot go in `Rendered::push` (`layout.rs:95`): blocks build their output in `Rendered::default()`, whose `width` is 0 until `layout()` sets it (`layout.rs:205`), so they would fail on every line. The existing `lines_never_exceed_the_width` test still covers the final output. The source fallback, `Layouter::diagram_source()`, calls `code()` (`layout.rs:445`), sets the target kind to `Diagram` (8.2), then adds the note line. |
| `src/text.rs` | The comment on `fit()` is fixed: it said "Pad or truncate" but only pads (`text.rs:250-257`). Grapheme-aware `clip` and `slice` come with the viewer (M5). |
| `src/theme.rs` | Two new palette entries per theme, `diagram_line` and `diagram_edge` (`theme.rs:54-99`), and four new `Theme` styles (`theme.rs:14-49`): `diagram_line`, `diagram_edge`, `diagram_label` and `diagram_note`, set in `Theme::new` (`theme.rs:103`); see 8.5. |
| `src/pager.rs` | `s`, `c`, the footer, help and messages (8.2); `Pager::relayout()` and focus kept across relayouts (8.3); the `s` toggles kept per document (8.2). The idle drawing step (6.6) comes in M3. |
| `src/viewer.rs` (new, M5) | `Viewer`, `ViewerAction`. Not written yet. |
| `src/main.rs` | `--diagrams draw\|source` in `Args` (`main.rs:24-53`), default `draw`. `ascii` comes with M3. `source` reproduces the earlier output and behavior exactly (8.2). One `Diagrams` is built in `main` and used by the print path and the `less -F` check (`main.rs:157-163`). It is passed into `run_pager` with the `Highlighter`, so its cache is reused. Opening the pager when the check deferred a diagram (6.6) comes with the budget in M3. `TERM=linux` changes nothing (decision 9). |
| `src/lib.rs` | `pub mod diagram`. |
| `Cargo.toml` | `regex` as a direct dependency (5.1). |
| `tests/render.rs` | `"diagrams"` in `FIXTURES` (`tests/render.rs:8`), with `tests/fixtures/diagrams.md`. A `Diagrams` passed into `render()` (`tests/render.rs:15-18`), with no document budget. The two direct `layout::layout` calls in `urls_are_hidden_when_the_terminal_shows_hyperlinks` (`tests/render.rs:159`, `tests/render.rs:162`). A `"diagram:<kind>"` arm in the exhaustive match (`tests/render.rs:132-135`). New tests for fences, unclosed fences, sources over 64 KiB, title truncation, bottom-rule notes, widths, `--diagrams source` and role styles. The style snapshots (section 10, item 6) are not written yet. |

**Call sites of `layout()`:** there are six, not five as the text-first proposal said.

- `main.rs:160` and `main.rs:167`;
- `pager.rs:358`;
- `tests/render.rs:17`, `tests/render.rs:159` and `tests/render.rs:162`.

### 8.2 Frame, focus and keys

**Frame.**

- A top rule labeled `── mermaid · flowchart ──`, with the title when there is one (`── mermaid · flowchart · Release flow ──`), and a bottom rule carrying the notes. The title comes from the front matter or from a sequence diagram's own `title` statement. Outlines and transcripts say `flowchart (outline)` or `sequence (transcript)`.
- **Truncation.**
  - A rule's label must fit in the width minus 6, so at least 2 `─` show on each side.
  - The title matters more than "(outline)" or the kind. It is shown in full, or cut with `…` to at least 10 columns, after the longest prefix that leaves room: `mermaid · flowchart (outline) · `, then `mermaid · flowchart · `, then `mermaid · `. Failing that, it is cut shorter.
  - Without room for a title, the label falls back to `mermaid · flowchart (outline)`, then `mermaid · flowchart`, then `mermaid`. If even `── mermaid ──` does not fit, the rule is plain `─`.
  - Notes in the bottom rule are joined with "; " and cut with `…` to the width minus 6. If fewer than 4 columns of them would be left, the bottom rule is plain.
- A drawing is centered when it is narrower than the content width. Outlines and transcripts start at the left edge.
- **The note line under a source fallback.**
  - It reads "Not drawn: <reason>". Quoted user text is cut to 30 columns with `…`, and the reason is cleaned with `doc::clean_line`, so it holds no control characters.
  - The line is wrapped like a paragraph, so it fits at any width.

**Focus target.**

- A drawn diagram's target spans are its two rules, at full width. Focus paints them with `focus_border`, through a `TargetKind::Diagram(_)` arm next to the code arm (`pager.rs:1223-1226`).
- Targets are already sorted in reading order (`layout.rs:164`), so `Tab` and `Shift-Tab` reach diagrams with no other change.
- **Source views and target kinds.**
  - With `--diagrams source`, every Mermaid block is the earlier code box with `TargetKind::Code`: `Enter` and `y` copy, as before. Output and behavior match the earlier ones exactly.
  - A diagram that falls back to source, or that the reader switched to source with `s`, keeps `TargetKind::Diagram`. That way `s` switches it back. On a diagram that fell back to source, `s` says why it isn't drawn.
  - Until the viewer exists (M5), `Enter` on any diagram copies its source, as it does on a code block (decision 2).

**Per-diagram toggle state (`s`).**

- The set of diagrams switched to source lives in `Diagrams`, as `as_source: HashSet<(u64, u32)>`.
- The key is (hash of the diagram's source, its occurrence number among the document's diagrams with that same hash).
- Layout reads the set through `dg`, so it stays a pure function of document, width, theme and diagram settings.
- **Behavior:**
  - Two identical diagrams have different occurrence numbers, so they toggle on their own.
  - Across a watch reload, a toggle survives edits elsewhere in the document and new diagrams added above it.
  - Editing the diagram's own source resets it to drawn.
  - Adding an identical copy above it shifts the occurrence numbers for that source only. This is rare.
  - **Toggles belong to one document.** When another document opens, the pager puts the current set away under the document's path (`other_toggles`) and brings back the new document's set. Going back to a document restores its toggles. A watch reload of the same file keeps them.
- **Why not position alone:** keying by position among diagrams would move every toggle when a diagram is added above.
- `s` calls `Pager::relayout()` (8.3).

**Keys.** `s` and `c` were unbound before M0; the key match is at `pager.rs:853-907`. `s` and `c` were planned for M5, but they were built in M0, because `Pager::relayout()` made them cheap. The viewer column is M5.

| Key | Focused diagram | In the viewer (M5) |
|---|---|---|
| `Enter` | copy the Mermaid source, as on a code block (`activate`, `pager.rs:764`); from M5, open the viewer | close |
| `s` | switch between drawing and source in place | switch |
| `y` | copy the Mermaid source (`copy_focused`, `pager.rs:793`) | copy the source |
| `c` | copy what is shown (drawing or outline) as plain text | copy the full drawing |

Without a focused diagram, `s` and `c` say "Press Tab to pick a diagram, then s" (or `c`).

**Footer** (`draw_target_info`, `pager.rs:1378`; the text now comes from `Pager::target_info`, so it can be tested).

- A drawing: `Mermaid flowchart · drawn top-down to fit`, followed by `s source  y copy source  c copy drawing`.
- An outline: `Mermaid flowchart · outline · subgraphs aren't drawn yet`, followed by `s source  y copy source  c copy outline`.
- A sequence diagram's outline is called a transcript, as in its top rule: `Mermaid sequence · transcript`, followed by `s source  y copy source  c copy transcript`. After `c`, the message says "Copied the transcript to the …".
- A source fallback: `Mermaid · not drawn: <reason>`, followed by `y copy`. A diagram the reader switched to source offers `s drawing  y copy`.
- `Enter view` joins the keys when the viewer exists (M5).

**Messages and help** (built).

- `next_target` (`pager.rs:721-728`) and `copy_focused` (`pager.rs:795`) say "link, code block or diagram".
- `HELP` (`pager.rs:40-61`) mentions diagrams for `Tab` and `y`, and has lines for `s` and `c`.
- The raw-view message is "Links, code blocks and diagrams are in the rendered view (press r)".

**Viewer wiring** (M5). `viewer: Option<Viewer>` in `Pager` (`pager.rs:150-188`):

- checked first in `on_key`, as `help` is (`pager.rs:819-824`);
- drawn last, as `help` is (`pager.rs:1177-1180`);
- `screen_links` returns nothing while it is open (`pager.rs:1233`).

### 8.3 Keeping focus across relayouts

Before M0, `rebuild()` cleared focus (`pager.rs:371`). Phase 2 lists this as a known gap (`PLAN.md:282`). `s`, the idle drawing step and the contents pane would make it worse.

**Fix** (built):

1. `Pager::relayout()` calls `rebuild()` at the current width (`self.view.width`). Before, the pager laid out again only when the width changed (`pager.rs:303-305`), so `s` and the idle step need this path.
2. Before any relayout or reload, the pager remembers the focused target by its content, as a `FocusKey` (`pager.rs`):
   - a link by its URL and its place among the document's links to that URL;
   - a code block by its language, its text and its place among identical code blocks;
   - a diagram by its `DiagramRef` key, the same (source hash, occurrence) key the `s` toggles use (8.2).
3. Afterwards, it looks the target up again by that key.

No part of the key depends on where the target is drawn or on its source line. So focus stays on the same target when wrapping changes (a table cell that wraps can reorder its links) and when a reload moves lines. The reader's scroll place is kept separately, by `anchor_at`/`line_for`.

**When focus is cleared.** If the target is gone, focus is cleared; it is never moved to another target. That happens after a reload that removed the target or changed its content (for a diagram, its source), and in the raw view, which has no targets. Focus is also cleared when another document opens, and by `Esc`.

The first version (M0) used (source line of the target's first line, kind, place among the targets on that line). The second review found that this moved focus to the wrong target in two cases: a wrapped table cell that reorders its links, and a reload that inserts lines above the target.

### 8.4 Search and reading time

**Flows come from the drawing's `labels` list**, not from scanning the drawn text:

- each node label, all its lines joined by a space;
- each edge label;
- each participant name, message, note and frame title.

`search::find` and the highlight painter work unchanged, and a match lights up on the label where it is drawn.

**Other flows:**

- Outlines and transcripts are ordinary prose flows.
- The diagram's title, as shown in the top rule, is a flow at the column where it is drawn. A title cut with `…` is found up to the cut. The rule line's word count is the shown title's word count.
- The source fallback keeps today's code flows.
- No text is ever both a drawn label and a source line, so there are no double matches.

**Reading time.** Each row's `words` counts the label words on it, so diagram labels count toward reading time.

### 8.5 Raw view, print mode, themes and no color

| Mode | Behavior |
|---|---|
| raw view (`r`) | unchanged: `layout_raw` shows the fence and its source |
| print, pipes, the `less -F` path | the same pure layout, so the same drawing; a diagram that does not fit prints its outline, so piped output never loses information |
| `--color never`, `NO_COLOR` | the same characters, no color; the structure lives in the glyphs |
| `--diagrams ascii` (M3) | ASCII glyphs, rules and titles (section 6.3) |
| `--diagrams source` | today's code box and today's `Code` target |

**Diagram lines need more contrast than code-box borders.**

- `p.border` is 0xC4CBD3 in the light theme: 1.64:1 on white.
- In the dark theme it is 0x4B5263: 1.79:1 on One Dark's 0x282C34 and 2.69:1 on black [M11].
- That is fine for a decorative code-box border. In a diagram, though, the lines carry the meaning.
- WCAG 1.4.11 asks 3:1 for "Parts of graphics required to understand the content" [V31].

**New palette entries**, with contrast against white (light) and 0x282C34 (dark) [M11]:

| Entry | Light | Dark | 256-color (xterm default values) |
|---|---|---|---|
| `diagram_line` (borders, frames) | 0x57606A, 6.39:1 | 0x9DA5B4, 5.65:1 (8.47:1 on black) | 59 (#5F5F5F) 6.39:1; 145 (#AFAFAF) 6.38:1 |
| `diagram_edge` (edges) | 0x6E7781 (today's muted), 4.55:1 | 0x7F848E (today's muted), 3.73:1 (5.59:1 on black) | 243 (#767676) 4.54:1; 102 (#878787) 3.90:1 |

The terminal's real background and 256-color palette are unknown. 0x282C34 is an assumption [U].

**Roles and theme styles.** Under `Depth::None` all colors disappear and italic remains (`theme.rs:7`).

| Role | Style |
|---|---|
| `Border` | `fg(p.diagram_line)` |
| `NodeText` | `Style::new()` (terminal foreground) |
| `Edge` | `fg(p.diagram_edge)` |
| `Arrow` | `Style::new()` |
| `EdgeLabel` | `fg(p.muted)` + italic |
| `Frame` | `fg(p.diagram_line)` |
| `FrameTitle` | `code_label` (muted italic) |
| `Note` | `fg(p.muted)` + italic (built as the `diagram_note` style, also used for the "Not drawn" line and bottom-rule notes) |

**Tests.** The tests never see styles today. `render()` always uses the dark theme (`tests/render.rs:16`), and the "80 color" snapshots are `to_plain` text. Section 10 adds style snapshots, and section 11 adds a manual check on a light-background terminal before M2 is frozen. `PLAN.md:259` already notes the light theme was never checked on a real Mac.

### 8.6 OSC 8

- Diagram rows carry no `Seg::link`, so the hyperlink re-print after each frame (`write_hyperlinks`, `pager.rs:1498`) never touches them.
- Mermaid `click` URLs are not made into links in v1. That keeps folio's rule that a document must not make the reader launch programs (`PLAN.md:134-136`).

### 8.7 Caching

`Diagrams` works like `Highlighter`: it is passed `&mut`, lives as long as the pager, and keeps:

- **Prepared IR** (parsed graph, ranks, order, lanes), keyed by source hash. It is passed to every `Engine::draw` (section 4.4), so a source is parsed once.
- **Drawings keyed by (hash, step, charset)**, and outlines keyed by (hash, width).
  - No step depends on the terminal width, so resizing only re-selects among cached drawings and never runs layout again.
  - Each step is rendered at most once per diagram.
- **Errors,** cached too.
- **The `s` toggles** (8.2). These are settings, not cache entries, and are never evicted.
- **Eviction.**
  - Every entry carries the number of the last layout that used it.
  - When the cache passes its cap (1,024 entries [U]), entries are evicted oldest-use first. An entry used by the current layout is never evicted.
  - A document with many diagrams therefore never thrashes. Revision 1 cleared the whole cache at 256 entries, which a document of about 40 diagrams would overflow in one layout.
- **Hashing.** In-memory keys use `DefaultHasher`, as `highlight.rs:36` already does. Each entry also stores its source, which is compared on a hit, so a hash collision cannot show the wrong diagram.

---

## 9. Images (later, opt-in, outside these milestones)

### Conditions for adding images

- **Strictly additive and opt-in.** The text drawing stays the placeholder and the fallback. `Outcome` gains an `Image` variant.
- **In the document view,** use only Kitty Unicode placeholders. They are ordinary cells that scroll like text.
  - The placeholder is U+10EEEE.
  - The image ID is carried in the foreground color: 24-bit IDs in truecolor, 8-bit IDs in 256 colors.
  - Kitty added placeholders in version 0.28.0 [V24].
  - U+10EEEE is East Asian Ambiguous [V8], so terminals that draw Ambiguous characters wide would break them.
- **No terminal probe at startup or in the picker.** Never block startup.
- **Never change the user's tmux configuration.**
  - tmux added `allow-passthrough` in 3.3, off by default, and the value `all` in 3.4. Sixel support exists only when tmux is built with `--enable-sixel` [V25].
  - ratatui-image 11.1.0 runs `tmux set -p allow-passthrough on` by itself when it detects tmux [V26]. folio must not use that code path.
  - Its default features include `chafa-dyn` [V26].
- **Legibility thresholds in device pixels,** not CSS pixels. For example, refuse an image whose label text would be under about 10 device pixels tall [U: threshold to be tested].
- **Known risk:** a user report against Ghostty 1.3.1 (the latest tag, v1.3.1 [V27]) says Kitty placeholder images inside tmux are drawn at a fraction of their width, and that the development build fixes it. The issue (#13056) was closed automatically because only maintainers may open issues, so the bug is not confirmed [V27].
- **Background work.** Images would need real background work (decoding, `mmdc`). That job system is not part of this plan and has no days here. It belongs to the image phase.

### `mmdc`

PLAN.md now says `mmdc` is used only when installed **and enabled** (decision 4), not just when installed.

- mermaid-cli 12.0.0 (2026-09-24, MIT) needs Node.js ≥ 22.13.0 and has Puppeteer ^25 as a peer dependency [V7].
- If enabled, it would run in the image phase's background jobs, with a timeout and a disk cache.

---

## 10. Testing

1. **Parser unit tests** (about 100):
   - every statement form in sections 3.3 and 3.4;
   - the documented traps: lowercase `end`, `A---oB`, `#59;`, `o` and `x` node names [V1, V2];
   - last label wins; `flowchart-elk`;
   - MyST `:key: value` option lines and Sphinx front-matter keys;
   - every degenerate case in section 3.6;
   - edge text without spaces;
   - participant names with spaces;
   - entity codes, `<br>` and Markdown strings;
   - presentational lines that must be ignored.
2. **Fixture parse test.** The 156 unique inputs of mermaid-ascii's fixtures (96 graph, 60 sequence; MIT, with the notice kept) [V21] must each parse or fall back for a stated reason, and never panic.
3. **Layout unit tests:**
   - ranks, chains and label ranks;
   - crossing counts before and after the sweeps;
   - lane selection, including merging arrivals and the iterative rejection;
   - **port rule:** the reviewer's `A-->C; A-->D; B-->D; B-->E` gives D two ports; a long edge from a fan-out keeps its own port through its dummies;
   - **crossing rule:** `A-->C; B-->D; A-->E; B-->F; C-->F; D-->E` draws one crossing with no junction glyph;
   - track assignment: a hard-constraint case, a minimal-tracks case and a hard-conflict case that must fail the step;
   - the glyph table;
   - mirroring;
   - the ladder decisions.
4. **Property tests** with no new dependency.
   - A small xorshift generator makes, with fixed seeds:
     - 2,000 random flowcharts (up to 30 nodes, with labels that include CJK and emoji);
     - 500 random sequence diagrams, many with messages that skip lifelines.
   - At every width from 20 to 140, check that:
     - every row fits;
     - every label appears exactly once;
     - every visible edge has its head next to its target;
     - **the connectivity trace finds no false connection, and the cell check finds no bad sharing**: the reachable targets from every source port equal the edge set;
     - **no message forms a junction with a lifeline between its ends;**
     - no line cell touches text, except the documented cases (section 6.4);
     - rendering twice gives identical output.
5. **Snapshot fixture `tests/fixtures/diagrams.md`.** Adding it to `FIXTURES` gives the 40/80/120 plain and 80 color snapshots, plus the width check at 20–140 at both color depths (`tests/render.rs:20-49`). It covers:
   - mock-ups 1 and 2;
   - the two routing examples in section 6.2 and the skipped-lifeline sequence;
   - an LR flowchart that must turn or narrow at 40;
   - a sequence diagram with a note, an activation and `autonumber`;
   - a subgraph diagram (outline with membership until M3b);
   - `style`, `classDef`, `class`, `linkStyle` and `click` lines (no stray nodes);
   - front matter, `%%{init}%%` and MyST option lines;
   - `~~~mermaid` and `{mermaid}` fences;
   - a diagram inside a list item and inside a quote (`nest()` shifting);
   - **deep `alt`/`loop` nesting (6 levels) inside a quote inside a list, and a long front-matter title inside a quote**, so the 20–140 width check covers narrow nested widths;
   - CJK and emoji labels;
   - a label containing `\u{1b}`;
   - `pie` (falls back to source);
   - a syntax error;
   - an unclosed fence;
   - the degenerate cases.
6. **Style snapshots.** Snapshot `ansi::render` output (`ansi.rs:12`) for one flowchart and one sequence diagram, in the light and dark themes, in truecolor and 256 colors. The escape codes are then visible in review.
7. **Engine stub tests:**
   - ladder selection, frame, notes and rule truncation;
   - label flows: `search::find` lands on the drawn column;
   - target order and spans: extend `links_and_code_blocks_become_focus_targets`;
   - focus kept across a relayout;
   - `s` toggles keyed and kept as in 8.2 across a simulated reload;
   - `--diagrams source` produces `TargetKind::Code`.
8. **Corpus gate (local, never committed).**
   - Run the Rust renderer over both samples at M2, M3, M3b and M4, using the definition of "drawn" in 7.4.
   - Record:
     - parse rates and steps used;
     - crash count;
     - defect counts of fitting drawings by check;
     - the subgraph subset as its own row;
     - p50/p99 times;
     - work-limit hits.
   - Gates use the held-out sample only.
   - The samples are third-party content of unknown license, so they stay outside the repository.
9. **Visual review.** Render 50 random held-out diagrams side by side with merman-ascii 0.9.2 and the mmdflux CLI. Include LR drawings with 8-column labels (decision 10) and drawings with crossings (risk 13). This is a reference only; neither tool is shipped.
10. **Terminal checks** (tests playbook):
    - tmux: `Tab` to a diagram, `Enter` into the viewer, pan, `s`, `y`, `c`; open and close the contents pane over a diagram; resize across the turn-to-TD threshold; a document with 30+ diagrams (idle drawing); 256 colors, `NO_COLOR`, `--diagrams ascii`.
    - A light-background terminal, before M2 is frozen.
    - A glyph test page (box drawing, `▸◂▴▾`, `┄┆`, `╱╲`) on Terminal.app, iTerm2 and Ghostty before decision 6. This needs a Mac (Q1).

---

## 11. Milestones

Working days [U]. The developer has not been named; the owner should name one (decision 14).

Revision 1 estimated 35 days (planning for 42). That sat at the bottom of the reviewers' range for the text-first plan, and it left out the correctness rules, the work limits, the idle drawing step and a separately gated subgraph milestone. This estimate includes them.

| Milestone | Contents | What readers get | Days [U] |
|---|---|---|---|
| **M0 Plumbing** (built) | `BlockKind::Diagram`, fence detection with MyST options, labeled source box with the note line, `TargetKind::Diagram`, `y`, `--diagrams`, `Pager::relayout()`, focus kept across relayouts, fixture; also `s` with its state and `c`, moved here from M5 | diagrams are labeled and copyable; focus survives resize and reload (closes a Phase 2 gap) | 3–4 |
| **M1 Parsers and outlines** (built) | preamble, label decoding (with breaks between wide characters), flowchart and sequence parsers, degenerate cases, outline (with subgraph membership) and transcript, source reasons, search flows, engine seam and stub, fixture parse test, corpus harness with the frozen held-out sample | every supported diagram shows a readable outline or transcript that always fits | 6–7 |
| **M2 Flowchart core** (built) | port of the prototype (about 2,000 lines of Python): cycles, ranks, chains, order, lanes, **port rule**, placement, router with hard-conflict detection, **crossing rule**, self-loops including LR, canvas with owners, **checks**, shapes, roles and the new colors, label flows, TD and LR, all ladder steps (order per decision 10), size caps and work counter, cache with eviction and prepared IR, BT/RL drawn with a note, style snapshots, light-terminal check | flowcharts without subgraphs are drawn, correctly | 13–16 |
| **M2 checkpoint** (gates 1 to 4 pass) | corpus gate with the kill criteria below | — | (in M2) |
| **M3 Flowchart hardening** | segment splitting, BT/RL mirroring, ASCII charset (including rules and titles), document cold-start budget and idle drawing, property tests, benchmark gate | `--diagrams ascii`; fast start on documents with many diagrams | 7–9 |
| **M3b Subgraph frames** | frames (section 6.2 N), titles, edges across frames, frame checks, own gate below | subgraph diagrams drawn | 6–9, plus up to 5 for plan B |
| **M4 Sequence** | column solver, rows, notes, frames and messages in front of lifelines, activations, `autonumber`, ladder, transcript, checks, tests | sequence diagrams drawn | 8–9 |
| **M5 Viewer and keys** | bounded viewer, `Enter` opens it, the height guard (decision 8), the viewer's footer and help, README and PLAN, tmux runs (`s` and `c` moved to M0) | full-size drawings | 5–6 |
| **Total M0–M5 with M3b** | | | **48–60; plan for 60 (65 if plan B is needed)** |
| M7 Wider than the reading width | x-offset for diagrams and tables (section 7.6) | more diagrams drawn on wide terminals | 4 |
| M6 State diagrams | section 3.5 (composite states need M3b) | state diagrams drawn | 4–5 |
| Later | class diagrams (compartments, UML heads); subgraph `direction` by recursive layout | | 5 + 2 |

**Status on 2026-10-28.** M0, M1 and M2 are built, on the local `mermaid` branch and not yet committed. Two rounds of review and fixes followed (see "Implementation status" at the top). From M3, the work limit is set (6.6). Some items in the M0 to M2 lists are not done:

- M1: the engine seam and stub engine (4.4), and the fixture parse test of mermaid-ascii's inputs (10.2).
- M2: the style snapshots (10.6); a unit test checks each role's theme style instead. The light-terminal check has not been recorded.
- M2 checkpoint: gate 5, the 50-diagram review.

The other milestones are not started.

**What ships if later milestones slip.**

- M0 and M1 stand alone: labeled, copyable source boxes, then outlines and transcripts that always fit.
- M2, M4 and M5 do not depend on M3 or M3b. They can ship in the order M0, M1, M2, M4, M5, with M3 and M3b after.
- In that case readers get:
  - drawn flowcharts without subgraphs;
  - subgraph flowcharts as outlines that list their groups;
  - BT/RL drawn top-down or left to right with a note;
  - sequence diagrams and the viewer.
- They do not get ASCII mode or the cold-start budget. A document with many diagrams starts more slowly, but the per-diagram work limit from M2 still bounds each diagram.

### M2 checkpoint: kill criteria

These are measured on the **held-out** sample, with the definition of "drawn" in 7.4. The brackets give the prototype's result and folio's after M2 and both review rounds (2026-10-28, Rust release build) [M12]. The owner confirmed these thresholds before the Rust measurement (decision 15).

1. ≥ 95% of flowcharts parse [prototype 94.9%; folio 95.4%, 5,371 of 5,632; see Q3 on `C#`/`C++` ids].
2. At 78 columns, ≥ 88% of parsed flowcharts without subgraphs are drawn [prototype 92.9%; folio 94.9%, 4,402 of 4,638], and ≥ 75% of all parsed flowcharts are drawn with subgraphs sent to the outline [prototype 80.4%; folio 82.0%].
3. Correctness:
   - no drawing that folio shows fails a check (enforced by construction);
   - fewer than 0.5% of drawings that fit fail a check [prototype 0.2%, first fitting step only; folio 0.1%, 11 of 18,504, every fitting step];
   - no crashes [prototype: 9 on the held-out sample; folio: 0];
   - identical output when rendered twice [folio: yes].
4. p99 ≤ 20 ms for all ladder steps in a release build [Python, single process: 132 ms; folio: 6.0 ms, with p50 1.4 ms and max 23.5 ms].
5. In the 50-diagram review, folio's drawing is at least as readable as merman-ascii's or mmdflux's in at least 40 of the diagrams that those tools draw [not done yet: it needs a person and the other tools].

Gates 1 to 4 pass; gate 5 is still to do. If gate 2 or 5 fails:

1. Spend up to one week replacing steps C to I with ascii-dag (plan B) behind the engine seam (4.4; not built yet), keeping folio's router, painter and checks, and measure again.
2. If it still fails, the owner chooses between outlines for complex diagrams and an optional merman cargo feature. That feature would need the size and license checks in section 5, and folio's checks would still apply to its output.

**M3b gate (subgraph frames):**

- at 78 columns, ≥ 55% of held-out subgraph flowcharts are drawn with frames [U: threshold; the no-frames upper bound is 71.3%];
- every frame contains exactly its members;
- the connectivity check passes.

If it fails, evaluate ascii-dag's cluster layout for up to 5 days. Otherwise subgraph diagrams keep the outline, which loses nothing.

**M4 gate (sequence, held-out):** ≥ 95% parse [prototype 97.4%; folio's parser 98.6% [M12]]; ≥ 72% drawn at 78 columns [78.6%]; no message junctions; no crashes.

---

## 12. Risks and open questions

### Risks

| # | Risk | Mitigation |
|---|---|---|
| 1 | Subgraph frames were not prototyped. They affect 20.4% (tuning) and 13.4% (held-out) of parsed flowcharts [M7]. Frames add width and constrain ordering. | Until M3b, these diagrams get an outline that lists every group, so nothing is lost. M3b has its own gate and plan B (ascii-dag clusters, section 5.2). The subset is reported as its own row. |
| 2 | Placement quality in Rust differs from the prototype. | Port the prototype's test diagrams first and compare output. Property tests and the M2 checkpoint. Done for M2: folio's parse results (on both samples) and drawings match the prototype's, apart from deliberate changes and width-table differences [M13], and the M2 checkpoint's gates 1 to 4 pass. The review rounds added more deliberate changes (see "Implementation status"). |
| 3 | Wrapping to 8 makes tall drawings: 76×47 for the 6-participant example. | Decision 8: a height guard, built with the viewer (M5). |
| 4 | Narrow sequence steps let frame titles cover a lifeline row. | Documented; titles are never cut. |
| 5 | Terminals that draw Ambiguous characters wide (CJK locales) misalign boxes. Greek and Cyrillic labels are Ambiguous too [V8]. | `--diagrams ascii` (M3) removes every Ambiguous glyph folio adds, including rules and titles. For labels and the rest of folio, decision 13 adds no option for now. |
| 6 | Fonts and the Linux console lack some glyphs [U]. | ASCII charset (M3). Decision 9 keeps one rule for all of folio's box drawing: nothing changes under `TERM=linux`. |
| 7 | The `╱ ╲` decision corners do not join `─` cleanly in some fonts [U]. | Decision 6 kept `╱ ╲` before the glyph test page. The test page on Mac terminals (Q1) is still to do. |
| 8 | The Mermaid grammar grows: 12.2.0 was released 2026-10-13 [V6]. | Strict subset; unknown syntax falls back to source on its own. |
| 9 | folio draws something Mermaid rejects, or the reverse. | Mirror the documented traps; the fallback loses nothing; track source rates on both samples (Q3). |
| 10 | Pathological diagrams are slow: 10.3 s in Python for the slowest, with the cost driven by the widest rank [M8]. | Size caps including the widest rank and element count, the work counter, the document budget with idle drawing, the bounded viewer and the M3 benchmark gate. No background threads are needed. After the review rounds, folio's p99 for all ladder steps of one diagram is 6.0 ms, and the work limit stops a diagram after roughly 17 to 33 ms [M12]. |
| 11 | Snapshot churn while tuning. | Deterministic algorithms; tune on the corpus before committing snapshots. |
| 12 | About 5,500 lines to maintain, including tests [U]. After the review rounds, `src/diagram` alone is about 9,300 lines, its unit tests included. | Small modules with a role-only API. The engine seam (4.4, not built yet) would allow a different backend later. |
| 13 | The crossing rule's half-cell gap may be too faint in some fonts [U]. | Visual review on several terminals. Fallback: leave one full blank cell on each side of a crossing (costs width at crossings; 8.9% of fitting held-out drawings have one [M7]). |
| 14 | The held-out sample differs from the tuning sample (more LR, README, ADR and selected-repo content). This revision looked at its aggregate counts. | Gates use it only. Do not tune on its individual failures; if that happens, collect a fresh sample before the gate. |
| 15 | The new colors assume a typical background; real terminal palettes vary [U]. | 3:1 or better on white, One Dark and black [M11]; manual light-terminal check; style snapshots. |

### Decisions for the owner

Taken on 2026-10-28. Where the plan recommended a choice, that choice was taken. Decisions 7, 12 and 14 are not needed yet and stay open.

1. **Frame:** the open frame (recommended; gives the full width) or the same rounded box as code blocks (4 columns narrower)? **Decided: the open frame.**
2. **Too wide in the pager:** outline or transcript plus `Enter` (recommended), or a clipped drawing with `›` marks? Sequence diagrams are never clipped either way. **Decided: outline or transcript, never clipped.** The viewer comes in M5; until then `Enter` on a diagram copies its source, as on a code block.
3. **Fence names:** `mermaid` (what GitHub documents [V22]) and MyST's `{mermaid}` [V23] (recommended), and also `mmd`? **Decided: `mermaid` and `{mermaid}`, in any case; not `mmd`.**
4. **PLAN.md wording:**
   - change "`mmdc` when installed" to "when installed and enabled";
   - change `PLAN.md:64` ("Slow work (images, diagrams) will run in background jobs in Phase 3") so that diagrams use bounded work and idle drawing instead, and background jobs belong to images only.

   **Decided: both changes, as recommended.** `PLAN.md:64` is reworded: diagrams are drawn during layout without background threads, each with a work limit, and the budget for documents with many diagrams comes later. PLAN.md's Phase 3 line now says `mmdc` is used only when installed and enabled.
5. **Wider than the reading width:** do M7 after M5 (recommended; data in section 7.6), or leave it to Phase 4? **Decided: M7 later, after M5.**
6. **Decision shape:** `╱───╲ <  > ╲───╱` as in the mock-ups, or `╭───╮ <  > ╰───╯`, which avoids the diagonals? Decide after the glyph test page (Q1). **Decided: `╱───╲ <  > ╲───╱`, as in the mock-ups.** The glyph test page (Q1) has not been done yet.
7. **After sequence diagrams:** state (cheaper; reuses the flowchart engine; 3.8%) or class (4.7% in this sample [M3])? **Open; not needed until after M4.**
8. **Height guard:** prefer the transcript when a label-wrapped drawing is more than about 3 times as tall as the transcript? [U: threshold] **Decided: not built yet; it belongs with the viewer (M5).**
9. **`TERM=linux`:** switch all of folio's box drawing to ASCII (code boxes, tables, rules and diagrams), or none of it? Switching diagrams alone would leave code boxes (`╭`) and tables (`╞═╡`) as they are today. Whether the console's fonts have these glyphs is unverified (Q2). **Decided: none of it; `TERM=linux` changes nothing.**
10. **Ladder order for LR diagrams:** A (turn first, as revision 1), B, or C (recommended; section 7.1)? **Decided: C.** A left-to-right diagram tries normal, compact, LR labels 16 and LR labels 8, then turned, turned compact, labels 16 and labels 8.
11. **Contents pane over diagrams:** accept that opening the pane at 80 columns turns many diagrams into outlines (recommended; instant, place kept), or keep drawings by showing the pane as an overlay while a drawn diagram is on screen, or use decision 2's clipped mode? **Decided: accept the outlines.**
12. **Frames in sequence diagrams:** draw frame borders in front of lifelines, as messages are (recommended and measured), or keep `┼` where a frame crosses a lifeline as a special case of the "one bus" rule? **Open; needed for M4.**
13. **Ambiguous width:** add a folio-wide option (for example `--ambiguous wide`) that measures East Asian Ambiguous characters as 2 columns with `width_cjk()` [V10], for terminals in CJK locales? It would affect tables, code boxes and labels, not just diagrams. **Decided: no option.**
14. **Who builds it,** and is a 48–60 day range (plan for 60) acceptable, with the slip order in section 11? **Open; not needed now.**
15. **Gate thresholds:** confirm the M2, M3b and M4 thresholds in section 11 before the Rust port is measured. **Decided: the thresholds as written.**

### Open questions (to check, not to decide)

- **Q1.** Do Terminal.app, iTerm2 and Ghostty draw `▸◂▴▾`, `┄┆`, `╱╲` and the crossing gap well? This needs a glyph test page on a Mac before decision 6.
- **Q2.** Which of folio's glyphs does the Linux console's default font have? Unverified; needed for decision 9.
- **Q3.** Does Mermaid accept node ids containing `#` or `++` (`C#[C#]`, `C++[C++]`)? 158 held-out flowcharts fall back to source because of them [M7]. If Mermaid accepts them, the parser should too; if not, the fallback is correct and the gate should count them as such.

---

## Appendix A: how the reviewers' concerns were resolved

### First review

| Concern | Resolution |
|---|---|
| merman-ascii refuses many real flowcharts | Confirmed on a larger sample: 2,286 of 12,705 (18.0%) [M4]. Mock-up 1 and a plain-rectangle retry loop are refused [M6]. Not used. |
| merman's Compact did not reduce top-down height | Confirmed: a 7-node chain was 51 rows in both profiles [M6]. Relevant only to the rejected option. |
| "Most diagrams fit at 80+ columns" | Wrong. The median natural width is 90 (flowcharts) and 91 (sequence diagrams) [M2]. Replaced by measured rates (section 7.4). |
| Mock-ups painted from hand-placed geometry | Replaced: every mock-up here is prototype output [M9]. |
| merman-ascii's `▲ ▼` are Ambiguous | Confirmed [V8]. folio uses `▸ ▾ ◂ ▴` (Neutral). |
| A merman build would fail a 3 MB size gate | Confirmed: +6.2 MB with the default release profile and +4.4 MB with fat LTO and `opt-level = "s"` [M5]. folio's release binary is 5,995,520 bytes [M5]. |
| mmdflux and mermaid-text as fallbacks | Confirmed: no released fitted API in mmdflux 2.6.1 [V18]; no sequence compaction in mermaid-text [V19]. |
| mermaid-text "would break the width rule" | Corrected: folio measures every row again, so it would only limit what fits (section 5.2). |
| MPL-2.0 dependencies of merman | Confirmed: cssparser 0.38 and selectors 0.37 are MPL-2.0; lol_html 3.0.2 is BSD-3-Clause [V16, V17]. Moot, since no crate is added. |
| Ghostty Kitty placeholders broken in tmux | The report exists, but it was auto-closed and is unconfirmed by maintainers [V27]. Listed as a risk for a later image phase only. |
| CSS-pixel legibility threshold | Agreed. A later image phase must use device pixels (section 9). |
| Terminal-specific image claims (Sixel erase, Windows Terminal, Konsole, WezTerm) | Not used by this plan; left unverified. |
| 22–27 days | Revised to 48–60 (section 11). |
| "5 call sites" for `layout()` | Corrected to 6 (section 8.1). |
| folio code references | Re-checked: `pager.rs:345`, `main.rs:133`, `text.rs:250-257`, `layout.rs:305-316`, `pager.rs:371`; `s`, `c` and `Y` are unbound (`pager.rs:853-907`). |

### Second review

| # | Issue | Where handled |
|---|---|---|
| 1 | Shared ports and `┼` crossings create false connections | Goal 2; port rule (6.2 H); crossing rule (6.2 L, 6.4); cell owners and checks (6.3); property tests (10.4); kill criterion 3 (11). Measured before and after (6.3). |
| 2 | Headline numbers measured fit, not correctness; no held-out set | "Drawn" redefined (7.4); checks at every step; hard conflicts fail the step (6.2 K); LR self-loops (6.2 M) and BT/RL notes (6.1); held-out sample; numbers restated (1, 7.4, 11). |
| 3 | `debug_assert!` in `Rendered::push` would always fail | Moved to `Layouter::diagram` (8.1). |
| 4 | Subgraphs shipped without frames | Outline with membership until M3b (3.3, 6.5); subset rows (7.4); M3b gate; ascii-dag clusters as plan B (5.2). |
| 5 | No document time budget; job system does not exist | Widest-rank and element caps, work counter, document budget with idle drawing, bounded viewer, no job system (6.6, 7.5); decision 4. |
| 6 | Diagram lines in the faintest colors | New palette entries with measured contrast (8.5); style snapshots (10.6); light-terminal check (10.10). |
| 7 | Estimate too low; work not counted | 48–60 days with the missing items; M3b separate; slip order (11); developer to be named (decision 14). |
| 8 | Ladder rotates LR diagrams by default | Ladders A, B and C measured (7.1); decision 10. |
| 9 | `s` state and relayout unspecified | Toggle key and reload behavior (8.2); `Pager::relayout()` (8.3); `--diagrams source` keeps `Code` targets (8.2). |
| 10 | Contents pane not in the width tables | 51-column rows (7.2, 7.3, 7.4); decision 11. |
| 11 | Cache thrashing; engine reparses | Use-based eviction; prepared IR passed to `draw` (4.4, 8.7). |
| 12 | Narrow nested widths, rule truncation, note line | Indent cap (6.5); truncation and note line (8.2); fixtures (10.5). |
| 13 | Degenerate diagrams | Table in 3.6; tests (10.1, 10.5). |
| 14 | `flowchart-elk`, last label, MyST options | 3.2, 3.3 with quotes [V1, V23, V30]. |
| 15 | mermaid-ascii fixture duplicates | 156 unique inputs (5.2, 10.2) [V21]. |
| 16 | ASCII mode gaps; CJK wrapping; `TERM=linux`; Mac glyphs | 6.3; CJK breaks measured (6.2 G); decisions 9 and 13; Q1 and Q2. |

---

## Appendix B: measurements

Measurements M1 to M11 were made on 2026-10-23, and M12 and M13 on 2026-10-28, on the development machine (Python 3.12 in WSL Ubuntu, and Rust on Windows). The prototype and the measurement scripts are in [`docs/mermaid-prototype/`](mermaid-prototype/README.md), and the paths below are relative to that folder. The GitHub samples, the per-diagram results (`results.jsonl`) and the downloaded verification files are kept outside the repository, because the samples are other people's content.

- **[M1] First prototype and examples.**
  - `proto/proto.py` (about 1,900 lines). Run: `python3 -I proto/proto.py 80 < proto/mock/flow1.mmd`.
  - Inputs are in `proto/mock/` and `proto/t/`.
- **[M2] First prototype corpus run.**
  - `proto/corpus.py` over the tuning sample.
  - It counted fit only.
- **[M3] Type and feature counts.** `proto/types.py` over the tuning blocks.
- **[M4] merman-ascii 0.9.2 corpus run.**
  - `merman-probe/src/bin/corpus.rs`, on the tuning files.
  - Totals: 12,705 flowcharts, 2,286 refused; 2,221 sequence diagrams, 59 refused. p50 2.9 ms and p99 22 ms per render.
- **[M5] Binary sizes** (stripped Windows release builds; the binaries were not kept):
  - a small program using merman-ascii: 6,421,504 bytes, against 219,648 for the same program without it;
  - with fat LTO and `opt-level = "s"`: 4,559,360 against 156,160;
  - folio's release binary: 5,995,520.
- **[M6] merman-ascii sample outputs.** Produced with `merman-probe`; the output files were not kept.
- **[M7] Checked prototype corpus run.**
  - `proto2/make_proto2.py` builds `proto2/proto.py` from `proto/proto.py`. It adds cell owners, the port rule, the crossing rule, hard-conflict counting, an LR self-loop "dropped" flag and CJK breaks.
  - `proto2/check.py` holds the text-over-line, cell and connectivity checks and the sequence junction check. `proto2/ladder.py` holds the ladders A, B and C.
  - Run: `proto2/corpus2.py`, as shown in the prototype's README; 8 processes, 380 s.
  - Tuning: 17,450 unique blocks. Held-out: 6,496 unique blocks not in the tuning set.
  - Every flowchart was drawn at every step in both the original and the fixed mode; CJK cases were drawn again with CJK breaks.
  - Tables: `proto2/analyze2.py` → `proto2/analysis.txt`.
- **[M8] Single-process timing and sizes.**
  - `proto2/timing2.py` → `proto2/timing.txt`.
  - It covers 1,500 random parsed flowcharts (seed 7), fixed mode, ladder A, timing parse, preparation and drawing; the 30 slowest diagrams re-timed; and widest-rank and element counts for all 17,836 parsed flowcharts.
- **[M9] Mock-ups and examples with the fixes.**
  - `proto2/mock2.py` → `proto2/mockups3.txt`.
  - `proto2/table2.py` → `proto2/table2.txt` (the section 7.3 table).
  - `proto2/demo.py` → `proto2/demo.txt` (the reviewer's examples, before and after; inputs in `review/`).
- **[M10] Diagrams per file.** `proto2/perfile.py` → `proto2/perfile.txt`.
  - README sample: 9,043 files, p50 1, p90 3, p99 9, max 92.
  - All six files: 28,871 files, p50 1, p90 4, p99 12, max 101.
- **[M11] Contrast ratios.** `proto2/contrast.py` → `proto2/contrast.txt`. WCAG 2 relative luminance. The 256-color values follow folio's `nearest_256` (`theme.rs:201-221`) with xterm default palette values.
- **[M12] folio's corpus gate (M2).**
  - `proto2/gate_blocks.py` writes the blocks of a sample to a JSONL file, outside the repository. folio's `examples/mermaid_gate.rs` runs folio's Mermaid code over them and prints the section 11 numbers: parse rates, drawn shares at 38, 51, 78, 100 and 118 columns, the steps used at 78, check failures, crashes, the twice-rendered comparison and timings.
  - Run: `cargo run --release --example mermaid_gate -- BLOCKS.jsonl held` (or `tune`).
  - Held-out sample: the same 6,496 blocks as [M7].
  - It also counts the diagrams over the work limit or the size caps after preparation.
  - The numbers in this plan come from the run after the second review round, with `WORK_LIMIT` at 500,000.
- **[M13] Differential tests against the prototype.**
  - Parsers and outlines: `proto2/m1_dump.py` and folio's `examples/mermaid_parse_diff.rs`. Every parse result matched on all 23,946 blocks of both samples, except for deliberate changes and differences in the width tables.
  - Drawings: `proto2/m2_dump.py` and `examples/mermaid_draw_diff.rs`. Before the changes for edges between the same two nodes (section 6.2 H), 83,350 of 85,228 ladder steps were identical. All 1,878 others were in diagrams whose labels Python's `unicodedata` and the `unicode-width` crate measure differently (emoji with VS16, Arabic lam-alef, Indic marks). With the prototype's width table swapped in, differences remained only in 31 diagrams, where the per-character tables disagree.
  - After those changes, the 14,211 diagrams with none of the affected features still match the prototype exactly. `examples/mermaid_edge_groups.rs` counts the flowcharts with edges that share a drawn line (edges between the same two nodes, two-headed lines, self-loops, lanes) and how their drawings at 78 columns turned out.
  - The second review round changed more parse results and drawings on purpose ("Implementation status" lists the cases). The comparisons were made before that round and were not run again after it.
- **Downloaded verification files** are kept with the samples, outside the repository. The East Asian Width lookups in sections 6.3 and 9 use the EastAsianWidth.txt from source 8.

---

## Sources (checked 2026-10-23)

1. Mermaid flowchart syntax, docs source on `develop` (includes "The `flowchart-elk` diagram type still works, but is no longer needed", "it is the last text found for the node that will be used", and the subgraph `direction` rule): https://github.com/mermaid-js/mermaid/blob/develop/packages/mermaid/src/docs/syntax/flowchart.md
2. Mermaid sequence-diagram syntax: https://github.com/mermaid-js/mermaid/blob/develop/packages/mermaid/src/docs/syntax/sequenceDiagram.md
3. Mermaid state-diagram syntax: https://github.com/mermaid-js/mermaid/blob/develop/packages/mermaid/src/docs/syntax/stateDiagram.md
4. Mermaid syntax reference (front matter "MUST be the only character on the first line"; directives; comments): https://github.com/mermaid-js/mermaid/blob/develop/packages/mermaid/src/docs/intro/syntax-reference.md
5. Mermaid directives ("deprecated from v10.5.0"): https://github.com/mermaid-js/mermaid/blob/develop/packages/mermaid/src/docs/config/directives.md
6. npm registry, mermaid (latest 12.2.0 on 2026-10-13; 12.0.0 on 2026-09-10): https://registry.npmjs.org/mermaid
7. npm registry, @mermaid-js/mermaid-cli (12.0.0 on 2026-09-24; MIT; Node ≥ 22.13.0; Puppeteer ^25.0.0 as a peer dependency): https://registry.npmjs.org/@mermaid-js/mermaid-cli
8. Unicode EastAsianWidth-18.0.0 (U+00B6..00B7 A; U+0391..03A1, 03A3..03A9, 03B1..03C1, 03C3..03C9 A; U+0401, 0410..044F, 0451 A; U+2500..254B A; U+2550..2573 A; U+25B4..25B5, 25B8..25BB, 25BE..25BF, 25C2..25C5 N): https://www.unicode.org/Public/UCD/latest/ucd/EastAsianWidth.txt
9. Unicode emoji-variation-sequences: https://www.unicode.org/Public/UCD/latest/ucd/emoji/emoji-variation-sequences.txt
10. unicode-width 0.2.2, `UnicodeWidthStr` (`width`, `width_cjk`): https://docs.rs/unicode-width/0.2.2/unicode_width/trait.UnicodeWidthStr.html
11. Rust `HashMap` (randomly seeded; arbitrary iteration order): https://doc.rust-lang.org/std/collections/struct.HashMap.html
12. comrak 0.57.0 `NodeCodeBlock`: https://docs.rs/comrak/0.57.0/comrak/nodes/struct.NodeCodeBlock.html
13. ELK `OrthogonalRoutingGenerator.java` (EPL-2.0 with GPL-3.0 secondary license; Sander and Di Battista references; penalties 1 and 16; topological numbering): https://github.com/eclipse-elk/elk/blob/master/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/orthogonal/OrthogonalRoutingGenerator.java
14. dagre wiki (papers per phase; `ranker` and `acyclicer` options): https://github.com/dagrejs/dagre/wiki
15. Brandes, Walter and Zink, "Erratum: Fast and Simple Horizontal Coordinate Assignment" (2020): https://arxiv.org/abs/2008.01252
16. crates.io API crate records (versions, dates, licenses, MSRV; ascii-dag 0.11.0 created 2026-09-06, 0.10.3 on 2026-08-26): https://crates.io/api/v1/crates/{merman, merman-core, merman-ascii, dugong, mmdflux, mermaid-text, ascii-dag, cssparser, selectors, lol_html, ratatui-image, unicode-width, unicode-segmentation, insta, cargo-deny, cargo-about, comrak}
17. crates.io dependency lists: https://crates.io/api/v1/crates/merman-core/0.9.2/dependencies, `/merman-ascii/0.9.2/dependencies`, `/lol_html/3.0.2/dependencies`, `/mmdflux/2.6.1/dependencies`, `/mermaid-text/0.57.0/dependencies`, `/ascii-dag/0.11.0/dependencies`
18. mmdflux 2.6.1 item list: https://docs.rs/mmdflux/2.6.1/mmdflux/all.html; README on `main`: https://github.com/kevinswiber/mmdflux
19. mermaid-text `lib.rs`: https://github.com/leboiko/markdown-reader/blob/master/crates/mermaid-text/src/lib.rs
20. ascii-dag README ("Subgraphs (clusters)"; `CustomNode` sizes; dual license): https://github.com/AshutoshMahala/ascii-dag
21. mermaid-ascii: repository record (default branch `master`, MIT, 1,612 stars, pushed 2026-10-19): https://api.github.com/repos/AlexanderGrooff/mermaid-ascii; directory listings `https://api.github.com/repos/AlexanderGrooff/mermaid-ascii/contents/{pkg/graph/testdata/ascii, pkg/graph/testdata/extended-chars, pkg/graph/testdata/multibyte, cmd/testdata/sequence, cmd/testdata/sequence-ascii}?ref=master`; fixture files read from `https://raw.githubusercontent.com/AlexanderGrooff/mermaid-ascii/master/…` and compared by input text. Repository: https://github.com/AlexanderGrooff/mermaid-ascii
22. GitHub Docs, Creating diagrams: https://docs.github.com/en/get-started/writing-on-github/working-with-advanced-formatting/creating-diagrams
23. sphinxcontrib-mermaid README (MyST `{mermaid}` fence; directive options as front matter in Markdown: name, alt, align, caption, zoom, fullscreen, config, title; `myst_fence_as_directive`): https://raw.githubusercontent.com/mgaitan/sphinxcontrib-mermaid/master/README.md
24. kitty graphics protocol, Unicode placeholders: https://sw.kovidgoyal.net/kitty/graphics-protocol/
25. tmux CHANGES (`allow-passthrough` in 3.3; `all` and Sixel in 3.4): https://raw.githubusercontent.com/tmux/tmux/master/CHANGES
26. ratatui-image `picker.rs` (runs `tmux set -p allow-passthrough on`) and `Cargo.toml` (default features): https://raw.githubusercontent.com/ratatui/ratatui-image/master/src/picker.rs, https://raw.githubusercontent.com/ratatui/ratatui-image/master/Cargo.toml
27. Ghostty tags (latest v1.3.1): https://api.github.com/repos/ghostty-org/ghostty/tags; issue #13056 and its closing comment: https://github.com/ghostty-org/ghostty/issues/13056
28. ascii-dag `docs/layout.md` ("Clusters (subgraphs)": boxes affect layout; nesting; labels capped at 40 characters): https://github.com/AshutoshMahala/ascii-dag/blob/main/docs/layout.md
29. ascii-dag 0.11.0 `LayoutIR` (`subgraphs()`: "Get all laid-out subgraph bounding boxes"): https://docs.rs/ascii-dag/0.11.0/ascii_dag/ir/struct.LayoutIR.html
30. MyST roles and directives (option block "defined by a set of lines prefixed with `:`", or enclosed in `---`): https://myst-parser.readthedocs.io/en/latest/syntax/roles-and-directives.html
31. WCAG 2.2, Understanding SC 1.4.11 Non-text Contrast (3:1 for "Parts of graphics required to understand the content"): https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html

**Folio files read:** `PLAN.md`, `Cargo.toml`, `Cargo.lock`, `src/main.rs`, `src/layout.rs`, `src/pager.rs`, `src/doc.rs`, `src/text.rs`, `src/theme.rs`, `src/highlight.rs`, `src/search.rs`, `src/ansi.rs`, `src/lib.rs` and `tests/render.rs`.
//! Text helpers shared by every diagram type: widths, wrapping, label
//! decoding, and splitting the source into a preamble and statements.

use unicode_segmentation::UnicodeSegmentation;

use super::NotDrawn;
pub use crate::text::width;

/// Labels wrap at the ladder step's width, but a word up to this many
/// columns is never broken.
const UNBROKEN_WORD: usize = 16;

/// Break a word longer than `w` columns into pieces of at most `w` columns
/// (a piece is longer only when one grapheme is wider than `w`).
pub fn break_word(word: &str, w: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cw = 0;
    for g in word.graphemes(true) {
        let gw = width(g);
        if cw + gw > w && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            cw = 0;
        }
        cur.push_str(g);
        cw += gw;
    }
    out.push(cur);
    out
}

/// Wrap `text` at `w` columns. Each `\n` starts a new line; words longer
/// than a line are broken. Always returns at least one line.
pub fn wrap(text: &str, w: usize) -> Vec<String> {
    let w = w.max(1);
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut cur = String::new();
        let mut cur_w = 0;
        for word in para.split_whitespace() {
            let ww = width(word);
            if !cur.is_empty() && cur_w + 1 + ww <= w {
                cur.push(' ');
                cur.push_str(word);
                cur_w += 1 + ww;
                continue;
            }
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            if ww <= w {
                cur = word.to_string();
                cur_w = ww;
            } else {
                let mut parts = break_word(word, w);
                cur = parts.pop().unwrap_or_default();
                cur_w = width(&cur);
                lines.extend(parts);
            }
        }
        lines.push(cur);
    }
    lines
}

/// Wrap a node or edge label at `w` columns. A word up to 16 columns is
/// never broken, except between two wide characters (CJK text has no
/// spaces to break at). A grapheme is wide only when its first character
/// is: a Devanagari or Myanmar cluster can be 2 columns because of its
/// marks, but it is still part of a word.
pub fn wrap_label(text: &str, w: usize) -> Vec<String> {
    let mut longest = 0;
    for word in text.split_whitespace() {
        let mut run = 0;
        for g in word.graphemes(true) {
            let gw = width(g);
            let first = g.chars().next().map_or(0, |c| width(c.encode_utf8(&mut [0; 4])));
            if first >= 2 {
                longest = longest.max(run).max(gw);
                run = 0;
            } else {
                run += gw;
            }
        }
        longest = longest.max(run);
    }
    wrap(text, w.max(longest.min(UNBROKEN_WORD)))
}

/// Cut user text quoted in a message to `w` columns, ending with `…`.
pub fn cut(text: &str, w: usize) -> String {
    crate::text::truncate(text, w)
}

/// User text quoted in a reason: in double quotes, cut to 30 columns, with
/// tabs as spaces and other control characters as U+FFFD.
pub fn quote(text: &str) -> String {
    let clean: String = text
        .chars()
        .map(|c| match c {
            c if c.is_control() && c.is_whitespace() => ' ',
            c if c.is_control() => '\u{fffd}',
            c => c,
        })
        .collect();
    format!("\"{}\"", cut(&clean, 30))
}

/// A regex that is compiled on first use.
macro_rules! re {
    ($pat:expr) => {
        std::sync::LazyLock::new(|| regex::Regex::new($pat).expect("valid regex"))
    };
}
pub(crate) use re;

pub(crate) type Re = std::sync::LazyLock<regex::Regex>;

static LETTER_OR_DIGIT: Re = re!(r"^[\p{L}\p{N}]$");
static ID_SYMBOL: Re = re!(r"^[\p{L}\p{N}\p{S}\p{M}]$");

/// Whether `c` is in the one-character class `re`.
fn class_match(re: &regex::Regex, c: char) -> bool {
    re.is_match(c.encode_utf8(&mut [0; 4]))
}

/// A letter or digit, as Python's `str.isalnum` sees it.
pub fn is_alnum(c: char) -> bool {
    if c.is_ascii() { c.is_ascii_alphanumeric() } else { class_match(&LETTER_OR_DIGIT, c) }
}

/// A character of a word (`\w` in the prototype's regexes).
pub fn is_word(c: char) -> bool {
    c == '_' || is_alnum(c)
}

/// A character of a flowchart node id: an ASCII letter, digit or `_`, or
/// any non-ASCII letter, digit, symbol or mark.
pub fn is_id_char(c: char) -> bool {
    if c.is_ascii() { c.is_ascii_alphanumeric() || c == '_' } else { class_match(&ID_SYMBOL, c) }
}

/// The HTML names of U+00A0 to U+00FF, in order.
const LATIN1: [&str; 96] = [
    "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf", "laquo", "not", "shy",
    "reg", "macr", "deg", "plusmn", "sup2", "sup3", "acute", "micro", "para", "middot", "cedil", "sup1", "ordm",
    "raquo", "frac14", "frac12", "frac34", "iquest", "Agrave", "Aacute", "Acirc", "Atilde", "Auml", "Aring", "AElig",
    "Ccedil", "Egrave", "Eacute", "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml", "ETH", "Ntilde", "Ograve",
    "Oacute", "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave", "Uacute", "Ucirc", "Uuml", "Yacute", "THORN",
    "szlig", "agrave", "aacute", "acirc", "atilde", "auml", "aring", "aelig", "ccedil", "egrave", "eacute", "ecirc",
    "euml", "igrave", "iacute", "icirc", "iuml", "eth", "ntilde", "ograve", "oacute", "ocirc", "otilde", "ouml",
    "divide", "oslash", "ugrave", "uacute", "ucirc", "uuml", "yacute", "thorn", "yuml",
];

/// Other HTML character names decoded in labels: the rest of HTML 4's
/// names, and a few HTML5 symbols that labels use.
const NAMED: &[(&str, char)] = &[
    ("quot", '"'),
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("apos", '\''),
    ("ensp", ' '),
    ("emsp", ' '),
    ("thinsp", ' '),
    ("ndash", '–'),
    ("mdash", '—'),
    ("lsquo", '‘'),
    ("rsquo", '’'),
    ("sbquo", '‚'),
    ("ldquo", '“'),
    ("rdquo", '”'),
    ("bdquo", '„'),
    ("dagger", '†'),
    ("Dagger", '‡'),
    ("bull", '•'),
    ("hellip", '…'),
    ("permil", '‰'),
    ("prime", '′'),
    ("Prime", '″'),
    ("lsaquo", '‹'),
    ("rsaquo", '›'),
    ("euro", '€'),
    ("trade", '™'),
    ("larr", '←'),
    ("uarr", '↑'),
    ("rarr", '→'),
    ("darr", '↓'),
    ("harr", '↔'),
    ("crarr", '↵'),
    ("lArr", '⇐'),
    ("uArr", '⇑'),
    ("rArr", '⇒'),
    ("dArr", '⇓'),
    ("hArr", '⇔'),
    ("forall", '∀'),
    ("part", '∂'),
    ("exist", '∃'),
    ("empty", '∅'),
    ("nabla", '∇'),
    ("isin", '∈'),
    ("notin", '∉'),
    ("ni", '∋'),
    ("prod", '∏'),
    ("sum", '∑'),
    ("minus", '−'),
    ("lowast", '∗'),
    ("radic", '√'),
    ("prop", '∝'),
    ("infin", '∞'),
    ("ang", '∠'),
    ("and", '∧'),
    ("or", '∨'),
    ("cap", '∩'),
    ("cup", '∪'),
    ("int", '∫'),
    ("there4", '∴'),
    ("sim", '∼'),
    ("cong", '≅'),
    ("asymp", '≈'),
    ("ne", '≠'),
    ("equiv", '≡'),
    ("le", '≤'),
    ("ge", '≥'),
    ("sub", '⊂'),
    ("sup", '⊃'),
    ("nsub", '⊄'),
    ("sube", '⊆'),
    ("supe", '⊇'),
    ("oplus", '⊕'),
    ("otimes", '⊗'),
    ("perp", '⊥'),
    ("sdot", '⋅'),
    ("loz", '◊'),
    ("spades", '♠'),
    ("clubs", '♣'),
    ("hearts", '♥'),
    ("diams", '♦'),
    ("check", '✓'),
    ("cross", '✗'),
    ("star", '☆'),
    ("starf", '★'),
    ("Alpha", 'Α'),
    ("Beta", 'Β'),
    ("Gamma", 'Γ'),
    ("Delta", 'Δ'),
    ("Epsilon", 'Ε'),
    ("Zeta", 'Ζ'),
    ("Eta", 'Η'),
    ("Theta", 'Θ'),
    ("Iota", 'Ι'),
    ("Kappa", 'Κ'),
    ("Lambda", 'Λ'),
    ("Mu", 'Μ'),
    ("Nu", 'Ν'),
    ("Xi", 'Ξ'),
    ("Omicron", 'Ο'),
    ("Pi", 'Π'),
    ("Rho", 'Ρ'),
    ("Sigma", 'Σ'),
    ("Tau", 'Τ'),
    ("Upsilon", 'Υ'),
    ("Phi", 'Φ'),
    ("Chi", 'Χ'),
    ("Psi", 'Ψ'),
    ("Omega", 'Ω'),
    ("alpha", 'α'),
    ("beta", 'β'),
    ("gamma", 'γ'),
    ("delta", 'δ'),
    ("epsilon", 'ε'),
    ("zeta", 'ζ'),
    ("eta", 'η'),
    ("theta", 'θ'),
    ("iota", 'ι'),
    ("kappa", 'κ'),
    ("lambda", 'λ'),
    ("mu", 'μ'),
    ("nu", 'ν'),
    ("xi", 'ξ'),
    ("omicron", 'ο'),
    ("pi", 'π'),
    ("rho", 'ρ'),
    ("sigmaf", 'ς'),
    ("sigma", 'σ'),
    ("tau", 'τ'),
    ("upsilon", 'υ'),
    ("phi", 'φ'),
    ("chi", 'χ'),
    ("psi", 'ψ'),
    ("omega", 'ω'),
];

/// The character an HTML character name stands for. Names are
/// case-sensitive, as in HTML: `rArr` is ⇒ and `rarr` is →.
fn named(name: &str) -> Option<char> {
    if let Some(k) = LATIN1.iter().position(|&n| n == name) {
        return char::from_u32(0xa0 + k as u32);
    }
    NAMED.iter().find(|&&(n, _)| n == name).map(|&(_, c)| c)
}

static BR: Re = re!(r"(?i)<br\s*/?>");
static TAG: Re = re!(r"</?[A-Za-z][^>]*>");
/// A numeric (`#35;`, `&#35;`) or named (`#quot;`, `&quot;`) entity code.
static ENTITY_CODE: Re = re!(r"[#&](?:#?([0-9]+)|([A-Za-z][A-Za-z0-9]*));");
/// A Font Awesome icon, in any of the styles Mermaid draws (`fa:`, `fab:`,
/// `fak:`, `fal:`, `far:`, `fas:`).
static ICON: Re = re!(r"fa[bklrs]?:fa-[\p{L}\p{N}_-]+\s*");
/// An entity code whose `;` might not end a statement; see
/// [`kept_semicolons`].
static ENTITY: Re = re!(r"#[\p{L}\p{N}_]+;|&[A-Za-z][A-Za-z0-9]*;");
static MYST_OPTION: Re = re!(r"^:[\p{L}\p{N}_-]+:(?:\s|$)");
/// A YAML block scalar indicator (`|`, `>-`, `|2+`), maybe with a comment.
static BLOCK_SCALAR: Re = re!(r"^[|>][0-9+-]*(?:\s+#.*)?$");
/// An accessibility line, whose text Mermaid reads to the end of the line.
static ACC_LINE: Re = re!(r"^\s*acc(?:Title|Descr)\s*:");

/// Decode a node, edge or participant label: quotes, Markdown strings,
/// `<br>`, tags, icons such as `fa:fa-car`, and entities. Entities are
/// decoded last and in one pass, so what they make is never read again as
/// markup or as another entity: `#38;lt;` shows `&lt;`. Tabs become spaces
/// and other control characters U+FFFD. Lines are separated by `\n`; runs
/// of spaces become one space.
pub fn decode(text: &str) -> String {
    let mut s = text.trim();
    let mut markdown = false;
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s = &s[1..s.len() - 1];
        if s.len() >= 2 && s.starts_with('`') && s.ends_with('`') {
            s = &s[1..s.len() - 1];
            markdown = true;
        }
    }
    let s = BR.replace_all(s, "\n");
    let s = TAG.replace_all(&s, "");
    let s = ICON.replace_all(&s, "");
    let s = if markdown { emphasis(&s.replace("**", "").replace("__", "")) } else { s.into_owned() };
    let s = ENTITY_CODE.replace_all(&s, |c: &regex::Captures| entity(c).unwrap_or_else(|| c[0].to_string()));
    let s: String = s
        .chars()
        .map(|c| match c {
            '\n' => c,
            c if c.is_control() && c.is_whitespace() => ' ',
            c if c.is_control() => '\u{fffd}',
            c => c,
        })
        .collect();
    let lines: Vec<String> = s.split('\n').map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    lines.join("\n").trim_matches('\n').to_string()
}

/// The character an entity code stands for, or `None` to leave the code as
/// it is. A number past Unicode stays text; a surrogate becomes U+FFFD.
fn entity(c: &regex::Captures) -> Option<String> {
    let ch = match (c.get(1), c.get(2)) {
        (Some(n), _) => {
            let n: u32 = n.as_str().parse().ok().filter(|&n| n <= 0x10ffff)?;
            char::from_u32(n).unwrap_or('\u{fffd}')
        }
        (_, Some(name)) => named(name.as_str())?,
        _ => return None,
    };
    Some(ch.to_string())
}

/// Drop the markers of `*emphasis*` and `_emphasis_` in a Markdown string.
/// A marker must not touch a word character on its outer side.
fn emphasis(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let marker = |c: char| c == '*' || c == '_';
    let mut out = String::with_capacity(s.len());
    let mut p = 0;
    while p < cs.len() {
        let opens = marker(cs[p])
            && (p == 0 || !is_word(cs[p - 1]))
            && cs.get(p + 1).is_some_and(|c| !c.is_whitespace());
        if opens
            && let Some(q) = (p + 2..cs.len()).find(|&q| marker(cs[q]))
            && cs.get(q + 1).is_none_or(|&c| !is_word(c))
        {
            out.extend(&cs[p + 1..q]);
            p = q + 1;
            continue;
        }
        out.push(cs[p]);
        p += 1;
    }
    out
}

/// Split a diagram's source into its front-matter title and the rest, with
/// comments, `%%{…}%%` directives and MyST `:key: value` option lines
/// removed. The title is the front matter's `title:`, or its `caption:`.
pub fn preamble(source: &str) -> Result<(Option<String>, String), NotDrawn> {
    let src = source.replace('\r', "");
    let lines: Vec<&str> = src.split('\n').collect();
    let blank = |i: usize| lines.get(i).is_some_and(|l| l.trim().is_empty());
    let mut i = 0;
    while blank(i) {
        i += 1;
    }
    while i < lines.len() && MYST_OPTION.is_match(lines[i].trim()) {
        i += 1;
    }
    while blank(i) {
        i += 1;
    }
    let mut title = None;
    let mut caption = None;
    if i < lines.len() && lines[i].trim() == "---" {
        // Keys count only at the indent of the opening `---`; deeper ones
        // belong to `config:` and the like.
        let base = indent(lines[i]);
        let mut j = i + 1;
        while j < lines.len() && lines[j].trim() != "---" {
            let line = lines[j];
            j += 1;
            if indent(line) != base {
                continue;
            }
            let Some((key, v)) = line[base..].split_once(':') else {
                continue;
            };
            if key != "title" && key != "caption" {
                continue;
            }
            // The more deeply indented lines below the key belong to its
            // value. YAML joins them with spaces.
            let mut parts = Vec::new();
            while j < lines.len()
                && lines[j].trim() != "---"
                && (lines[j].trim().is_empty() || indent(lines[j]) > base)
            {
                parts.push(lines[j].trim());
                j += 1;
            }
            let joined = format!("{v} {}", parts.join(" "));
            let value = if BLOCK_SCALAR.is_match(v.trim()) {
                // A block scalar (`title: >-`).
                one_line(&parts.join(" "))
            } else if joined.trim_start().starts_with(['"', '\'']) || !parts.iter().any(|p| not_plain(p)) {
                // A quoted value, or a plain one, may go on over the lines
                // below (`title: "Order processing` / `  pipeline"`), and
                // may start on the next line (`title:` / `  Words`).
                front_matter_value(&joined)
            } else {
                // A mapping or a list below the key: it is not text.
                front_matter_value(v)
            };
            if key == "title" {
                title = Some(value);
            } else {
                caption = Some(value);
            }
        }
        if j >= lines.len() {
            return Err(NotDrawn::unsupported("front matter without a closing ---"));
        }
        i = j + 1;
    }
    let title = title.filter(|t| !t.is_empty()).or(caption.filter(|t| !t.is_empty()));
    let later = closed_later(&lines);
    let mut body = Vec::new();
    let mut in_directive = false;
    // Whether the diagram is a flowchart, once its first line is seen.
    let mut flow = None;
    // Whether a line starts inside a Markdown string; it is then text, not
    // a comment or a directive.
    let mut md = false;
    for (n, line) in lines.iter().enumerate().skip(i) {
        let s = line.trim();
        if !md {
            if in_directive {
                in_directive = !s.contains("}%%");
                continue;
            }
            if s.starts_with("%%{") {
                in_directive = !s.contains("}%%");
                continue;
            }
            if s.starts_with("%%") {
                continue;
            }
            if flow.is_none() && !s.is_empty() {
                flow = Some(kind(s) == Kind::Flow);
            }
        }
        let (open, comment) = scan(line, md);
        md = open && later[n] && flow == Some(true);
        body.push(comment.map_or(*line, |c| &line[..c]));
    }
    Ok((title, body.join("\n")))
}

/// The width of a line's leading spaces and tabs, in bytes.
fn indent(line: &str) -> usize {
    line.bytes().take_while(|&b| b == b' ' || b == b'\t').count()
}

/// Whether a line below a front-matter key can't go on a plain value: it
/// starts a list item or holds a `key:`, so the key's value is a list or a
/// mapping. A comment line ends a plain value.
fn not_plain(line: &str) -> bool {
    line.starts_with("- ") || line == "-" || line.starts_with('#') || line.contains(": ") || line.ends_with(':')
}

/// A front-matter value as YAML reads it, on one line. A quoted value loses
/// one pair of quotes (`''` inside single quotes is one quote, and `\` escapes
/// inside double quotes); an unquoted one ends at a comment (` #`).
fn front_matter_value(v: &str) -> String {
    let v = v.trim();
    let mut text = String::new();
    if let Some(rest) = v.strip_prefix('\'') {
        let mut cs = rest.chars().peekable();
        while let Some(c) = cs.next() {
            if c != '\'' {
                text.push(c);
            } else if cs.next_if_eq(&'\'').is_some() {
                text.push('\'');
            } else {
                break;
            }
        }
    } else if let Some(rest) = v.strip_prefix('"') {
        let mut cs = rest.chars();
        while let Some(c) = cs.next() {
            match c {
                '"' => break,
                '\\' => text.extend(cs.next()),
                c => text.push(c),
            }
        }
    } else {
        let end = v
            .char_indices()
            .find(|&(k, c)| c == '#' && (k == 0 || v[..k].ends_with([' ', '\t'])))
            .map_or(v.len(), |(k, _)| k);
        text.push_str(&v[..end]);
    }
    one_line(&text)
}

/// Text on one line: tabs and newlines become spaces, other control
/// characters U+FFFD, and runs of spaces one space.
fn one_line(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| match c {
            c if c.is_control() && c.is_whitespace() => ' ',
            c if c.is_control() => '\u{fffd}',
            c => c,
        })
        .collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The diagram types whose statements are split differently.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Flow,
    Seq,
    Other,
}

/// The kind of diagram whose first line is `line`.
fn kind(line: &str) -> Kind {
    match line.split(|c: char| c.is_whitespace() || c == ';').next().unwrap_or("") {
        "flowchart" | "graph" | "flowchart-v2" | "flowchart-elk" => Kind::Flow,
        "sequenceDiagram" => Kind::Seq,
        _ => Kind::Other,
    }
}

/// Read one line for quotes. Returns whether a Markdown string (``"`…`"``)
/// is still open at its end, and where a `%%` comment outside quotes
/// starts. `md` says whether a Markdown string is open at its start.
fn scan(line: &str, mut md: bool) -> (bool, Option<usize>) {
    let b = line.as_bytes();
    let mut quoted = false;
    let mut k = 0;
    while k < b.len() {
        let next = b.get(k + 1).copied();
        if md {
            if b[k] == b'`' && next == Some(b'"') {
                md = false;
                k += 1;
            }
        } else if quoted {
            quoted = b[k] != b'"';
        } else if b[k] == b'"' {
            if next == Some(b'`') {
                md = true;
                k += 1;
            } else {
                quoted = true;
            }
        } else if b[k] == b'%' && next == Some(b'%') {
            return (false, Some(k));
        }
        k += 1;
    }
    (md, None)
}

/// For each line, whether a later line closes a Markdown string. A
/// Markdown string may go on over several lines, as in Mermaid, but only
/// when something closes it, so a stray quote can't swallow the diagram.
fn closed_later(lines: &[&str]) -> Vec<bool> {
    let mut later = vec![false; lines.len()];
    for n in (0..lines.len().saturating_sub(1)).rev() {
        later[n] = later[n + 1] || lines[n + 1].contains("`\"");
    }
    later
}

/// For each line, whether a later line holds a `}`, which could close
/// `@{ … }` shape data that this line leaves open.
fn brace_later(lines: &[&str]) -> Vec<bool> {
    let mut later = vec![false; lines.len()];
    for n in (0..lines.len().saturating_sub(1)).rev() {
        later[n] = later[n + 1] || lines[n + 1].contains('}');
    }
    later
}

/// Read one flowchart line for what may go on to the next line: a
/// Markdown string (``"`…`"``) and `@{ … }` shape data, which Mermaid
/// reads over several lines. `md` and `data` say which is open at the
/// line's start; the same pair is returned for its end. Shape data starts
/// at an `@{` right after an id, outside quotes, brackets, `>…]` shapes and
/// `|…|` edge text, and ends at the next `}` outside quotes.
fn flow_scan(line: &str, mut md: bool, mut data: bool) -> (bool, bool) {
    let b = line.as_bytes();
    let mut quoted = false;
    let mut piped = false;
    let mut depth = 0usize;
    let mut k = 0;
    while k < b.len() {
        let next = b.get(k + 1).copied();
        if md {
            if b[k] == b'`' && next == Some(b'"') {
                md = false;
                k += 1;
            }
        } else if quoted {
            quoted = b[k] != b'"';
        } else if b[k] == b'"' {
            if next == Some(b'`') {
                md = true;
                k += 1;
            } else {
                quoted = true;
            }
        } else if data {
            data = b[k] != b'}';
        } else {
            // Only asked at an ASCII byte, so `k` is a character boundary.
            let after_id = || line[..k].chars().next_back().is_some_and(is_id_char);
            match b[k] {
                b'@' if next == Some(b'{') && depth == 0 && !piped && after_id() => {
                    data = true;
                    k += 1;
                }
                b'|' if depth == 0 => piped = !piped,
                b'>' if depth == 0 && !piped && after_id() => depth += 1,
                b'[' | b'(' | b'{' => depth += 1,
                b']' | b')' | b'}' => depth = depth.saturating_sub(1),
                // A comment, as in [`scan`].
                b'%' if next == Some(b'%') => return (false, false),
                _ => {}
            }
        }
        k += 1;
    }
    (md, data)
}

/// Split the body into statements: one per line, and lines split again at
/// `;`. The `;` of an entity code such as `#59;` never splits. In a
/// sequence diagram every other `;` does, as in Mermaid. In other diagrams
/// a `;` inside text does not (see [`split_flow`]), and a Markdown string
/// or `@{ … }` shape data that a later line closes keeps its line breaks.
/// Blank statements are dropped and each is trimmed.
pub fn statements(body: &str) -> Vec<String> {
    let lines: Vec<&str> = body.split('\n').collect();
    let kind = lines.iter().map(|l| l.trim()).find(|l| !l.is_empty()).map_or(Kind::Other, kind);
    let mut out = Vec::new();
    if kind == Kind::Seq {
        for line in lines {
            split_seq(line, &mut out);
        }
    } else {
        let flow = kind == Kind::Flow;
        let later = closed_later(&lines);
        let brace_later = brace_later(&lines);
        let mut md = false;
        let mut data = false;
        // Byte offsets in `body` of the current statement line's start and
        // of the next line.
        let mut start = 0;
        let mut at = 0;
        for (n, line) in lines.iter().enumerate() {
            let (m, d) = flow_scan(line, md, data);
            md = m && later[n] && flow;
            data = d && brace_later[n] && flow;
            let end = at + line.len();
            if !md && !data {
                split_flow(&body[start..end], &mut out);
                start = end + 1;
            }
            at = end + 1;
        }
    }
    out.into_iter().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// Where the `;` of each entity code in `line` is: `#59;`, `&#59;`,
/// `#quot;`, and `&quot;` and the other names [`decode`] knows. Mermaid
/// itself keeps only the `#` forms; the `&` names are kept as a lenient
/// extra. After `&` and any other word the `;` ends a statement, so
/// `A --> B&C;` is a group and a statement end.
fn kept_semicolons(line: &str) -> Vec<usize> {
    ENTITY
        .find_iter(line)
        .filter(|m| m.as_str().starts_with('#') || named(&m.as_str()[1..m.len() - 1]).is_some())
        .map(|m| m.end() - 1)
        .collect()
}

/// Whether the statement at the start of `s` runs to the end of the line,
/// `;` included: `accTitle: …` and `accDescr: …`, in every diagram type.
fn runs_to_line_end(s: &str) -> bool {
    ACC_LINE.is_match(s)
}

/// Split a sequence diagram line at every `;` but an entity code's. Mermaid
/// ends message text at `;` whatever brackets or quotes come before it.
fn split_seq<'a>(line: &'a str, out: &mut Vec<&'a str>) {
    let kept = kept_semicolons(line);
    let mut start = 0;
    let mut whole = runs_to_line_end(line);
    for (k, b) in line.bytes().enumerate() {
        if b == b';' && kept.binary_search(&k).is_err() && !whole {
            out.push(&line[start..k]);
            start = k + 1;
            whole = runs_to_line_end(&line[start..]);
        }
    }
    out.push(&line[start..]);
}

/// Split a flowchart line at `;`, except where Mermaid reads `;` as text:
/// inside quotes, brackets and `>…]` shapes, `|…|` edge text, and edge text
/// between a link's halves (`A -- a;b --> B`).
fn split_flow<'a>(line: &'a str, out: &mut Vec<&'a str>) {
    let kept = kept_semicolons(line);
    let b = line.as_bytes();
    let mut depth = 0usize;
    let mut start = 0;
    let mut whole = runs_to_line_end(line);
    // Openers found to have no closer on the rest of the line, so that each
    // search is made once: Markdown strings, `|`, and edge text.
    let mut md_unclosed = false;
    let mut pipe_unclosed = false;
    let mut text_unclosed = [false; 3];
    let mut k = 0;
    while k < b.len() {
        // Where scanning goes on, if not at the next byte.
        let next = match b[k] {
            b'"' => {
                let mut md = None;
                if b.get(k + 1) == Some(&b'`') && !md_unclosed {
                    md = line[k + 2..].find("`\"").map(|e| k + e + 4);
                    md_unclosed = md.is_none();
                }
                // An open quote runs to the end of the line.
                Some(md.or_else(|| line[k + 1..].find('"').map(|e| k + e + 2)).unwrap_or(b.len()))
            }
            b'[' | b'(' | b'{' => {
                depth += 1;
                None
            }
            b']' | b')' | b'}' => {
                depth = depth.saturating_sub(1);
                None
            }
            // `A>text]`: a `>` right after a node id opens a shape.
            b'>' if depth == 0 && line[..k].chars().next_back().is_some_and(is_id_char) => {
                depth += 1;
                None
            }
            b'|' if depth == 0 && !pipe_unclosed => {
                let end = pipe_end(line, k);
                pipe_unclosed = end.is_none();
                end
            }
            b'-' | b'=' if depth == 0 => crate::diagram::flow::parse::link_end(line, k, &mut text_unclosed),
            b';' if depth == 0 && !whole && kept.binary_search(&k).is_err() => {
                out.push(&line[start..k]);
                start = k + 1;
                whole = runs_to_line_end(&line[start..]);
                None
            }
            _ => None,
        };
        k = next.unwrap_or(k + 1);
    }
    out.push(&line[start..]);
}

/// The index after the `|` that closes the `|` at `k`, skipping quotes, or
/// `None` if nothing closes it.
fn pipe_end(line: &str, k: usize) -> Option<usize> {
    let mut quoted = false;
    for (p, b) in line.bytes().enumerate().skip(k + 1) {
        match b {
            b'"' => quoted = !quoted,
            b'|' if !quoted => return Some(p + 1),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_breaks_long_words_and_keeps_newlines() {
        assert_eq!(wrap("a bb ccc", 4), ["a bb", "ccc"]);
        assert_eq!(wrap("abcdefgh", 3), ["abc", "def", "gh"]);
        assert_eq!(wrap("one\ntwo", 10), ["one", "two"]);
        assert_eq!(wrap("", 5), [""]);
    }

    #[test]
    fn labels_keep_short_words_whole_but_break_cjk() {
        assert_eq!(wrap_label("Authentication service", 8), ["Authentication", "service"]);
        assert_eq!(wrap_label("認証サービス", 8), ["認証サー", "ビス"]);
    }

    #[test]
    fn labels_never_break_inside_a_word_of_spacing_marks() {
        // Devanagari and Myanmar clusters can be 2 columns wide, but they
        // are letters of one word, not wide characters to break between.
        for word in ["सिंहावलोकन", "ကြိုဆိုပါသည်"] {
            assert!(width(word) <= UNBROKEN_WORD, "{word}");
            assert_eq!(wrap_label(word, 8), [word]);
        }
        assert_eq!(wrap_label("सिंहावलोकन 認証サービス", 8), ["सिंहावलोकन", "認証サー", "ビス"]);
    }

    #[test]
    fn decode_handles_breaks_tags_entities_and_icons() {
        assert_eq!(decode("\"Hello<br>world\""), "Hello\nworld");
        assert_eq!(decode("a<br/>b<BR />c"), "a\nb\nc");
        assert_eq!(decode("<b>bold</b> <span class='x'>text</span>"), "bold text");
        assert_eq!(decode("#quot;hi#quot; &amp; #35; #9829; #lt;tag#gt;"), "\"hi\" & # ♥ <tag>");
        assert_eq!(decode("#unknown; &#169;"), "#unknown; ©");
        assert_eq!(decode("fa:fa-car Car"), "Car");
        assert_eq!(decode("  many   spaces\there  "), "many spaces here");
        assert_eq!(decode("a\u{1b}[31mb"), "a\u{fffd}[31mb");
        // A surrogate can't be a character; a number past Unicode stays text.
        assert_eq!(decode("#55357;"), "\u{fffd}");
        assert_eq!(decode("#99999999;"), "#99999999;");
        assert_eq!(decode("\"\""), "");
    }

    #[test]
    fn tabs_in_labels_are_spaces() {
        assert_eq!(decode("Name:\tValue"), "Name: Value");
        assert_eq!(decode("\"quoted\ttab\""), "quoted tab");
        assert_eq!(decode("a#9;b"), "a b");
    }

    #[test]
    fn every_icon_style_is_dropped() {
        for icon in ["fa:fa-car", "fab:fa-github", "fas:fa-star", "far:fa-bell", "fal:fa-x", "fak:fa-my-kit"] {
            assert_eq!(decode(&format!("{icon} Name")), "Name", "{icon}");
        }
        assert_eq!(decode("fax:fa-car Car"), "fax:fa-car Car");
    }

    #[test]
    fn entities_are_decoded_once() {
        assert_eq!(decode("#38;lt;"), "&lt;");
        assert_eq!(decode("\"Type #38;lt;br#38;gt; or #38;nbsp; end\""), "Type &lt;br&gt; or &nbsp; end");
        assert_eq!(decode("#35;quot;q#35;quot;"), "#quot;q#quot;");
        assert_eq!(decode("\"`Use #42;stars#42; literally`\""), "Use *stars* literally");
        assert_eq!(decode("#60;br#62; #60;b#62;x#60;/b#62;"), "<br> <b>x</b>");
        assert_eq!(decode("#102;a:fa-car x"), "fa:fa-car x");
    }

    #[test]
    fn entity_names_are_case_sensitive_and_cover_html4() {
        assert_eq!(
            decode("x #le; 10 #mdash; ok #hellip; #rArr; #deg; #lArr; #RARR; #frac12; #rarr; #copy;"),
            "x ≤ 10 — ok … ⇒ ° ⇐ #RARR; ½ → ©"
        );
        assert_eq!(decode("a #rArr; b #mdash; c #Hearts; #hearts;"), "a ⇒ b — c #Hearts; ♥");
        assert_eq!(decode("&eacute;t&eacute; #Omega; #omega; #sigmaf; #yuml; #nbsp;x"), "été Ω ω ς ÿ x");
        assert_eq!(decode("#sup2; #sup; #euro;5 #infin;"), "² ⊃ €5 ∞");
    }

    #[test]
    fn markdown_strings_drop_their_markers() {
        assert_eq!(decode("\"`**Bold** and *it* and _this_`\""), "Bold and it and this");
        assert_eq!(decode("\"`snake_case_name`\""), "snake_case_name");
        assert_eq!(decode("\"`Line one\nLine two`\""), "Line one\nLine two");
        // Outside a Markdown string the markers stay.
        assert_eq!(decode("**Bold**"), "**Bold**");
    }

    #[test]
    fn quote_cuts_and_cleans_user_text() {
        assert_eq!(quote("-|\\"), "\"-|\\\"");
        assert_eq!(quote("a\tb\u{7}c"), "\"a b\u{fffd}c\"");
        let long = quote(&"x".repeat(100));
        assert_eq!(width(&long), 32);
        assert!(long.ends_with("…\""));
    }

    #[test]
    fn preamble_keeps_the_title_and_drops_options_directives_and_comments() {
        let src = "\n\
            :name: release\n\
            :align: center\n\
            \n\
            ---\n\
            title: \"Release flow\"\n\
            config:\n  title: not this\n  theme: dark\n\
            ---\n\
            %%{init: {'theme': 'dark'}}%%\n\
            %% a comment\n\
            flowchart LR\n\
            A --> B %% trailing\n\
            C[\"50%% off\"]\n";
        let (title, body) = preamble(src).unwrap();
        assert_eq!(title.as_deref(), Some("Release flow"));
        assert_eq!(statements(&body), ["flowchart LR", "A --> B", "C[\"50%% off\"]"]);
    }

    #[test]
    fn caption_is_the_title_when_there_is_no_title() {
        let (title, _) = preamble("---\ncaption: A caption\nalt: words\n---\ngraph TD\nA").unwrap();
        assert_eq!(title.as_deref(), Some("A caption"));
        let (title, _) = preamble("---\ntitle: T\ncaption: C\n---\ngraph TD\nA").unwrap();
        assert_eq!(title.as_deref(), Some("T"));
        let (title, _) = preamble("graph TD\n---\ntitle: T\n---").unwrap();
        assert_eq!(title, None);
    }

    #[test]
    fn front_matter_values_are_read_as_yaml() {
        let title = |fm: &str| preamble(&format!("---\n{fm}\n---\nflowchart LR\nA --> B")).unwrap().0;
        assert_eq!(title("title: 'Say \"hi\"'").as_deref(), Some("Say \"hi\""));
        assert_eq!(title("title: 'It''s'").as_deref(), Some("It's"));
        assert_eq!(title("title: \"a \\\"b\\\"\" # note").as_deref(), Some("a \"b\""));
        assert_eq!(title("title: Release flow # the main one").as_deref(), Some("Release flow"));
        assert_eq!(title("title: C# notes").as_deref(), Some("C# notes"));
        assert_eq!(title("title: >-\n  Release\n  flow\nconfig:\n  theme: dark").as_deref(), Some("Release flow"));
        assert_eq!(title("title: |\n  One\n\n  Two").as_deref(), Some("One Two"));
        assert_eq!(title("title: # nothing\ncaption: Cap").as_deref(), Some("Cap"));
        assert_eq!(title("title:\ttabbed\ttext").as_deref(), Some("tabbed text"));
        // An indented block: keys at the opening `---`'s indent count.
        let (title, body) = preamble("  ---\n  title: Indented\n  config:\n    title: no\n  ---\n  flowchart LR\n  A").unwrap();
        assert_eq!(title.as_deref(), Some("Indented"));
        assert_eq!(statements(&body), ["flowchart LR", "A"]);
    }

    #[test]
    fn front_matter_values_may_go_on_over_lines() {
        let title = |fm: &str| preamble(&format!("---\n{fm}\n---\nflowchart LR\nA --> B")).unwrap().0;
        assert_eq!(title("title: \"Order processing\n  pipeline\"").as_deref(), Some("Order processing pipeline"));
        assert_eq!(title("title: This is\n  a long title\nconfig:\n  theme: dark").as_deref(), Some("This is a long title"));
        assert_eq!(title("title:\n  My Long Title").as_deref(), Some("My Long Title"));
        assert_eq!(title("title:\n  \"Step 1: load\"").as_deref(), Some("Step 1: load"));
        assert_eq!(title("title: 'It''s\n\n  two'").as_deref(), Some("It's two"));
        assert_eq!(title("title: Plain # note\n  more").as_deref(), Some("Plain"));
        // A mapping or list below the key is not a title.
        assert_eq!(title("title:\n  text: x\ncaption: Cap").as_deref(), Some("Cap"));
        assert_eq!(title("title:\n  - a\n  - b").as_deref(), None);
        // Deeper lines under other keys are still skipped.
        assert_eq!(title("config:\n  title: no\ntitle: yes").as_deref(), Some("yes"));
    }

    #[test]
    fn shape_data_may_span_lines() {
        let st = |s: &str| statements(&format!("flowchart LR\n{s}"))[1..].to_vec();
        assert_eq!(st("A@{\n  shape: cyl\n  label: \"x;y\"\n} --> B; C"), ["A@{\n  shape: cyl\n  label: \"x;y\"\n} --> B", "C"]);
        assert_eq!(st("A@{ shape: cyl\n  label: \"}\" }\nB"), ["A@{ shape: cyl\n  label: \"}\" }", "B"]);
        // Only an `@{` after an id, outside text, and only when a later line
        // has a `}`.
        assert_eq!(st("A@{ shape: cyl\nB --> C"), ["A@{ shape: cyl", "B --> C"]);
        assert_eq!(st("A[\"@{\"] --> B[x@{]\nC -->|y@{| D\nE>z@{]\nF{a}"), ["A[\"@{\"] --> B[x@{]", "C -->|y@{| D", "E>z@{]", "F{a}"]);
        for odd in ["@{", "é@{", "é>@{", "😀@{\n\"", "\"`é@{", "|é@{|", "é\u{3000}@{ x", "A@{ \"é", "A@{ %% x"] {
            assert!(!st(&format!("{odd}\n}}")).is_empty(), "{odd:?}");
        }
        // Sequence diagrams have no shape data.
        assert_eq!(statements("sequenceDiagram\nA->>B: x@{\nB->>A: }"), ["sequenceDiagram", "A->>B: x@{", "B->>A: }"]);
    }

    #[test]
    fn directives_may_span_lines() {
        let (_, body) = preamble("%%{\n  init: {\n    'theme': 'dark'\n  }\n}%%\nsequenceDiagram\nA->>B: hi").unwrap();
        assert_eq!(statements(&body), ["sequenceDiagram", "A->>B: hi"]);
    }

    #[test]
    fn front_matter_must_be_closed() {
        assert_eq!(
            preamble("---\ntitle: T\nflowchart TD\nA --> B"),
            Err(NotDrawn::unsupported("front matter without a closing ---"))
        );
    }

    #[test]
    fn statements_split_at_semicolons_outside_entities_quotes_and_brackets() {
        assert_eq!(statements("graph TD; A-->B;B-->C;"), ["graph TD", "A-->B", "B-->C"]);
        assert_eq!(statements("A[\"x;y\"] --> B[a;b]"), ["A[\"x;y\"] --> B[a;b]"]);
        assert_eq!(
            statements("sequenceDiagram\nAlice->>Bob: I #9829; you; ok"),
            ["sequenceDiagram", "Alice->>Bob: I #9829; you", "ok"]
        );
        assert_eq!(statements("A -->|#59;| B"), ["A -->|#59;| B"]);
        assert_eq!(statements("  \n\t\n ; ;"), Vec::<String>::new());
    }

    #[test]
    fn a_semicolon_after_an_ampersand_group_ends_the_statement() {
        assert_eq!(statements("graph TD; A-->B&C; C-->D;"), ["graph TD", "A-->B&C", "C-->D"]);
        assert_eq!(statements("flowchart LR\n  A --> B&C; C --> D"), ["flowchart LR", "A --> B&C", "C --> D"]);
        assert_eq!(
            statements("sequenceDiagram\nAlice->>Bob: Q&A; Bob->>Alice: ok"),
            ["sequenceDiagram", "Alice->>Bob: Q&A", "Bob->>Alice: ok"]
        );
        // Entity codes keep their `;`: the `#` forms, and `&` with a name
        // that decode knows.
        assert_eq!(
            statements("sequenceDiagram\nA->>B: Q &amp; A &#169; #quot;x#quot;; B->>A: ok"),
            ["sequenceDiagram", "A->>B: Q &amp; A &#169; #quot;x#quot;", "B->>A: ok"]
        );
    }

    #[test]
    fn flowchart_text_keeps_its_semicolons() {
        let one = |s: &str| assert_eq!(statements(&format!("flowchart LR\n{s}")), ["flowchart LR", s]);
        one("A -.->|retry; backoff| B");
        one("A -->|\"a|b; c\"| B");
        one("A>Flag; raised] --> B");
        one("A -- a;b --> B");
        one("A <-- a;b --> B");
        one("A == a;b ==> B");
        one("A -. a;b .-> B");
        one("A@{ shape: rect; label: x } --> B");
        let two = |s: &str, a: &str, b: &str| assert_eq!(statements(&format!("flowchart LR\n{s}")), ["flowchart LR", a, b]);
        two("A --- B; C --> D", "A --- B", "C --> D");
        two("A-->|x|B; C-->D", "A-->|x|B", "C-->D");
        two("A-->B>x] ; C", "A-->B>x]", "C");
        two("A --> B; A>x]", "A --> B", "A>x]");
        two("A -- unclosed; B", "A -- unclosed", "B");
        two("A | B; C", "A | B", "C");
        // Quoted edge text may hold an arrow.
        one("A -- \"a --> b; c\" --> B");
        two("A -- \"a --> b\" --> B; C", "A -- \"a --> b\" --> B", "C");
        two("A == \"x ==> y\" ==> B; C", "A == \"x ==> y\" ==> B", "C");
    }

    #[test]
    fn sequence_text_ends_at_every_semicolon() {
        let two = |s: &str, a: &str, b: &str| {
            assert_eq!(statements(&format!("sequenceDiagram\n{s}")), ["sequenceDiagram", a, b]);
        };
        two("A->>B: sad :( ; B->>A: ok", "A->>B: sad :(", "B->>A: ok");
        two("A->>B: a 12\" pipe; B->>A: ok", "A->>B: a 12\" pipe", "B->>A: ok");
        two("A->>B: [x; y]", "A->>B: [x", "y]");
        two("A->>B: \"`x; y`\"", "A->>B: \"`x", "y`\"");
    }

    #[test]
    fn long_lines_of_unclosed_openers_split_quickly() {
        // Each opener here has no closer; searching for one from every
        // opener would take minutes.
        for unit in ["-.a ", "-- a ", "== a ", "| \"", "\"`x\" ", "; ", "-. \".-\" ", "-- \"-->\" ", "x@{ "] {
            let line = format!("flowchart LR\nA {}", unit.repeat(16_000));
            let start = std::time::Instant::now();
            let _ = statements(&line);
            assert!(start.elapsed().as_secs() < 5, "{unit:?}");
        }
        let line = format!("sequenceDiagram\n;{}accDescr: x{}", " ".repeat(30_000), ";".repeat(30_000));
        let start = std::time::Instant::now();
        assert_eq!(statements(&line).len(), 2);
        assert!(start.elapsed().as_secs() < 5);
    }

    #[test]
    fn accessibility_text_runs_to_the_end_of_the_line() {
        assert_eq!(
            statements("sequenceDiagram\naccDescr: a; b {c\nA->>B: x; accTitle : t; u"),
            ["sequenceDiagram", "accDescr: a; b {c", "A->>B: x", "accTitle : t; u"]
        );
        assert_eq!(statements("flowchart LR\nA-->B; accTitle: x; y"), ["flowchart LR", "A-->B", "accTitle: x; y"]);
    }

    #[test]
    fn markdown_strings_may_span_lines() {
        let src = "flowchart LR\n  a(\"`The **cat**\n  in the hat`\") -- \"edge label\" --> b{{\"`The **dog** in the hog`\"}}";
        let (_, body) = preamble(src).unwrap();
        let st = statements(&body);
        assert_eq!(st.len(), 2, "{st:?}");
        assert!(st[1].starts_with("a(\"`The **cat**\n  in the hat`\")"), "{st:?}");
        // A line inside the string is text, not a comment.
        let (_, body) = preamble("flowchart LR\nA[\"`one\n%% two\n%%{three}%%\nfour`\"] --> B %% gone").unwrap();
        assert_eq!(statements(&body), ["flowchart LR", "A[\"`one\n%% two\n%%{three}%%\nfour`\"] --> B"]);
        // A string that nothing closes stays on its line.
        let (_, body) = preamble("flowchart LR\nA[\"`open] %% gone\nB --> C").unwrap();
        assert_eq!(statements(&body), ["flowchart LR", "A[\"`open] %% gone", "B --> C"]);
        // Sequence diagrams have no Markdown strings.
        let (_, body) = preamble("sequenceDiagram\nA->>B: \"`x\nB->>A: y`\"").unwrap();
        assert_eq!(statements(&body), ["sequenceDiagram", "A->>B: \"`x", "B->>A: y`\""]);
    }

    #[test]
    fn python_character_classes() {
        assert!(is_alnum('a') && is_alnum('7') && is_alnum('é') && is_alnum('認') && is_alnum('²'));
        assert!(!is_alnum('_') && !is_alnum('-') && !is_alnum('。'));
        assert!(is_id_char('_') && is_id_char('認') && is_id_char('😀') && is_id_char('\u{301}'));
        assert!(!is_id_char('-') && !is_id_char('#') && !is_id_char('。') && !is_id_char('\u{3000}'));
    }

    /// What `render` does with `src` at 80 columns.
    fn outcome(src: &str) -> String {
        use crate::diagram::{Charset, Diagrams, Outcome, Request, render};
        let mut dg = Diagrams::default();
        match render(src, &Request { width: 80, charset: Charset::Unicode }, &mut dg) {
            Outcome::Source(why) => format!("source: {why}"),
            Outcome::Outline(_) | Outcome::Drawn(_) => "shown".into(),
        }
    }

    #[test]
    fn degenerate_diagrams_have_fixed_outcomes() {
        let empty = "source: empty diagram";
        let blank = ["", "\n  \n", "%% only a comment\n%%{init: {}}%%", ":align: center\n:name: x"];
        for src in blank.into_iter().chain(["---\ntitle: T\n---\n"]) {
            assert_eq!(outcome(src), empty, "{src:?}");
        }
        assert_eq!(outcome("flowchart TD"), empty);
        assert_eq!(outcome("graph LR;"), empty);
        assert_eq!(outcome("sequenceDiagram\n%% nothing"), empty);
        let presentational = "flowchart TD
            style A fill:#f9f
            classDef big font-size:20px
            class A big
            linkStyle 0 stroke:red
            click A callback
            accTitle: Title
            accDescr: Words
            accDescr {
              More words
            }";
        assert_eq!(outcome(presentational), "source: nothing to draw");
        assert_eq!(outcome("sequenceDiagram\nautonumber"), "source: nothing to draw");
        assert_eq!(outcome("flowchart TD\n  A"), "shown");
        assert_eq!(outcome("flowchart LR\n  A ~~~ B"), "shown");
        assert_eq!(outcome("sequenceDiagram\n  participant A\n  participant B"), "shown");
    }

    #[test]
    fn other_types_and_syntax_errors_fall_back_to_source() {
        assert_eq!(outcome("pie title Pets\n  \"Dogs\" : 386"), "source: pie diagrams aren't drawn yet");
        assert_eq!(outcome("flowchart TD\n  A --> B -|\\ C"), "source: unexpected text \"-|\\ C\"");
        assert_eq!(outcome("sequenceDiagram\n  A ->>> B"), "source: unexpected statement \"A ->>> B\"");
        assert_eq!(outcome("flowchart TD\n  A --> end"), "source: lowercase \"end\" used as a node name");
    }
}

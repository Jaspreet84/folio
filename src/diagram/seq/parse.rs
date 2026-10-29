//! The sequence diagram parser (plan section 3.4), ported from `parse_seq`
//! in the prototype. Keywords are read in any case, as Mermaid's lexer
//! reads them.

use std::collections::HashMap;

use super::{Autonumber, Block, Branch, Event, NotePlace, PartKind, Participant, SeqBox, SeqDiagram, SeqHead};
use crate::diagram::NotDrawn;
use crate::diagram::flow::Stroke;
use crate::diagram::pre::{Re, decode, quote, re};

static PARTICIPANT: Re = re!(r"(?i)^(participant|actor)\s+(.+?)(?:@\{[^}]*\})?(?:\s+as\s+(.+))?$");
static CREATE: Re = re!(r"(?i)^create\s+(participant|actor)\s+(.+?)(?:\s+as\s+(.+))?$");
static DESTROY: Re = re!(r"(?i)^destroy\s+(.+)$");
static ACTIVATE: Re = re!(r"(?i)^(activate|deactivate)\s+(.+)$");
static AUTONUMBER: Re = re!(r"(?i)^autonumber(?:\s+(off)|(?:\s+([0-9]+)(?:\s+([0-9]+))?)?)$");
static NOTE: Re = re!(r"(?i)^note\s+(left of|right of|over)\s+([^:]+?)\s*:\s*(.*)$");
static OPEN: Re = re!(r"(?i)^(loop|alt|opt|par_over|par|critical|break|rect)\b\s*(.*)$");
static BOX: Re = re!(r"(?i)^box\b\s*(.*)$");
static BRANCH: Re = re!(r"(?i)^(else|and|option)\b\s*(.*)$");
static LINKS: Re = re!(r"(?i)^links?\s+([^:]+?)\s*:");
/// Mermaid's split of a `box` line into a colour and a title. The function
/// names are matched in lower case only, as in Mermaid.
static BOX_COLOR: Re = re!(r"^((?:rgba?|hsla?)\s*\(.*\)|[A-Za-z0-9_]*)(.*)$");
static MESSAGE: Re = re!(concat!(
    r"^(?P<a>[^:]+?)\s*(?P<arrow><<-->>|<<->>|-->>|->>|--x|-x|--\)|-\)|-->|->)",
    r"\s*(?P<act>[+-]?)\s*(?P<b>[^:]+?)\s*:\s*(?P<text>.*)$"
));

/// Parse a sequence diagram. `header` is the first statement
/// (`sequenceDiagram`, possibly followed by more text).
pub fn parse(header: &str, statements: &[String]) -> Result<SeqDiagram, NotDrawn> {
    let mut sd = SeqDiagram::default();
    let rest = header.trim().strip_prefix("sequenceDiagram").unwrap_or("").trim();
    let mut todo: Vec<&str> = Vec::new();
    if !rest.is_empty() {
        todo.push(rest);
    }
    todo.extend(statements.iter().map(String::as_str));
    if todo.is_empty() {
        return Err(NotDrawn::unsupported("empty diagram"));
    }
    // Open blocks; `None` is a `box`, which has no events of its own.
    let mut stack: Vec<Option<Block>> = Vec::new();
    // Open boxes, as indexes into `sd.boxes`, and the box each participant
    // was declared in.
    let mut boxes_open: Vec<usize> = Vec::new();
    let mut in_box: HashMap<usize, usize> = HashMap::new();
    let mut skipping = false;
    for st in todo {
        let st = st.trim();
        if skipping {
            skipping = !st.contains('}');
            continue;
        }
        let first = st.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
        if first == "title" || first.starts_with("title:") {
            // `first` starts with the 5 ASCII bytes of `title`.
            let rest = st.get(5..).unwrap_or("").trim_start();
            let title = decode(rest.strip_prefix(':').unwrap_or(rest)).replace('\n', " ");
            if !title.is_empty() {
                sd.title = Some(title);
            }
            continue;
        }
        if ["acctitle", "accdescr"].contains(&first.as_str())
            || ["acctitle:", "accdescr:", "accdescr{"].iter().any(|p| first.starts_with(p))
        {
            // A multi-line `accDescr { … }` runs to its `}`. In the one-line
            // form `accDescr: …`, a `{` is just text.
            let block = first.starts_with("accdescr") && st.get(8..).is_some_and(|r| r.trim_start().starts_with('{'));
            skipping = block && !st.contains('}');
            continue;
        }
        if let Some(c) = PARTICIPANT.captures(st) {
            let k = part(&mut sd, &c[2], c.get(3).map(|m| text(m.as_str())), kind(&c[1]));
            join_box(&mut sd, &mut in_box, boxes_open.last().copied(), k)?;
            continue;
        }
        if let Some(c) = CREATE.captures(st) {
            let k = part(&mut sd, &c[2], c.get(3).map(|m| text(m.as_str())), kind(&c[1]));
            join_box(&mut sd, &mut in_box, boxes_open.last().copied(), k)?;
            sd.participants[k].created = Some(sd.events.len());
            sd.simplified.insert("create".into());
            continue;
        }
        if let Some(c) = DESTROY.captures(st) {
            let name = keyword_name(&c[1]).ok_or_else(|| unexpected(st))?;
            let k = part(&mut sd, name, None, PartKind::Participant);
            sd.events.push(Event::Destroy(k));
            continue;
        }
        if let Some(c) = ACTIVATE.captures(st) {
            let name = keyword_name(&c[2]).ok_or_else(|| unexpected(st))?;
            let k = part(&mut sd, name, None, PartKind::Participant);
            let on = c[1].eq_ignore_ascii_case("activate");
            sd.events.push(if on { Event::Activate(k) } else { Event::Deactivate(k) });
            continue;
        }
        if let Some(c) = AUTONUMBER.captures(st) {
            sd.autonumbers.push((sd.events.len(), autonumber(&c)));
            if c.get(1).is_none() {
                let n = |i: usize| c.get(i).map_or(1, |m| m.as_str().parse::<u32>().unwrap_or(u32::MAX));
                sd.autonumber = Some((n(2), n(3)));
            }
            continue;
        }
        if let Some(c) = NOTE.captures(st) {
            let at = match c[1].to_lowercase().as_str() {
                "left of" => NotePlace::LeftOf,
                "right of" => NotePlace::RightOf,
                _ => NotePlace::Over,
            };
            let mut who = Vec::new();
            for x in c[2].split(',') {
                let name = message_name(x).ok_or_else(|| unexpected(st))?;
                who.push(part(&mut sd, name, None, PartKind::Participant));
            }
            sd.events.push(Event::Note { at, who, label: text(&c[3]) });
            continue;
        }
        if let Some(c) = OPEN.captures(st) {
            let kind = block(&c[1]);
            let label = if kind == Block::Rect { String::new() } else { text(&c[2]) };
            sd.events.push(Event::Open { kind, label });
            stack.push(Some(kind));
            continue;
        }
        if let Some(c) = BOX.captures(st) {
            boxes_open.push(sd.boxes.len());
            sd.boxes.push(SeqBox { title: box_title(&c[1]), members: Vec::new() });
            stack.push(None);
            sd.simplified.insert("box".into());
            continue;
        }
        if let Some(c) = BRANCH.captures(st) {
            if !matches!(stack.last(), Some(Some(_))) {
                return Err(NotDrawn::unsupported(format!("\"{}\" outside a block", &c[1])));
            }
            let kind = match c[1].to_ascii_lowercase().as_str() {
                "else" => Branch::Else,
                "and" => Branch::And,
                _ => Branch::Option,
            };
            sd.events.push(Event::Divider { kind, label: text(&c[2]) });
            continue;
        }
        if st.eq_ignore_ascii_case("end") {
            match stack.pop() {
                None => return Err(NotDrawn::unsupported("\"end\" without a block")),
                Some(Some(_)) => sd.events.push(Event::Close),
                Some(None) => {
                    boxes_open.pop();
                }
            }
            continue;
        }
        if let Some(c) = LINKS.captures(st) {
            // Mermaid ignores a link to a participant it doesn't know, so the
            // name is only checked.
            message_name(&c[1]).ok_or_else(|| unexpected(st))?;
            continue;
        }
        if let Some(c) = MESSAGE.captures(st) {
            // `()` next to the arrow is a central connection (v11.12.3+),
            // which folio doesn't draw.
            if c["a"].trim_end().ends_with("()") || c["b"].trim_start().starts_with("()") {
                return Err(unexpected(st));
            }
            let (Some(a), Some(b)) = (message_name(&c["a"]), message_name(&c["b"])) else {
                return Err(unexpected(st));
            };
            let from = part(&mut sd, a, None, PartKind::Participant);
            let to = part(&mut sd, b, None, PartKind::Participant);
            let (stroke, head, both) = arrow(&c["arrow"]);
            let act = match &c["act"] {
                "+" => Some(true),
                "-" => Some(false),
                _ => None,
            };
            sd.events.push(Event::Message { from, to, stroke, head, both, label: text(&c["text"]), act });
            continue;
        }
        return Err(unexpected(st));
    }
    if !stack.is_empty() {
        return Err(NotDrawn::unsupported("block without \"end\""));
    }
    if sd.participants.is_empty() {
        return Err(NotDrawn::unsupported("nothing to draw"));
    }
    Ok(sd)
}

fn unexpected(st: &str) -> NotDrawn {
    NotDrawn::unsupported(format!("unexpected statement {}", quote(st)))
}

/// Message, note, block and alias text, decoded. A leading `wrap:` or
/// `nowrap:` (also `:wrap:`) is a setting in Mermaid, not text, so it is
/// dropped; Mermaid matches it in lower case only.
fn text(s: &str) -> String {
    let s = s.trim();
    let t = s.strip_prefix(':').unwrap_or(s);
    decode(t.strip_prefix("wrap:").or_else(|| t.strip_prefix("nowrap:")).unwrap_or(s))
}

/// An `autonumber` statement: `off`, or an optional start and step. As in
/// Mermaid, a start without a step sets the step to 1, and a 0 counts as
/// left out.
fn autonumber(c: &regex::Captures) -> Autonumber {
    if c.get(1).is_some() {
        return Autonumber { start: None, step: None, visible: false };
    }
    let n = |i: usize| c.get(i).map(|m| m.as_str().parse::<u64>().unwrap_or(u64::MAX)).filter(|&n| n != 0);
    let step = if c.get(2).is_some() && c.get(3).is_none() { Some(1) } else { n(3) };
    Autonumber { start: n(2), step, visible: true }
}

/// Put participant `k`, just declared, in the box `open` if a box is open.
/// Mermaid keeps a participant in the first box it was declared in and
/// rejects a diagram that declares it in a second one.
fn join_box(
    sd: &mut SeqDiagram,
    in_box: &mut HashMap<usize, usize>,
    open: Option<usize>,
    k: usize,
) -> Result<(), NotDrawn> {
    let Some(b) = open else { return Ok(()) };
    match in_box.get(&k) {
        Some(&old) if old != b => {
            Err(NotDrawn::unsupported(format!("{} is in two boxes", quote(&sd.participants[k].id))))
        }
        Some(_) => Ok(()),
        None => {
            in_box.insert(k, b);
            sd.boxes[b].members.push(k);
            Ok(())
        }
    }
}

/// A `box`'s title: the text after `box` without the colour in front. As
/// in Mermaid, the first word, or an `rgb(…)`, `rgba(…)`, `hsl(…)` or
/// `hsla(…)` there, is a colour only if CSS reads it as one; otherwise the
/// whole text is the title.
fn box_title(s: &str) -> String {
    let s = s.trim();
    let title = match BOX_COLOR.captures(s) {
        Some(c) if is_color(&c[1]) => c.get(2).map_or("", |m| m.as_str()),
        _ => s,
    };
    text(title)
}

/// The colour names CSS knows, system colours and CSS-wide keywords
/// included, which a browser accepts as a colour in any case.
const COLOR_NAMES: &str = "aliceblue antiquewhite aqua aquamarine azure beige bisque black \
    blanchedalmond blue blueviolet brown burlywood cadetblue chartreuse chocolate coral \
    cornflowerblue cornsilk crimson cyan darkblue darkcyan darkgoldenrod darkgray darkgreen \
    darkgrey darkkhaki darkmagenta darkolivegreen darkorange darkorchid darkred darksalmon \
    darkseagreen darkslateblue darkslategray darkslategrey darkturquoise darkviolet deeppink \
    deepskyblue dimgray dimgrey dodgerblue firebrick floralwhite forestgreen fuchsia gainsboro \
    ghostwhite gold goldenrod gray green greenyellow grey honeydew hotpink indianred indigo \
    ivory khaki lavender lavenderblush lawngreen lemonchiffon lightblue lightcoral lightcyan \
    lightgoldenrodyellow lightgray lightgreen lightgrey lightpink lightsalmon lightseagreen \
    lightskyblue lightslategray lightslategrey lightsteelblue lightyellow lime limegreen linen \
    magenta maroon mediumaquamarine mediumblue mediumorchid mediumpurple mediumseagreen \
    mediumslateblue mediumspringgreen mediumturquoise mediumvioletred midnightblue mintcream \
    mistyrose moccasin navajowhite navy oldlace olive olivedrab orange orangered orchid \
    palegoldenrod palegreen paleturquoise palevioletred papayawhip peachpuff peru pink plum \
    powderblue purple rebeccapurple red rosybrown royalblue saddlebrown salmon sandybrown \
    seagreen seashell sienna silver skyblue slateblue slategray slategrey snow springgreen \
    steelblue tan teal thistle tomato turquoise violet wheat white whitesmoke yellow \
    yellowgreen transparent currentcolor \
    activetext buttonborder buttonface buttontext canvas canvastext field fieldtext graytext \
    highlight highlighttext linktext mark marktext selecteditem selecteditemtext visitedtext \
    activeborder activecaption appworkspace background buttonhighlight buttonshadow \
    captiontext inactiveborder inactivecaption inactivecaptiontext infobackground infotext \
    menu menutext scrollbar threeddarkshadow threedface threedhighlight threedlightshadow \
    threedshadow window windowframe windowtext \
    inherit initial unset revert";

/// Whether CSS reads `s` (a word, or a colour function call) as a colour.
fn is_color(s: &str) -> bool {
    if let Some((name, args)) = s.split_once('(') {
        let args = args.strip_suffix(')').unwrap_or("");
        let ok = |c: char| c.is_ascii_alphanumeric() || " \t.,%/+-".contains(c);
        return ["rgb", "rgba", "hsl", "hsla"].contains(&name) && !args.trim().is_empty() && args.chars().all(ok);
    }
    !s.is_empty() && COLOR_NAMES.split_whitespace().any(|c| c.eq_ignore_ascii_case(s))
}

/// A participant name after `activate`, `deactivate` or `destroy`, trimmed.
/// Mermaid ends these names at `<`, `>`, `:`, `,`, `;` and `@`.
fn keyword_name(s: &str) -> Option<&str> {
    let s = s.trim();
    (!s.is_empty() && !s.contains(['<', '>', ':', ',', ';', '@'])).then_some(s)
}

/// A participant name in a message or note, trimmed. Mermaid ends these
/// names at `+`, `<`, `>`, `,`, `;` and `:`, and allows a `-` only inside
/// one, so `A->>>B` and `A->>+-B` are errors, not a participant `>B`.
fn message_name(s: &str) -> Option<&str> {
    let s = s.trim();
    let bad = s.is_empty() || s.contains(['+', '<', '>', ',', ';', ':']) || s.starts_with('-') || s.ends_with('-');
    (!bad).then_some(s)
}

/// The participant named `id`, added if new. A label replaces the old one.
fn part(sd: &mut SeqDiagram, id: &str, label: Option<String>, kind: PartKind) -> usize {
    let id = id.trim();
    if let Some(&k) = sd.ids.get(id) {
        if let Some(label) = label {
            sd.participants[k].label = label;
        }
        return k;
    }
    let k = sd.participants.len();
    sd.ids.insert(id.to_string(), k);
    let label = label.unwrap_or_else(|| id.to_string());
    sd.participants.push(Participant { id: id.to_string(), label, kind, created: None });
    k
}

fn kind(word: &str) -> PartKind {
    if word.eq_ignore_ascii_case("actor") { PartKind::Actor } else { PartKind::Participant }
}

fn block(word: &str) -> Block {
    match word.to_ascii_lowercase().as_str() {
        "loop" => Block::Loop,
        "alt" => Block::Alt,
        "opt" => Block::Opt,
        "par" => Block::Par,
        "par_over" => Block::ParOver,
        "critical" => Block::Critical,
        "break" => Block::Break,
        _ => Block::Rect,
    }
}

/// Stroke, head and whether both ends have a head.
fn arrow(a: &str) -> (Stroke, SeqHead, bool) {
    let stroke = if a.starts_with("--") || a.starts_with("<<--") { Stroke::Dotted } else { Stroke::Solid };
    let head = if a.ends_with(">>") {
        SeqHead::Arrow
    } else if a.ends_with('x') {
        SeqHead::Cross
    } else if a.ends_with(')') {
        SeqHead::Async
    } else {
        SeqHead::None
    };
    (stroke, head, a.starts_with("<<"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram::pre;

    fn seq(src: &str) -> Result<SeqDiagram, NotDrawn> {
        let (_, body) = pre::preamble(src).unwrap();
        let st = pre::statements(&body);
        parse(&st[0], &st[1..])
    }

    fn ok(src: &str) -> SeqDiagram {
        seq(src).unwrap_or_else(|e| panic!("{src:?}: {e:?}"))
    }

    fn why(src: &str) -> String {
        match seq(src) {
            Err(NotDrawn::Unsupported(why)) => why,
            other => panic!("{src:?}: {other:?}"),
        }
    }

    #[test]
    fn every_documented_arrow() {
        let sd = ok("sequenceDiagram
            A->B: 1
            A-->B: 2
            A->>B: 3
            A-->>B: 4
            A-xB: 5
            A--xB: 6
            A-)B: 7
            A--)B: 8
            A<<->>B: 9
            A<<-->>B: 10");
        use SeqHead::*;
        use Stroke::*;
        let arrows: Vec<_> = sd
            .events
            .iter()
            .map(|e| match e {
                Event::Message { stroke, head, both, .. } => (*stroke, *head, *both),
                _ => panic!("{e:?}"),
            })
            .collect();
        assert_eq!(
            arrows,
            [
                (Solid, None, false),
                (Dotted, None, false),
                (Solid, Arrow, false),
                (Dotted, Arrow, false),
                (Solid, Cross, false),
                (Dotted, Cross, false),
                (Solid, Async, false),
                (Dotted, Async, false),
                (Solid, Arrow, true),
                (Dotted, Arrow, true),
            ]
        );
        assert_eq!(sd.participants.len(), 2);
    }

    #[test]
    fn participants_with_spaces_aliases_and_kinds() {
        let sd = ok("sequenceDiagram
            participant Edge Function as EF<br>v2
            actor User
            participant DB@{ \"type\": \"database\" }
            User->>+Edge Function: call
            Edge Function-->>-User: done
            participant User as The user");
        let parts: Vec<_> = sd.participants.iter().map(|p| (p.id.as_str(), p.label.as_str(), p.kind)).collect();
        assert_eq!(
            parts,
            [
                ("Edge Function", "EF\nv2", PartKind::Participant),
                ("User", "The user", PartKind::Actor),
                ("DB", "DB", PartKind::Participant)
            ]
        );
        let acts: Vec<_> = sd
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Message { from, to, act, .. } => Some((*from, *to, *act)),
                _ => None,
            })
            .collect();
        assert_eq!(acts, [(1, 0, Some(true)), (0, 1, Some(false))]);
    }

    #[test]
    fn notes_blocks_and_activations() {
        let sd = ok("sequenceDiagram
            autonumber 10 5
            Note right of A: Hi
            note over A,B: Both
            loop Every minute
              A->>B: ping
            end
            alt ok
              B->>A: pong
            else failed
              B-xA: lost
            end
            rect rgb(0, 0, 0)
              activate A
              deactivate A
            end
            box Aqua Group
              participant C
            end
            create participant D
            destroy D
            link A: Docs @ https://example.com");
        assert_eq!(sd.autonumber, Some((10, 5)));
        let kinds: Vec<String> = sd
            .events
            .iter()
            .map(|e| match e {
                Event::Message { label, .. } => format!("msg {label}"),
                Event::Note { at, who, label } => format!("note {} {who:?} {label}", at.name()),
                Event::Open { kind, label } => format!("open {} {label}", kind.name()),
                Event::Divider { kind, label } => format!("divider {} {label}", kind.name()),
                Event::Close => "close".into(),
                Event::Activate(k) => format!("activate {k}"),
                Event::Deactivate(k) => format!("deactivate {k}"),
                Event::Destroy(k) => format!("destroy {k}"),
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "note right of [0] Hi",
                "note over [0, 1] Both",
                "open loop Every minute",
                "msg ping",
                "close",
                "open alt ok",
                "msg pong",
                "divider else failed",
                "msg lost",
                "close",
                "open rect ",
                "activate 0",
                "deactivate 0",
                "close",
                "destroy 3"
            ]
        );
        assert!(sd.simplified.contains("box") && sd.simplified.contains("create"));
        assert_eq!(sd.boxes, [SeqBox { title: "Group".into(), members: vec![2] }]);
    }

    #[test]
    fn entities_and_semicolons_in_messages() {
        let sd = ok("sequenceDiagram
            Alice->>John: I #9829; you #59; really
            John-->>Alice: I #9829; you #infin; times!");
        let labels: Vec<_> = sd
            .events
            .iter()
            .map(|e| match e {
                Event::Message { label, .. } => label.as_str(),
                _ => "",
            })
            .collect();
        assert_eq!(labels, ["I ♥ you ; really", "I ♥ you ∞ times!"]);
    }

    #[test]
    fn accessibility_lines_are_skipped() {
        let sd = ok("sequenceDiagram
            title: Reset
            accTitle: Reset flow
            accDescr {
              The user asks for a reset.
              The site sends a link.
            }
            A->>B: reset");
        assert_eq!(sd.events.len(), 1);
        assert_eq!(sd.title.as_deref(), Some("Reset"));
    }

    #[test]
    fn one_line_acc_descr_with_a_brace_keeps_what_follows() {
        let sd = ok("sequenceDiagram
            accDescr: Maps {key -> value
            A->>B: first
            B->>A: second {ok}
            A->>B: third");
        assert_eq!(sd.events.len(), 3);
        // Without a later `}` the diagram is still read.
        assert_eq!(ok("sequenceDiagram\naccDescr: a {\nA->>B: x").events.len(), 1);
        assert_eq!(ok("sequenceDiagram\naccDescr {\nwords\n}\nA->>B: x").events.len(), 1);
        assert_eq!(ok("sequenceDiagram\naccDescr{ words\nmore }\nA->>B: x").events.len(), 1);
    }

    #[test]
    fn title_is_kept_on_one_line() {
        let title = |head: &str| ok(&format!("sequenceDiagram\n{head}\nA->>B: hi")).title;
        assert_eq!(title("title Login flow").as_deref(), Some("Login flow"));
        assert_eq!(title("title: Login <br> flow #9829;").as_deref(), Some("Login flow ♥"));
        // The last one wins, and an empty one is ignored.
        assert_eq!(title("title One\ntitle Two\ntitle:").as_deref(), Some("Two"));
        assert_eq!(title("%% none"), None);
    }

    #[test]
    fn activate_and_destroy_take_names_with_spaces() {
        let sd = ok("sequenceDiagram
            participant Edge Function
            participant DB
            DB->>Edge Function: call
            activate Edge Function
            Edge Function-->>DB: done
            deactivate  Edge Function
            destroy Edge Function");
        assert_eq!(sd.participants.len(), 2);
        assert_eq!(sd.events[1], Event::Activate(0));
        assert_eq!(sd.events[3], Event::Deactivate(0));
        assert_eq!(sd.events[4], Event::Destroy(0));
        for bad in ["activate A->>B: x", "destroy A, B", "deactivate A:B"] {
            assert_eq!(why(&format!("sequenceDiagram\n{bad}")), format!("unexpected statement \"{bad}\""));
        }
    }

    #[test]
    fn create_remembers_where_it_was() {
        let sd = ok("sequenceDiagram
            A->>B: one
            create participant C as Carol
            A->>C: two");
        let created: Vec<_> = sd.participants.iter().map(|p| p.created).collect();
        assert_eq!(created, [None, None, Some(1)]);
    }

    #[test]
    fn central_connections_and_stray_arrow_marks_fall_back() {
        let bad = [
            "Alice->>()John: Hello John",
            "Alice()->>John: How are you?",
            "John()->>()Alice: Great!",
            "A->>>B: z",
            "A->>+-B: y",
            "A->>++B: y",
            "A-->>-B-: y",
            "A<->>B: x",
            "A->> : x",
            "note over A,: x",
            "note over A+B: x",
        ];
        for st in bad {
            let src = format!("sequenceDiagram\nparticipant Alice\nparticipant John\n{st}");
            assert!(why(&src).starts_with("unexpected statement"), "{st}");
        }
        // Brackets elsewhere in a name, and a `-` inside one, are fine.
        let sd = ok("sequenceDiagram\nWeb (v2)->>Edge-Fn: x\nEdge-Fn-->>-Web (v2): y");
        let ids: Vec<_> = sd.participants.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["Web (v2)", "Edge-Fn"]);
    }

    /// Each event as a short line of text.
    fn events(sd: &SeqDiagram) -> Vec<String> {
        sd.events
            .iter()
            .map(|e| match e {
                Event::Message { from, to, label, .. } => format!("msg {from}>{to} {label}"),
                Event::Note { at, who, label } => format!("note {} {who:?} {label}", at.name()),
                Event::Open { kind, label } => format!("open {} {label}", kind.name()),
                Event::Divider { kind, label } => format!("divider {} {label}", kind.name()),
                Event::Close => "close".into(),
                Event::Activate(k) => format!("activate {k}"),
                Event::Deactivate(k) => format!("deactivate {k}"),
                Event::Destroy(k) => format!("destroy {k}"),
            })
            .collect()
    }

    #[test]
    fn keywords_are_read_in_any_case() {
        let sd = ok("sequenceDiagram
            Title: Upper case
            AccTitle: a
            ACCDESCR {
              b
            }
            Participant A AS Alice
            Actor U
            Box Aqua Team
            PARTICIPANT T
            END
            Autonumber 5 2
            Loop Every day
              A->>U: x
            End
            Alt ok
              U->>A: y
            ELSE bad
              U-xA: z
            end
            PAR_OVER first
              A->>T: p
            AND second
              A->>T: q
            end
            Par one
            end
            CRITICAL c
            OPTION o
            end
            Opt o
            end
            Break b
            end
            RECT rgb(0,0,0)
            end
            Activate A
            DeActivate A
            NOTE Over A: n
            Create Participant C
            Destroy C
            LINK A: Docs @ https://example.com
            Links A: {}
            AUTONUMBER OFF");
        assert_eq!(sd.title.as_deref(), Some("Upper case"));
        let parts: Vec<_> = sd.participants.iter().map(|p| (p.id.as_str(), p.label.as_str(), p.kind)).collect();
        assert_eq!(
            parts,
            [
                ("A", "Alice", PartKind::Participant),
                ("U", "U", PartKind::Actor),
                ("T", "T", PartKind::Participant),
                ("C", "C", PartKind::Participant)
            ]
        );
        assert_eq!(sd.boxes, [SeqBox { title: "Team".into(), members: vec![2] }]);
        assert_eq!(
            events(&sd),
            [
                "open loop Every day",
                "msg 0>1 x",
                "close",
                "open alt ok",
                "msg 1>0 y",
                "divider else bad",
                "msg 1>0 z",
                "close",
                "open par_over first",
                "msg 0>2 p",
                "divider and second",
                "msg 0>2 q",
                "close",
                "open par one",
                "close",
                "open critical c",
                "divider option o",
                "close",
                "open opt o",
                "close",
                "open break b",
                "close",
                "open rect ",
                "close",
                "activate 0",
                "deactivate 0",
                "note over [0] n",
                "destroy 3",
            ]
        );
        let on = Autonumber { start: Some(5), step: Some(2), visible: true };
        let off = Autonumber { start: None, step: None, visible: false };
        assert_eq!(sd.autonumbers, [(0, on), (28, off)]);
        // A participant whose name starts with a keyword is still a name.
        let sd = ok("sequenceDiagram\nparticipant Endpoint\nEndpoint->>Boxer: x\nLooper->>Endpoint: y");
        let ids: Vec<_> = sd.participants.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["Endpoint", "Boxer", "Looper"]);
    }

    #[test]
    fn autonumber_statements_are_kept_in_place() {
        let sd = ok("sequenceDiagram
            A->>B: a
            autonumber
            B->>A: b
            autonumber 10
            autonumber 3 0
            A->>B: c
            autonumber off");
        let at = |start, step| Autonumber { start, step, visible: true };
        let off = Autonumber { start: None, step: None, visible: false };
        assert_eq!(sd.autonumbers, [(1, at(None, None)), (2, at(Some(10), Some(1))), (2, at(Some(3), None)), (3, off)]);
        // The prototype's record: the last numbered statement, 1 for a
        // number left out.
        assert_eq!(sd.autonumber, Some((3, 0)));
        assert_eq!(ok("sequenceDiagram\nautonumber 0\nA->>B: x").autonumbers, [(0, at(None, Some(1)))]);
        for bad in ["autonumber off 5", "autonumber 5 off", "autonumber 1 2 3", "autonumber -1", "autonumber x"] {
            assert!(why(&format!("sequenceDiagram\nA->>B: x\n{bad}")).starts_with("unexpected statement"), "{bad}");
        }
    }

    #[test]
    fn links_take_names_with_spaces() {
        let sd = ok("sequenceDiagram
            participant Edge Function
            Edge Function->>B: x
            link Edge Function: Docs @ https://example.com
            links Edge Function: {\"Dashboard\": \"https://example.com\"}
            link Nobody here: Docs @ https://example.com");
        assert_eq!(sd.participants.len(), 2);
        assert_eq!(events(&sd), ["msg 0>1 x"]);
        for bad in ["link A->>B: x", "link A, B: x", "links +A: x"] {
            assert_eq!(why(&format!("sequenceDiagram\nA->>B: x\n{bad}")), format!("unexpected statement \"{bad}\""));
        }
    }

    #[test]
    fn wrap_settings_are_not_text() {
        let sd = ok("sequenceDiagram
            participant C as wrap: Carol
            create participant D as nowrap:Dave
            A->>B: wrap: a long message
            A->>C: :wrap: colon form
            A->>D:nowrap:tight
            Note over A: nowrap: note
            loop wrap: every minute
              A->>B: x
            else :nowrap: other
            end
            box wrap: Team
            end
            A->>B: Wrap: kept
            A->>B: unwrap: kept
            A->>B: : kept");
        let labels: Vec<_> = sd.participants.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, ["Carol", "Dave", "A", "B"]);
        assert_eq!(
            events(&sd),
            [
                "msg 2>3 a long message",
                "msg 2>0 colon form",
                "msg 2>1 tight",
                "note over [2] note",
                "open loop every minute",
                "msg 2>3 x",
                "divider else other",
                "close",
                "msg 2>3 Wrap: kept",
                "msg 2>3 unwrap: kept",
                "msg 2>3 : kept",
            ]
        );
        assert_eq!(sd.boxes[0].title, "Team");
        // The title keeps it, as in Mermaid.
        assert_eq!(ok("sequenceDiagram\ntitle wrap: T\nA->>B: x").title.as_deref(), Some("wrap: T"));
    }

    #[test]
    fn box_titles_drop_a_colour_and_keep_members() {
        let title = |rest: &str| ok(&format!("sequenceDiagram\nbox {rest}\nend\nA->>B: x")).boxes[0].title.clone();
        assert_eq!(title("Aqua Frontend services"), "Frontend services");
        assert_eq!(title("AQUA Frontend"), "Frontend");
        assert_eq!(title("transparent Aqua"), "Aqua");
        assert_eq!(title("rgb(33,66,99) Group"), "Group");
        assert_eq!(title("rgba(33, 66, 99, 0.5) Group"), "Group");
        assert_eq!(title("hsl(120deg 50% 50%) Group"), "Group");
        assert_eq!(title("Aqua"), "");
        assert_eq!(title("Background jobs"), "jobs");
        // Not a colour: the whole text is the title.
        assert_eq!(title("Backend"), "Backend");
        assert_eq!(title("Frontend services"), "Frontend services");
        assert_eq!(title("Aquamarines Group"), "Aquamarines Group");
        assert_eq!(title("rgb (1,2,3) Group"), "rgb (1,2,3) Group");
        assert_eq!(title("RGB(1,2,3) Group"), "RGB(1,2,3) Group");
        assert_eq!(title("rgb(1,2,3) Payments (EU)"), "rgb(1,2,3) Payments (EU)");
        assert_eq!(title("2024 Plan"), "2024 Plan");
        assert_eq!(title("Blue Team #9829;"), "Team ♥");
        let sd = ok("sequenceDiagram
            participant X
            box One
            participant A
            participant X
            create actor C
            participant A
            end
            box Two
            participant B
            end
            participant A
            A->>B: x");
        let members: Vec<_> = sd.boxes.iter().map(|b| (b.title.as_str(), b.members.clone())).collect();
        assert_eq!(members, [("One", vec![1, 0, 2]), ("Two", vec![3])]);
        // Mermaid rejects a participant declared in two boxes.
        let src = "sequenceDiagram\nbox One\nparticipant A\nend\nbox Two\nparticipant A\nend";
        assert_eq!(why(src), "\"A\" is in two boxes");
    }

    #[test]
    fn errors() {
        assert_eq!(why("sequenceDiagram\nelse x"), "\"else\" outside a block");
        assert_eq!(why("sequenceDiagram\nbox\nand x\nend"), "\"and\" outside a block");
        assert_eq!(why("sequenceDiagram\nA->>B: x\nend"), "\"end\" without a block");
        assert_eq!(why("sequenceDiagram\nloop\nA->>B: x"), "block without \"end\"");
        assert_eq!(why("sequenceDiagram\nA->>B hello"), "unexpected statement \"A->>B hello\"");
        assert_eq!(why("sequenceDiagram\nautonumber\ntitle T"), "nothing to draw");
        assert_eq!(why("sequenceDiagram"), "empty diagram");
    }

    #[test]
    fn odd_input_never_panics() {
        let odd = ["", ":", "->>", "A->>: x", "note over : x", "note over A,: x", "participant", "end", "box", "loop"];
        let more = ["autonumber 99999999999", "A<<->>", "A->>+-B: x", "else", "activate  ", "destroy :", "title"];
        let more = more.into_iter().chain(["title:", "accDescr: {", "create participant", "note over ,: x"]);
        let boxes = ["box rgb(", "box rgb()", "box rgb(1,2,3", "box hsla() x", "box (", "box é", "box #fff x"];
        let rest = ["autonumber off 5", "link :", "links", "link  : x", "par_over", "Box", "END", "TITLE", "accdescr"];
        let rest = rest.into_iter().chain(["ACCDESCR{", "Titleé", "accDescré", ":wrap:", "A->>B: wrap:", "A->>B: :"]);
        for s in odd.into_iter().chain(more).chain(["()->>(): x", "->>B: x"]).chain(boxes).chain(rest) {
            let _ = parse("sequenceDiagram", &[s.to_string()]);
            let _ = parse(&format!("sequenceDiagram {s}"), &[]);
        }
    }
}

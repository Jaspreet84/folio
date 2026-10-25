// Reads Sourcegraph SSE dumps as data, extracts complete ```mermaid blocks,
// renders flowcharts and sequence diagrams with merman-ascii, prints stats.
use merman::ascii::{AsciiLayoutProfile, AsciiRenderOptions, AsciiViewportPolicy, OverflowPolicy};
use merman::{AsciiRequest, OperationControl, RenderOutput, RenderRequest, Renderer};
use std::collections::{BTreeMap, HashSet};
use std::io::BufRead;

fn width(s: &str) -> usize {
    // rough: count chars, CJK as 2
    s.trim_end().chars().map(|c| if (c as u32) >= 0x1100 && ((c as u32) >= 0x2E80 && (c as u32) <= 0xFFDC || (c as u32) >= 0x20000) { 2 } else { 1 }).sum()
}

fn kind(src: &str) -> &'static str {
    let mut in_fm = false;
    for (i, l) in src.lines().enumerate() {
        let t = l.trim();
        if t.is_empty() { continue; }
        if i == 0 || !in_fm { if t == "---" && !in_fm { in_fm = true; continue; } }
        if in_fm { if t == "---" { in_fm = false; } continue; }
        if t.starts_with("%%") { continue; }
        let w = t.split(|c: char| c.is_whitespace() || c == ';').next().unwrap_or("");
        return match w { "flowchart" | "graph" => "flow", "sequenceDiagram" => "seq", _ => "other" };
    }
    "other"
}

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    let mut blocks: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for p in &paths {
        let f = std::fs::File::open(p).unwrap();
        let mut ev = String::new();
        for line in std::io::BufReader::new(f).lines() {
            let Ok(line) = line else { continue };
            if let Some(e) = line.strip_prefix("event: ") { ev = e.to_string(); continue; }
            let Some(d) = line.strip_prefix("data: ") else { continue };
            if ev != "matches" { continue; }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(d) else { continue };
            let Some(arr) = v.as_array() else { continue };
            for m in arr {
                let mut contents: Vec<String> = Vec::new();
                for cm in m.get("chunkMatches").and_then(|x| x.as_array()).into_iter().flatten() {
                    contents.push(cm.get("content").and_then(|x| x.as_str()).unwrap_or("").to_string());
                }
                if let Some(lms) = m.get("lineMatches").and_then(|x| x.as_array()) {
                    let mut v: Vec<(u64, String)> = lms.iter().map(|lm| (lm.get("lineNumber").and_then(|x| x.as_u64()).unwrap_or(0), lm.get("line").and_then(|x| x.as_str()).unwrap_or("").to_string())).collect();
                    v.sort_by_key(|x| x.0);
                    let mut out = String::new();
                    let mut prev: Option<u64> = None;
                    for (ln, l) in v {
                        if let Some(p) = prev { if ln != p + 1 { out.push_str("\n\u{0}\n"); } else { out.push('\n'); } }
                        out.push_str(&l);
                        prev = Some(ln);
                    }
                    contents.push(out);
                }
                for c in &contents {
                    let c = c.as_str();
                    let mut rest = c;
                    while let Some(i) = rest.find("```mermaid") {
                        let after = &rest[i + 10..];
                        let Some(nl) = after.find('\n') else { break };
                        if !after[..nl].trim().is_empty() { rest = &after[nl..]; continue; }
                        let body = &after[nl + 1..];
                        let Some(end) = body.find("\n```").or_else(|| body.find("```")) else { break };
                        let b = body[..end].to_string(); if b.contains('\u{0}') { rest = &body[end..]; if rest.len() > 3 { rest = &rest[3..]; continue; } else { break; } }
                        // strip common indent
                        let ind = b.lines().filter(|l| !l.trim().is_empty()).map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0);
                        let b: String = b.lines().map(|l| if l.len() >= ind { &l[ind..] } else { l.trim_start() }).collect::<Vec<_>>().join("\n");
                        if seen.insert(b.clone()) { blocks.push(b); }
                        rest = &body[end..];
                        if rest.len() > 3 { rest = &rest[3..]; } else { break; }
                    }
                }
            }
        }
    }
    let r = Renderer::new().with_parse_options(merman::ParseOptions::strict());
    let mut stats: BTreeMap<String, usize> = BTreeMap::new();
    let mut errs: BTreeMap<String, usize> = BTreeMap::new();
    let mut n = 0;
    let limit: usize = std::env::var("LIMIT").ok().and_then(|x| x.parse().ok()).unwrap_or(2000);
    let mut times = Vec::new();
    for b in &blocks {
        let k = kind(b);
        if k == "other" { continue; }
        n += 1;
        if n > limit { break; }
        let mut best: Option<(usize, usize)> = None;
        let mut err: Option<String> = None;
        let is_lr = k == "flow" && b.lines().any(|l| { let t = l.trim(); (t.starts_with("flowchart") || t.starts_with("graph")) && (t.contains(" LR") || t.contains(" RL")) });
        if is_lr { *stats.entry("flow LR/RL".into()).or_default() += 1; }
        let td: String = if is_lr { b.lines().map(|l| { let t = l.trim(); if (t.starts_with("flowchart") || t.starts_with("graph")) { l.replacen(" LR", " TD", 1).replacen(" RL", " TD", 1) } else { l.to_string() } }).collect::<Vec<_>>().join("\n") } else { String::new() };
        let variants: Vec<(&str, AsciiLayoutProfile)> = if is_lr { vec![(b.as_str(), AsciiLayoutProfile::Canonical), (b.as_str(), AsciiLayoutProfile::Compact), (td.as_str(), AsciiLayoutProfile::Compact)] } else { vec![(b.as_str(), AsciiLayoutProfile::Canonical), (b.as_str(), AsciiLayoutProfile::Compact)] };
        for (src, prof) in variants {
            let b = src;
            let opts = AsciiRenderOptions::unicode().with_node_padding_y(0).with_layout_profile(prof);
            let t = std::time::Instant::now();
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r.render(RenderRequest::ascii(b, OperationControl::new(), AsciiRequest {
                options: opts,
                viewport: AsciiViewportPolicy::unrestricted().overflow(OverflowPolicy::Allow),
                ..Default::default()
            }))));
            times.push(t.elapsed().as_micros());
            match res {
                Ok(Ok(RenderOutput::Ascii(Some(rep)))) => {
                    let w = rep.text.lines().map(width).max().unwrap_or(0);
                    let h = rep.text.lines().count();
                    if best.is_none_or(|(bw, _)| w < bw) { best = Some((w, h)); }
                }
                Ok(Ok(_)) => { err.get_or_insert("no diagram".into()); }
                Ok(Err(e)) => {
                    let s = e.to_string();
                    let key = if s.contains("does not support") { s.split('`').nth(1).unwrap_or("?").to_string() } else { s.chars().take(50).collect() };
                    err.get_or_insert(key);
                }
                Err(_) => { err.get_or_insert("PANIC".into()); }
            }
        }
        let cat = match best {
            Some((w, h)) => {
                let wb = if w <= 34 { "<=34" } else if w <= 74 { "35-74" } else if w <= 96 { "75-96" } else { ">96" };
                let hb = if h <= 24 { "h<=24" } else if h <= 48 { "h25-48" } else { "h>48" };
                *stats.entry(format!("{k} drawn w{wb}")).or_default() += 1;
                *stats.entry(format!("{k} drawn {hb}")).or_default() += 1;
                format!("{k} drawn")
            }
            None => {
                *errs.entry(format!("{k}: {}", err.clone().unwrap_or_default())).or_default() += 1;
                format!("{k} failed")
            }
        };
        *stats.entry(cat).or_default() += 1;
        *stats.entry(format!("{k} total")).or_default() += 1;
    }
    times.sort();
    println!("unique blocks: {}, flow+seq rendered: {}", blocks.len(), n.min(limit));
    for (k, v) in &stats { println!("{k:28} {v}"); }
    println!("--- failure reasons");
    let mut ev: Vec<_> = errs.into_iter().collect();
    ev.sort_by(|a, b| b.1.cmp(&a.1));
    for (k, v) in ev.iter().take(15) { println!("{v:5}  {k}"); }
    if !times.is_empty() {
        println!("render us p50={} p95={} p99={} max={}", times[times.len()/2], times[times.len()*95/100], times[times.len()*99/100], times[times.len()-1]);
    }
}

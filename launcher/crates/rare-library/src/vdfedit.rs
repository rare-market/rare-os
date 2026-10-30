//! Byte-preserving edits of Valve text KeyValues files (config.vdf, localconfig.vdf).
//!
//! `vdf::parse` throws formatting away, so writing its tree back would reshuffle a file Steam owns.
//! This module parses with spans instead and splices the changed bytes only: everything else in the
//! file (comments, indentation, key order, odd escapes) comes back identical. Every edit is checked
//! before it is returned: the result must parse strictly, the target must read back, and every other
//! key and value must be unchanged. A file that does not parse strictly is never edited.
//!
//! Keys match case-insensitively, as Steam does.

use std::ops::Range;

#[derive(Debug)]
struct Node {
    key: String,
    /// Bytes of the whole `"key" value` entry, from the key's first quote to the value's last byte.
    span: Range<usize>,
    val: Val,
}

#[derive(Debug)]
enum Val {
    /// Decoded text and the span of the value token, quotes included.
    Str(String, Range<usize>),
    /// Position of `{`, of `}`, and the children.
    Obj { close: usize, kids: Vec<Node> },
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

enum Tok {
    Str(String, Range<usize>),
    Open,
    Close(usize),
}

impl P<'_> {
    fn ws(&mut self) {
        loop {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.s[self.i..].starts_with(b"//") {
                while self.i < self.s.len() && self.s[self.i] != b'\n' {
                    self.i += 1;
                }
                continue;
            }
            break;
        }
    }

    fn tok(&mut self) -> Result<Option<Tok>, String> {
        self.ws();
        if self.i >= self.s.len() {
            return Ok(None);
        }
        let start = self.i;
        Ok(Some(match self.s[self.i] {
            b'{' => {
                self.i += 1;
                Tok::Open
            }
            b'}' => {
                self.i += 1;
                Tok::Close(start)
            }
            b'"' => {
                self.i += 1;
                let mut out = Vec::new();
                loop {
                    let Some(&c) = self.s.get(self.i) else { return Err("unterminated string".into()) };
                    match c {
                        b'"' => break,
                        b'\\' => {
                            let Some(&e) = self.s.get(self.i + 1) else { return Err("unterminated string".into()) };
                            out.push(match e {
                                b'n' => b'\n',
                                b't' => b'\t',
                                c => c,
                            });
                            self.i += 2;
                        }
                        c => {
                            out.push(c);
                            self.i += 1;
                        }
                    }
                }
                self.i += 1;
                Tok::Str(String::from_utf8(out).map_err(|_| "not UTF-8".to_string())?, start..self.i)
            }
            _ => {
                while self.i < self.s.len() && !self.s[self.i].is_ascii_whitespace() && !matches!(self.s[self.i], b'{' | b'}' | b'"') {
                    self.i += 1;
                }
                Tok::Str(String::from_utf8_lossy(&self.s[start..self.i]).into_owned(), start..self.i)
            }
        }))
    }

    fn obj(&mut self, top: bool) -> Result<(Vec<Node>, usize), String> {
        let mut kids = Vec::new();
        loop {
            let (key, kspan) = match self.tok()? {
                Some(Tok::Str(k, sp)) => (k, sp),
                Some(Tok::Close(at)) if !top => return Ok((kids, at)),
                Some(_) => return Err(format!("unexpected brace at byte {}", self.i - 1)),
                None if top => return Ok((kids, self.s.len())),
                None => return Err("missing closing brace".into()),
            };
            match self.tok()? {
                Some(Tok::Str(v, vsp)) => kids.push(Node { key, span: kspan.start..vsp.end, val: Val::Str(v, vsp) }),
                Some(Tok::Open) => {
                    let (sub, close) = self.obj(false)?;
                    kids.push(Node { key, span: kspan.start..close + 1, val: Val::Obj { close, kids: sub } });
                }
                _ => return Err(format!("key {key:?} has no value")),
            }
        }
    }
}

fn parse_strict(text: &str) -> Result<Vec<Node>, String> {
    let mut p = P { s: text.as_bytes(), i: 0 };
    Ok(p.obj(true)?.0)
}

fn find<'a>(kids: &'a [Node], key: &str) -> Option<&'a Node> {
    kids.iter().find(|n| n.key.eq_ignore_ascii_case(key))
}

fn walk<'a>(mut kids: &'a [Node], path: &[&str]) -> Option<&'a Node> {
    let mut node = None;
    for (i, k) in path.iter().enumerate() {
        let n = find(kids, k)?;
        if i + 1 < path.len() {
            match &n.val {
                Val::Obj { kids: sub, .. } => kids = sub,
                Val::Str(..) => return None,
            }
        }
        node = Some(n);
    }
    node
}

/// Every string leaf as ("a/b/c" lowercase path, value), in file order.
fn flatten(kids: &[Node], prefix: &str, out: &mut Vec<(String, String)>) {
    for n in kids {
        let p = format!("{prefix}/{}", n.key.to_ascii_lowercase());
        match &n.val {
            Val::Str(v, _) => out.push((p, v.clone())),
            Val::Obj { kids, .. } => {
                out.push((p.clone(), "{".into()));
                flatten(kids, &p, out);
            }
        }
    }
}

fn flat(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    flatten(&parse_strict(text)?, "", &mut out);
    Ok(out)
}

pub fn escape(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\t', "\\t")
}

/// The string at `path`, `None` when absent or when it is a block.
pub fn get(text: &str, path: &[&str]) -> Result<Option<String>, String> {
    let kids = parse_strict(text)?;
    Ok(match walk(&kids, path).map(|n| &n.val) {
        Some(Val::Str(v, _)) => Some(v.clone()),
        _ => None,
    })
}

/// True when `path` names a block or a string.
pub fn has(text: &str, path: &[&str]) -> Result<bool, String> {
    Ok(walk(&parse_strict(text)?, path).is_some())
}

/// Names of the entries inside the block at `path` (empty when absent).
pub fn keys(text: &str, path: &[&str]) -> Result<Vec<String>, String> {
    let kids = parse_strict(text)?;
    Ok(match walk(&kids, path).map(|n| &n.val) {
        Some(Val::Obj { kids, .. }) => kids.iter().map(|n| n.key.clone()).collect(),
        _ => Vec::new(),
    })
}

fn line_indent(text: &str, at: usize) -> (usize, String, bool) {
    let ls = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let pre = &text[ls..at];
    (ls, pre.chars().take_while(|c| *c == ' ' || *c == '\t').collect(), pre.trim().is_empty())
}

fn block_text(path_tail: &[&str], value: &str, indent: &str, nl: &str) -> String {
    // path_tail = [block, block, ..., key]
    let mut out = String::new();
    let mut ind = indent.to_string();
    for b in &path_tail[..path_tail.len() - 1] {
        out += &format!("{ind}\"{}\"{nl}{ind}{{{nl}", escape(b));
        ind.push('\t');
    }
    out += &format!("{ind}\"{}\"\t\t\"{}\"{nl}", escape(path_tail[path_tail.len() - 1]), escape(value));
    for _ in 0..path_tail.len() - 1 {
        ind.pop();
        out += &format!("{ind}}}{nl}");
    }
    out
}

/// Sets the string at `path`, creating missing blocks along the way. Errors when `path` names a
/// block, the file is malformed, or the checked result differs from the file anywhere else.
pub fn set(text: &str, path: &[&str], value: &str) -> Result<String, String> {
    if path.is_empty() {
        return Err("empty path".into());
    }
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let kids = parse_strict(text)?;
    // Deepest existing block along the path.
    let mut level: &[Node] = &kids;
    let mut depth = 0;
    let mut close_at: Option<usize> = None;
    let mut out = None;
    while depth < path.len() {
        let Some(n) = find(level, path[depth]) else { break };
        match &n.val {
            Val::Str(_, vs) if depth + 1 == path.len() => {
                let mut s = text.to_string();
                s.replace_range(vs.clone(), &format!("\"{}\"", escape(value)));
                out = Some(s);
                break;
            }
            Val::Str(..) => return Err(format!("{:?} is a value, not a block", path[depth])),
            Val::Obj { .. } if depth + 1 == path.len() => return Err(format!("{:?} is a block", path[depth])),
            Val::Obj { close, kids: sub } => {
                close_at = Some(*close);
                level = sub;
                depth += 1;
            }
        }
    }
    let new = match out {
        Some(s) => s,
        None => {
            let mut s = text.to_string();
            match close_at {
                Some(close) => {
                    let (ls, indent, own_line) = line_indent(text, close);
                    let child = format!("{indent}\t");
                    if own_line {
                        s.insert_str(ls, &block_text(&path[depth..], value, &child, nl));
                    } else {
                        let ins = format!("{nl}{}{indent}", block_text(&path[depth..], value, &child, nl));
                        s.insert_str(close, &ins);
                    }
                }
                None => {
                    if !s.is_empty() && !s.ends_with('\n') {
                        s.push_str(nl);
                    }
                    s.push_str(&block_text(&path[depth..], value, "", nl));
                }
            }
            s
        }
    };
    check(text, &new, path, Some(value))?;
    Ok(new)
}

/// Removes the entry at `path` (a string or a whole block). No-op when it is absent.
pub fn remove(text: &str, path: &[&str]) -> Result<String, String> {
    let kids = parse_strict(text)?;
    let Some(n) = walk(&kids, path) else { return Ok(text.to_string()) };
    let (mut a, mut b) = (n.span.start, n.span.end);
    let (ls, _, own) = line_indent(text, a);
    if own {
        a = ls;
        let rest = &text[b..];
        let eol = rest.find('\n').map_or(rest.len(), |i| i + 1);
        if rest[..eol].trim().is_empty() {
            b += eol;
        }
    }
    let mut s = text.to_string();
    s.replace_range(a..b, "");
    check(text, &s, path, None)?;
    Ok(s)
}

/// The edit must leave a strictly parseable file in which `path` reads `want` and every other
/// entry is as before.
fn check(before: &str, after: &str, path: &[&str], want: Option<&str>) -> Result<(), String> {
    let mut a = flat(before)?;
    let b = flat(after)?;
    let target = format!("/{}", path.iter().map(|p| p.to_ascii_lowercase()).collect::<Vec<_>>().join("/"));
    let under = |p: &str| p == target || p.starts_with(&format!("{target}/"));
    a.retain(|(p, _)| !under(p));
    let mut rest: Vec<(String, String)> = b.iter().filter(|(p, _)| !under(p)).cloned().collect();
    // Parents created by the edit are new "{" entries on the way to the target.
    let mut parent = String::new();
    for p in &path[..path.len() - 1] {
        parent = format!("{parent}/{}", p.to_ascii_lowercase());
        if !a.iter().any(|(q, _)| *q == parent) {
            a.push((parent.clone(), "{".into()));
        }
    }
    let norm = |v: &mut Vec<(String, String)>| v.sort();
    norm(&mut a);
    norm(&mut rest);
    if a != rest {
        return Err("the edit would change other entries; refusing".into());
    }
    let got = get(after, path)?;
    match (want, got) {
        (Some(w), Some(g)) if w == g => Ok(()),
        (None, None) => Ok(()),
        _ => Err("the edit did not read back; refusing".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "\"InstallConfigStore\"\r\n{\r\n\t\"Software\"\r\n\t{\r\n\t\t\"Valve\"\r\n\t\t{\r\n\t\t\t\"Steam\"\r\n\t\t\t{\r\n\t\t\t\t\"AutoUpdateWindowEnabled\"\t\t\"0\"\r\n\t\t\t\t// kept\r\n\t\t\t\t\"ShaderCacheManager\"\r\n\t\t\t\t{\r\n\t\t\t\t\t\"HasCurrentBucket\"\t\t\"1\"\r\n\t\t\t\t\t\"CurrentBucketGPU\"\t\t\"RADV;PHOENIX\"\r\n\t\t\t\t}\r\n\t\t\t\t\"Weird\"\t\t\"a \\\"quoted\\\" \\\\ path\"\r\n\t\t\t}\r\n\t\t}\r\n\t}\r\n}\r\n";
    const STEAM: [&str; 4] = ["InstallConfigStore", "Software", "Valve", "Steam"];

    fn p(extra: &[&'static str]) -> Vec<&'static str> {
        let mut v: Vec<&str> = STEAM.to_vec();
        v.extend_from_slice(extra);
        v
    }

    #[test]
    fn adds_a_nested_block_and_leaves_every_other_byte() {
        let path = p(&["CompatToolMapping", "0", "name"]);
        let out = set(CONFIG, &path, "proton_11").unwrap();
        assert_eq!(get(&out, &path).unwrap().as_deref(), Some("proton_11"));
        // Everything before and after the insertion is byte-identical, CRLF kept.
        let ins_at = CONFIG.find("\t\t\t}\r\n\t\t}\r\n\t}\r\n}").unwrap();
        assert!(out.starts_with(&CONFIG[..ins_at]));
        assert!(out.ends_with(&CONFIG[ins_at..]));
        assert!(out[ins_at..].contains("\"CompatToolMapping\"\r\n"));
        assert!(!out[ins_at..out.len() - (CONFIG.len() - ins_at)].contains("\r\r"));
        assert!(!out.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn round_trips_unrelated_values_and_escapes() {
        let out = set(CONFIG, &p(&["ShaderCacheManager", "EnableShaderBackgroundProcessing"]), "0").unwrap();
        assert_eq!(get(&out, &p(&["Weird"])).unwrap().as_deref(), Some("a \"quoted\" \\ path"));
        assert_eq!(get(&out, &p(&["ShaderCacheManager", "CurrentBucketGPU"])).unwrap().as_deref(), Some("RADV;PHOENIX"));
        assert!(out.contains("// kept"));
        // Setting the same thing again is a value replacement, not a second key.
        let again = set(&out, &p(&["ShaderCacheManager", "EnableShaderBackgroundProcessing"]), "1").unwrap();
        assert_eq!(again.matches("EnableShaderBackgroundProcessing").count(), 1);
        assert_eq!(get(&again, &p(&["ShaderCacheManager", "EnableShaderBackgroundProcessing"])).unwrap().as_deref(), Some("1"));
        // Undo gives the original file back byte for byte.
        let gone = remove(&out, &p(&["ShaderCacheManager", "EnableShaderBackgroundProcessing"])).unwrap();
        assert_eq!(gone, CONFIG);
    }

    #[test]
    fn case_insensitive_keys_and_lf_files() {
        let t = "\"a\"\n{\n\t\"B\"\t\"1\"\n}\n";
        let out = set(t, &["A", "b"], "2").unwrap();
        assert_eq!(out, "\"a\"\n{\n\t\"B\"\t\"2\"\n}\n");
        let out = set(t, &["a", "c", "d"], "x").unwrap();
        assert_eq!(out, "\"a\"\n{\n\t\"B\"\t\"1\"\n\t\"c\"\n\t{\n\t\t\"d\"\t\t\"x\"\n\t}\n}\n");
    }

    #[test]
    fn single_line_blocks_and_missing_root() {
        let out = set("\"a\" { \"b\" \"1\" }", &["a", "c"], "2").unwrap();
        assert!(get(&out, &["a", "c"]).unwrap().as_deref() == Some("2") && get(&out, &["a", "b"]).unwrap().as_deref() == Some("1"));
        let out = set("", &["Registry", "k"], "v").unwrap();
        assert_eq!(out, "\"Registry\"\n{\n\t\"k\"\t\t\"v\"\n}\n");
    }

    #[test]
    fn values_with_quotes_are_escaped() {
        let out = set("\"a\"\n{\n}\n", &["a", "k"], "x\"y\\z").unwrap();
        assert_eq!(get(&out, &["a", "k"]).unwrap().as_deref(), Some("x\"y\\z"));
    }

    #[test]
    fn malformed_files_are_refused_untouched() {
        for bad in ["\"a\"\n{\n\t\"b\"\t\"1\"\n", "\"a\"\n{\n\t\"b\"\n}\n", "\"a\" }", "\"a\"\n{ \"b\" \"unterminated }\n"] {
            assert!(set(bad, &["a", "c"], "1").is_err(), "{bad:?}");
        }
        assert!(set(CONFIG, &p(&["ShaderCacheManager"]), "1").is_err(), "a block is not a string");
        assert!(set(CONFIG, &p(&["Weird", "x"]), "1").is_err(), "a value is not a block");
    }

    #[test]
    fn removes_whole_blocks_and_ignores_missing_paths() {
        let with = set(CONFIG, &p(&["CompatToolMapping", "0", "name"]), "proton_11").unwrap();
        let with = set(&with, &p(&["CompatToolMapping", "0", "priority"]), "75").unwrap();
        assert_eq!(keys(&with, &p(&["CompatToolMapping", "0"])).unwrap(), ["name", "priority"]);
        let gone = remove(&with, &p(&["CompatToolMapping"])).unwrap();
        assert_eq!(gone, CONFIG);
        assert_eq!(remove(CONFIG, &p(&["Nope"])).unwrap(), CONFIG);
    }

    /// RARE_VDF_FIXTURE=/path/config.vdf cargo test real_file -- --ignored
    #[test]
    #[ignore]
    fn real_file() {
        let t = std::fs::read_to_string(std::env::var("RARE_VDF_FIXTURE").unwrap()).unwrap();
        let out = set(&t, &["InstallConfigStore", "Software", "Valve", "Steam", "CompatToolMapping", "0", "name"], "proton_11").unwrap();
        let out = set(&out, &["InstallConfigStore", "Software", "Valve", "Steam", "ShaderCacheManager", "EnableShaderBackgroundProcessing"], "0").unwrap();
        let back = remove(&remove(&out, &["InstallConfigStore", "Software", "Valve", "Steam", "CompatToolMapping"]).unwrap(), &["InstallConfigStore", "Software", "Valve", "Steam", "ShaderCacheManager", "EnableShaderBackgroundProcessing"]).unwrap();
        assert_eq!(back, t);
        assert!(out.len() > t.len());
    }

    #[test]
    fn real_world_shapes_survive() {
        // Quoted keys holding long ids, nested app tables, unquoted tokens and a trailing comment.
        let t = "\"InstallConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"ShaderCacheManager\"\n\t\t\t\t{\n\t\t\t\t\t\"App\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"10190\"\n\t\t\t\t\t\t{\n\t\t\t\t\t\t\t\"ShaderCacheSize\"\t\t\"41018430\"\n\t\t\t\t\t\t}\n\t\t\t\t\t}\n\t\t\t\t\t\"4628710\"\t\t\"25833067;G7:VulkanPipelinesV6_923fa87291dfec1c\"\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n} // eof";
        let out = set(t, &p(&["CompatToolMapping", "1145360", "name"]), "GE-Proton11-11").unwrap();
        let out = set(&out, &p(&["CompatToolMapping", "1145360", "priority"]), "250").unwrap();
        assert!(out.ends_with("} // eof"));
        assert_eq!(get(&out, &p(&["ShaderCacheManager", "App", "10190", "ShaderCacheSize"])).unwrap().as_deref(), Some("41018430"));
        assert_eq!(keys(&out, &p(&["CompatToolMapping"])).unwrap(), ["1145360"]);
    }
}

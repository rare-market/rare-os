//! Minimal parser for Valve's text KeyValues format (libraryfolders.vdf, appmanifest_*.acf).
//! Keys are matched case-insensitively, as Steam itself does.

#[derive(Debug, Clone, PartialEq)]
pub enum Vdf {
    Str(String),
    Obj(Vec<(String, Vdf)>),
}

impl Vdf {
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Obj(items) => items.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            Vdf::Str(_) => None,
        }
    }
    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Vdf::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }
    pub fn entries(&self) -> &[(String, Vdf)] {
        match self {
            Vdf::Obj(items) => items,
            Vdf::Str(_) => &[],
        }
    }
}

struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
}

#[derive(Debug, PartialEq)]
enum Tok {
    Str(String),
    Open,
    Close,
}

impl<'a> Lexer<'a> {
    fn next(&mut self) -> Option<Tok> {
        loop {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.i + 1 < self.s.len() && self.s[self.i] == b'/' && self.s[self.i + 1] == b'/' {
                while self.i < self.s.len() && self.s[self.i] != b'\n' {
                    self.i += 1;
                }
                continue;
            }
            break;
        }
        if self.i >= self.s.len() {
            return None;
        }
        match self.s[self.i] {
            b'{' => {
                self.i += 1;
                Some(Tok::Open)
            }
            b'}' => {
                self.i += 1;
                Some(Tok::Close)
            }
            b'"' => {
                self.i += 1;
                let mut out = Vec::new();
                while self.i < self.s.len() && self.s[self.i] != b'"' {
                    if self.s[self.i] == b'\\' && self.i + 1 < self.s.len() {
                        self.i += 1;
                        out.push(match self.s[self.i] {
                            b'n' => b'\n',
                            b't' => b'\t',
                            c => c,
                        });
                    } else {
                        out.push(self.s[self.i]);
                    }
                    self.i += 1;
                }
                self.i += 1;
                Some(Tok::Str(String::from_utf8_lossy(&out).into_owned()))
            }
            _ => {
                // Unquoted token (rare in Steam files, but legal).
                let start = self.i;
                while self.i < self.s.len() && !self.s[self.i].is_ascii_whitespace() && !matches!(self.s[self.i], b'{' | b'}' | b'"') {
                    self.i += 1;
                }
                Some(Tok::Str(String::from_utf8_lossy(&self.s[start..self.i]).into_owned()))
            }
        }
    }
}

fn parse_obj(lx: &mut Lexer, top: bool) -> Vdf {
    let mut items = Vec::new();
    loop {
        let key = match lx.next() {
            Some(Tok::Str(k)) => k,
            Some(Tok::Close) if !top => break,
            Some(_) => continue,
            None => break,
        };
        match lx.next() {
            Some(Tok::Str(v)) => items.push((key, Vdf::Str(v))),
            Some(Tok::Open) => items.push((key, parse_obj(lx, false))),
            _ => break,
        }
    }
    Vdf::Obj(items)
}

/// Parses a whole file. Never fails: malformed input yields whatever parsed cleanly.
pub fn parse(text: &str) -> Vdf {
    let mut lx = Lexer { s: text.as_bytes(), i: 0 };
    parse_obj(&mut lx, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_library_folders() {
        let t = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/deck/.local/share/Steam"
		"apps" { "228980" "12345" "1145360" "678" }
	}
	"1" { "path" "/run/media/deck/SD\\Games" } // comment
}"#;
        let v = parse(t);
        let lf = v.get("LibraryFolders").unwrap();
        assert_eq!(lf.get("0").unwrap().str("path"), Some("/home/deck/.local/share/Steam"));
        assert_eq!(lf.get("1").unwrap().str("path"), Some("/run/media/deck/SD\\Games"));
        assert_eq!(lf.get("0").unwrap().get("apps").unwrap().entries().len(), 2);
    }

    #[test]
    fn parses_manifest() {
        let t = "\"AppState\"\n{\n\"appid\" \"1145360\"\n\"name\" \"Hades\"\n\"installdir\" \"Hades\"\n\"SizeOnDisk\" \"15000000000\"\n\"LastPlayed\" \"1729900000\"\n}";
        let v = parse(t);
        let a = v.get("AppState").unwrap();
        assert_eq!(a.str("name"), Some("Hades"));
        assert_eq!(a.str("APPID"), Some("1145360"));
    }
}

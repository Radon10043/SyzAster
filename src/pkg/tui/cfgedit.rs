//! Format-preserving editor for syz-manager JSON configs.
//!
//! The TUI toggles single options (e.g. `cover`) in a user's config file.
//! Re-serializing the whole document would reorder keys and drop comments,
//! so this module keeps the original text and splices new values into the
//! byte spans recorded while parsing. Like syzkaller's loader, lines whose
//! first non-blank character is `#` are treated as comments.

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Object(Vec<Member>),
    Array(Vec<Node>),
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    pub key: String,
    pub key_start: usize,
    pub value: Node,
}

#[derive(Debug, Clone)]
pub struct JsonDoc {
    text: String,
    root: Node,
}

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn err(&self, msg: &str) -> String {
        let before = &self.s[..self.pos.min(self.s.len())];
        let line = before.iter().filter(|&&b| b == b'\n').count() + 1;
        let col = before.iter().rev().take_while(|&&b| b != b'\n').count() + 1;
        format!("line {}, column {}: {}", line, col, msg)
    }

    fn skip_ws(&mut self) {
        while self.pos < self.s.len() {
            match self.s[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                b'#' => {
                    while self.pos < self.s.len() && self.s[self.pos] != b'\n' {
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn value(&mut self) -> Result<Node, String> {
        self.skip_ws();
        let start = self.pos;
        let kind = match self.peek() {
            Some(b'{') => self.object()?,
            Some(b'[') => self.array()?,
            Some(b'"') => Kind::Str(self.string()?),
            Some(b't') => self.literal("true", Kind::Bool(true))?,
            Some(b'f') => self.literal("false", Kind::Bool(false))?,
            Some(b'n') => self.literal("null", Kind::Null)?,
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number()?,
            Some(_) => return Err(self.err("unexpected character")),
            None => return Err(self.err("unexpected end of input")),
        };
        Ok(Node {
            start,
            end: self.pos,
            kind,
        })
    }

    fn literal(&mut self, word: &str, kind: Kind) -> Result<Kind, String> {
        if self.s[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(kind)
        } else {
            Err(self.err("invalid literal"))
        }
    }

    fn number(&mut self) -> Result<Kind, String> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.' | b'e' | b'E') {
                self.pos += 1;
            } else {
                break;
            }
        }
        let txt = std::str::from_utf8(&self.s[start..self.pos]).unwrap_or("");
        txt.parse::<f64>()
            .map(Kind::Num)
            .map_err(|_| self.err("invalid number"))
    }

    fn string(&mut self) -> Result<String, String> {
        self.pos += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(self.err("unterminated string"));
            };
            self.pos += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let Some(e) = self.peek() else {
                        return Err(self.err("unterminated escape"));
                    };
                    self.pos += 1;
                    match e {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let hex = self
                                .s
                                .get(self.pos..self.pos + 4)
                                .and_then(|h| std::str::from_utf8(h).ok())
                                .and_then(|h| u32::from_str_radix(h, 16).ok())
                                .ok_or_else(|| self.err("invalid unicode escape"))?;
                            self.pos += 4;
                            let ch = char::from_u32(hex).unwrap_or('\u{fffd}');
                            let mut tmp = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                        }
                        _ => return Err(self.err("invalid escape")),
                    }
                }
                _ => out.push(c),
            }
        }
        String::from_utf8(out).map_err(|_| self.err("invalid UTF-8 in string"))
    }

    fn object(&mut self) -> Result<Kind, String> {
        self.pos += 1;
        let mut members = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Kind::Object(members));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected object key"));
            }
            let key_start = self.pos;
            let key = self.string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(self.err("expected ':'"));
            }
            self.pos += 1;
            let value = self.value()?;
            members.push(Member {
                key,
                key_start,
                value,
            });
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Kind::Object(members));
                }
                _ => return Err(self.err("expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self) -> Result<Kind, String> {
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Kind::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Kind::Array(items));
                }
                _ => return Err(self.err("expected ',' or ']'")),
            }
        }
    }
}

/// Encodes a string as a JSON string literal.
pub fn quote(s: &str) -> String {
    format!("\"{}\"", crate::pkg::json_escape(s))
}

impl Node {
    pub fn member(&self, key: &str) -> Option<&Member> {
        match &self.kind {
            Kind::Object(m) => m.iter().find(|m| m.key == key),
            _ => None,
        }
    }
}

impl JsonDoc {
    pub fn parse(text: &str) -> Result<JsonDoc, String> {
        let mut p = Parser {
            s: text.as_bytes(),
            pos: 0,
        };
        let root = p.value()?;
        p.skip_ws();
        if p.pos != text.len() {
            return Err(p.err("trailing characters after JSON value"));
        }
        Ok(JsonDoc {
            text: text.to_string(),
            root,
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn root(&self) -> &Node {
        &self.root
    }

    pub fn get(&self, path: &[&str]) -> Option<&Node> {
        let mut node = &self.root;
        for key in path {
            node = &node.member(key)?.value;
        }
        Some(node)
    }

    pub fn get_bool(&self, path: &[&str]) -> Option<bool> {
        match self.get(path)?.kind {
            Kind::Bool(b) => Some(b),
            _ => None,
        }
    }

    pub fn get_str(&self, path: &[&str]) -> Option<&str> {
        match &self.get(path)?.kind {
            Kind::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn get_num(&self, path: &[&str]) -> Option<f64> {
        match self.get(path)?.kind {
            Kind::Num(n) => Some(n),
            _ => None,
        }
    }

    /// Raw source text of a value.
    pub fn raw(&self, node: &Node) -> &str {
        &self.text[node.start..node.end]
    }

    fn indent_unit(&self) -> String {
        if self.text.contains("\n\t") {
            "\t".to_string()
        } else if self.text.contains("\n    ") {
            "    ".to_string()
        } else {
            "  ".to_string()
        }
    }

    /// Leading whitespace of the line containing byte offset `pos`.
    fn line_indent(&self, pos: usize) -> String {
        let line_start = self.text[..pos].rfind('\n').map_or(0, |i| i + 1);
        self.text[line_start..]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect()
    }

    /// Sets the value at `path` to the JSON literal `literal`, creating
    /// missing intermediate objects. Untouched text is preserved verbatim.
    pub fn set(&mut self, path: &[&str], literal: &str) -> Result<(), String> {
        if path.is_empty() {
            return Err("empty path".to_string());
        }
        // Walk down as far as the path exists.
        let mut node = &self.root;
        let mut depth = 0;
        while depth < path.len() {
            match node.member(path[depth]) {
                Some(m) => {
                    node = &m.value;
                    depth += 1;
                }
                None => break,
            }
        }
        let edit = if depth == path.len() {
            (node.start, node.end, literal.to_string())
        } else {
            let Kind::Object(members) = &node.kind else {
                return Err(format!("'{}' is not an object", path[..depth].join(".")));
            };
            let unit = self.indent_unit();
            let multiline = self.text[node.start..node.end].contains('\n') || members.is_empty();
            let (indent, base) = match members.last() {
                Some(last) if multiline => (self.line_indent(last.key_start), String::new()),
                Some(_) => (String::new(), String::new()),
                None => {
                    let base = self.line_indent(node.start);
                    (format!("{}{}", base, unit), base)
                }
            };
            // Build `"k1": { "k2": literal }` for the missing tail of the path.
            let mut value = literal.to_string();
            for (i, key) in path[depth + 1..].iter().enumerate().rev() {
                let inner = format!("{}{}", indent, unit.repeat(i + 1));
                let outer = format!("{}{}", indent, unit.repeat(i));
                value = format!("{{\n{}{}: {}\n{}}}", inner, quote(key), value, outer);
            }
            let member = format!("{}: {}", quote(path[depth]), value);
            match members.last() {
                Some(last) if multiline => (
                    last.value.end,
                    last.value.end,
                    format!(",\n{}{}", indent, member),
                ),
                Some(last) => (last.value.end, last.value.end, format!(", {}", member)),
                None => (
                    node.start + 1,
                    node.end - 1,
                    format!("\n{}{}\n{}", indent, member, base),
                ),
            }
        };
        let mut text = String::with_capacity(self.text.len() + edit.2.len());
        text.push_str(&self.text[..edit.0]);
        text.push_str(&edit.2);
        text.push_str(&self.text[edit.1..]);
        *self = JsonDoc::parse(&text)?;
        Ok(())
    }
}

/// Returns the `__NAME__` placeholders still present in `text`.
pub fn placeholders(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i + 4 < b.len() {
        if b[i] == b'_' && b[i + 1] == b'_' {
            let mut j = i + 2;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            // Trim the run so that it ends with exactly "__".
            let run = &text[i..j];
            if run.len() > 4 && run.ends_with("__") {
                let name = run.trim_matches('_');
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                {
                    let p = format!("__{}__", name);
                    if !out.contains(&p) {
                        out.push(p);
                    }
                }
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

/// Replaces `__NAME__` placeholders with the given values.
pub fn fill_placeholders(text: &str, values: &[(&str, String)]) -> String {
    let mut out = text.to_string();
    for (name, val) in values {
        out = out.replace(&format!("__{}__", name), val);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: &str = "{\n\t\"name\": \"x\",\n\t# comment line\n\t\"cover\": true,\n\t\"experimental\": {\n\t\t\"remote_cover\": false\n\t},\n\t\"vm\": {\n\t\t\"count\": 1\n\t}\n}\n";

    #[test]
    fn reads_values() {
        let doc = JsonDoc::parse(CFG).unwrap();
        assert_eq!(doc.get_bool(&["cover"]), Some(true));
        assert_eq!(doc.get_str(&["name"]), Some("x"));
        assert_eq!(doc.get_num(&["vm", "count"]), Some(1.0));
        assert_eq!(doc.get_bool(&["experimental", "remote_cover"]), Some(false));
    }

    #[test]
    fn replaces_in_place_preserving_text() {
        let mut doc = JsonDoc::parse(CFG).unwrap();
        doc.set(&["cover"], "false").unwrap();
        assert_eq!(
            doc.text(),
            CFG.replace("\"cover\": true", "\"cover\": false")
        );
        doc.set(&["vm", "count"], "4").unwrap();
        assert!(doc.text().contains("\t\t\"count\": 4\n"));
        assert!(doc.text().contains("# comment line"));
    }

    #[test]
    fn inserts_nested_members_with_indentation() {
        let mut doc = JsonDoc::parse(CFG).unwrap();
        doc.set(&["experimental", "adaptive_restart", "enabled"], "true")
            .unwrap();
        assert_eq!(
            doc.get_bool(&["experimental", "adaptive_restart", "enabled"]),
            Some(true)
        );
        assert!(doc.text().contains(
            "\t\t\"remote_cover\": false,\n\t\t\"adaptive_restart\": {\n\t\t\t\"enabled\": true\n\t\t}\n\t},"
        ));
        doc.set(&["experimental", "crash_snapshot"], "true")
            .unwrap();
        assert_eq!(
            doc.get_bool(&["experimental", "crash_snapshot"]),
            Some(true)
        );
    }

    #[test]
    fn inserts_into_empty_and_inline_objects() {
        let mut doc = JsonDoc::parse("{\n\t\"a\": {}\n}").unwrap();
        doc.set(&["a", "b"], "1").unwrap();
        assert_eq!(doc.text(), "{\n\t\"a\": {\n\t\t\"b\": 1\n\t}\n}");
        let mut doc = JsonDoc::parse("{\"x\": {\"y\": 1}}").unwrap();
        doc.set(&["x", "z"], "2").unwrap();
        assert_eq!(doc.text(), "{\"x\": {\"y\": 1, \"z\": 2}}");
    }

    #[test]
    fn reports_errors_with_position() {
        let err = JsonDoc::parse("{\n  \"a\": tru\n}").unwrap_err();
        assert!(err.starts_with("line 2"), "{}", err);
    }

    #[test]
    fn finds_and_fills_placeholders() {
        let t = "{\"w\": \"__WORKDIR__\", \"n\": __VMCOUNT__, \"k\": \"__KERNEL__/x\", \"s\": \"a__b\"}";
        assert_eq!(
            placeholders(t),
            vec!["__WORKDIR__", "__VMCOUNT__", "__KERNEL__"]
        );
        let filled = fill_placeholders(t, &[("VMCOUNT", "2".to_string())]);
        assert!(filled.contains("\"n\": 2"));
    }
}

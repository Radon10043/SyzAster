//! Lightweight zero-dependency JSON parser and pretty-printer.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(BTreeMap<String, JsonValue>),
}

impl JsonValue {
    pub fn parse(input: &str) -> Result<JsonValue, String> {
        let mut parser = Parser {
            chars: input.chars().peekable(),
        };
        let val = parser.parse_value()?;
        parser.skip_ws();
        if parser.chars.peek().is_some() {
            return Err("trailing characters after JSON value".to_string());
        }
        Ok(val)
    }

    pub fn as_object(&self) -> Option<&BTreeMap<String, JsonValue>> {
        match self {
            JsonValue::Object(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut BTreeMap<String, JsonValue>> {
        match self {
            JsonValue::Object(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<JsonValue>> {
        match self {
            JsonValue::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            JsonValue::Number(n) if *n >= 0.0 => Some(*n as u64),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        self.as_object()?.get(key)
    }

    pub fn to_string_pretty(&self) -> String {
        let mut out = String::new();
        self.fmt_indent(&mut out, 0);
        out
    }

    fn fmt_indent(&self, out: &mut String, indent: usize) {
        match self {
            JsonValue::Null => out.push_str("null"),
            JsonValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            JsonValue::Number(n) => {
                if n.fract() == 0.0 && n.abs() < (1i64 << 53) as f64 {
                    out.push_str(&format!("{}", *n as i64));
                } else {
                    out.push_str(&format!("{}", n));
                }
            }
            JsonValue::String(s) => {
                out.push('"');
                out.push_str(&crate::pkg::json_escape(s));
                out.push('"');
            }
            JsonValue::Array(arr) => {
                if arr.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push_str("[\n");
                for (i, item) in arr.iter().enumerate() {
                    for _ in 0..=indent {
                        out.push_str("    ");
                    }
                    item.fmt_indent(out, indent + 1);
                    if i + 1 < arr.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                for _ in 0..indent {
                    out.push_str("    ");
                }
                out.push(']');
            }
            JsonValue::Object(map) => {
                if map.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push_str("{\n");
                for (i, (k, v)) in map.iter().enumerate() {
                    for _ in 0..=indent {
                        out.push_str("    ");
                    }
                    out.push('"');
                    out.push_str(&crate::pkg::json_escape(k));
                    out.push_str("\": ");
                    v.fmt_indent(out, indent + 1);
                    if i + 1 < map.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                for _ in 0..indent {
                    out.push_str("    ");
                }
                out.push('}');
            }
        }
    }
}

impl fmt::Display for JsonValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string_pretty())
    }
}

struct Parser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while let Some(&c) = self.chars.peek() {
            if c.is_whitespace() {
                self.chars.next();
            } else {
                break;
            }
        }
    }

    fn parse_value(&mut self) -> Result<JsonValue, String> {
        self.skip_ws();
        let Some(&c) = self.chars.peek() else {
            return Err("unexpected end of JSON input".to_string());
        };
        match c {
            '"' => self.parse_string().map(JsonValue::String),
            '{' => self.parse_object(),
            '[' => self.parse_array(),
            't' | 'f' => self.parse_bool(),
            'n' => self.parse_null(),
            '-' | '0'..='9' => self.parse_number(),
            other => Err(format!("unexpected character in JSON: '{}'", other)),
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        if self.chars.next() != Some('"') {
            return Err("expected '\"'".to_string());
        }
        let mut s = String::new();
        while let Some(c) = self.chars.next() {
            match c {
                '"' => return Ok(s),
                '\\' => {
                    let esc = self.chars.next().ok_or("unterminated escape sequence")?;
                    match esc {
                        '"' => s.push('"'),
                        '\\' => s.push('\\'),
                        '/' => s.push('/'),
                        'b' => s.push('\u{0008}'),
                        'f' => s.push('\u{000c}'),
                        'n' => s.push('\n'),
                        'r' => s.push('\r'),
                        't' => s.push('\t'),
                        'u' => {
                            let mut hex = String::with_capacity(4);
                            for _ in 0..4 {
                                hex.push(self.chars.next().ok_or("incomplete unicode escape")?);
                            }
                            let code = u32::from_str_radix(&hex, 16)
                                .map_err(|_| format!("invalid unicode escape: \\u{}", hex))?;
                            if let Some(ch) = char::from_u32(code) {
                                s.push(ch);
                            }
                        }
                        other => return Err(format!("invalid escape character: \\{}", other)),
                    }
                }
                other => s.push(other),
            }
        }
        Err("unterminated string literal".to_string())
    }

    fn parse_object(&mut self) -> Result<JsonValue, String> {
        self.chars.next(); // '{'
        self.skip_ws();
        let mut map = BTreeMap::new();
        if self.chars.peek() == Some(&'}') {
            self.chars.next();
            return Ok(JsonValue::Object(map));
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            if self.chars.next() != Some(':') {
                return Err("expected ':' after object key".to_string());
            }
            let val = self.parse_value()?;
            map.insert(key, val);
            self.skip_ws();
            match self.chars.next() {
                Some(',') => continue,
                Some('}') => break,
                other => return Err(format!("expected ',' or '}}' in object, got {:?}", other)),
            }
        }
        Ok(JsonValue::Object(map))
    }

    fn parse_array(&mut self) -> Result<JsonValue, String> {
        self.chars.next(); // '['
        self.skip_ws();
        let mut arr = Vec::new();
        if self.chars.peek() == Some(&']') {
            self.chars.next();
            return Ok(JsonValue::Array(arr));
        }
        loop {
            let val = self.parse_value()?;
            arr.push(val);
            self.skip_ws();
            match self.chars.next() {
                Some(',') => continue,
                Some(']') => break,
                other => return Err(format!("expected ',' or ']' in array, got {:?}", other)),
            }
        }
        Ok(JsonValue::Array(arr))
    }

    fn parse_bool(&mut self) -> Result<JsonValue, String> {
        let mut word = String::new();
        while let Some(&c) = self.chars.peek() {
            if c.is_ascii_alphabetic() {
                word.push(c);
                self.chars.next();
            } else {
                break;
            }
        }
        match word.as_str() {
            "true" => Ok(JsonValue::Bool(true)),
            "false" => Ok(JsonValue::Bool(false)),
            _ => Err(format!("invalid boolean literal: {}", word)),
        }
    }

    fn parse_null(&mut self) -> Result<JsonValue, String> {
        let mut word = String::new();
        for _ in 0..4 {
            if let Some(c) = self.chars.next() {
                word.push(c);
            }
        }
        if word == "null" {
            Ok(JsonValue::Null)
        } else {
            Err(format!("invalid null literal: {}", word))
        }
    }

    fn parse_number(&mut self) -> Result<JsonValue, String> {
        let mut s = String::new();
        while let Some(&c) = self.chars.peek() {
            if c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E') {
                s.push(c);
                self.chars.next();
            } else {
                break;
            }
        }
        let n: f64 = s
            .parse()
            .map_err(|_| format!("invalid number literal: {}", s))?;
        Ok(JsonValue::Number(n))
    }
}

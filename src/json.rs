use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum JsonVal {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<JsonVal>),
    Obj(HashMap<String, JsonVal>),
}

impl JsonVal {
    pub fn as_str(&self) -> Option<&str> {
        if let Self::Str(s) = self { Some(s.as_str()) } else { None }
    }

    pub fn str_or<'a>(&'a self, default: &'a str) -> &'a str {
        self.as_str().unwrap_or(default)
    }

    pub fn as_f64(&self) -> Option<f64> {
        if let Self::Num(n) = self { Some(*n) } else { None }
    }

    pub fn f64_or(&self, default: f64) -> f64 {
        self.as_f64().unwrap_or(default)
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn i64_or(&self, default: i64) -> i64 {
        self.as_f64().map_or(default, |v| v as i64)
    }

    pub fn as_arr(&self) -> Option<&[JsonVal]> {
        if let Self::Arr(a) = self { Some(a.as_slice()) } else { None }
    }

    pub fn get(&self, key: &str) -> &JsonVal {
        static NULL: JsonVal = JsonVal::Null;
        if let Self::Obj(map) = self {
            map.get(key).unwrap_or(&NULL)
        } else {
            &NULL
        }
    }
}

pub fn parse_json(input: &str) -> Option<JsonVal> {
    let mut parser = Parser { bytes: input.as_bytes(), pos: 0 };
    let val = parser.parse_value()?;
    Some(val)
}

pub fn parse_jsonc(input: &str) -> Option<JsonVal> {
    let mut filtered = String::with_capacity(input.len());
    for line in input.lines() {
        if !line.trim_start().starts_with("//") {
            filtered.push_str(line);
            filtered.push('\n');
        }
    }
    parse_json(&filtered)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn parse_value(&mut self) -> Option<JsonVal> {
        self.skip_ws();
        match self.peek()? {
            b'"' => self.parse_string().map(JsonVal::Str),
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b't' => {
                if self.bytes.get(self.pos..self.pos + 4) == Some(b"true") {
                    self.pos += 4;
                    Some(JsonVal::Bool(true))
                } else {
                    None
                }
            }
            b'f' => {
                if self.bytes.get(self.pos..self.pos + 5) == Some(b"false") {
                    self.pos += 5;
                    Some(JsonVal::Bool(false))
                } else {
                    None
                }
            }
            b'n' => {
                if self.bytes.get(self.pos..self.pos + 4) == Some(b"null") {
                    self.pos += 4;
                    Some(JsonVal::Null)
                } else {
                    None
                }
            }
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => None,
        }
    }

    fn parse_string(&mut self) -> Option<String> {
        if self.peek()? != b'"' {
            return None;
        }
        self.pos += 1;
        let mut out = String::new();
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            self.pos += 1;
            match b {
                b'"' => return Some(out),
                b'\\' => {
                    let esc = *self.bytes.get(self.pos)?;
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = std::str::from_utf8(self.bytes.get(self.pos..self.pos + 4)?).ok()?;
                            let code = u32::from_str_radix(hex, 16).ok()?;
                            self.pos += 4;
                            if let Some(ch) = char::from_u32(code) {
                                out.push(ch);
                            }
                        }
                        _ => out.push(esc as char),
                    }
                }
                _ => {
                    let start = self.pos - 1;
                    while self.pos < self.bytes.len()
                        && self.bytes[self.pos] != b'"'
                        && self.bytes[self.pos] != b'\\'
                    {
                        self.pos += 1;
                    }
                    let slice = std::str::from_utf8(&self.bytes[start..self.pos]).ok()?;
                    out.push_str(slice);
                }
            }
        }
        None
    }

    fn parse_number(&mut self) -> Option<JsonVal> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while let Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-') = self.peek() {
            self.pos += 1;
        }
        let s = std::str::from_utf8(&self.bytes[start..self.pos]).ok()?;
        let n = s.parse::<f64>().ok()?;
        Some(JsonVal::Num(n))
    }

    fn parse_array(&mut self) -> Option<JsonVal> {
        self.pos += 1; // '['
        let mut arr = Vec::new();
        loop {
            self.skip_ws();
            if self.peek()? == b']' {
                self.pos += 1;
                return Some(JsonVal::Arr(arr));
            }
            arr.push(self.parse_value()?);
            self.skip_ws();
            match self.peek()? {
                b',' => self.pos += 1,
                b']' => {
                    self.pos += 1;
                    return Some(JsonVal::Arr(arr));
                }
                _ => return None,
            }
        }
    }

    fn parse_object(&mut self) -> Option<JsonVal> {
        self.pos += 1; // '{'
        let mut map = HashMap::new();
        loop {
            self.skip_ws();
            if self.peek()? == b'}' {
                self.pos += 1;
                return Some(JsonVal::Obj(map));
            }
            let key = self.parse_string()?;
            self.skip_ws();
            if self.peek()? != b':' {
                return None;
            }
            self.pos += 1;
            let val = self.parse_value()?;
            map.insert(key, val);
            self.skip_ws();
            match self.peek()? {
                b',' => self.pos += 1,
                b'}' => {
                    self.pos += 1;
                    return Some(JsonVal::Obj(map));
                }
                _ => return None,
            }
        }
    }
}

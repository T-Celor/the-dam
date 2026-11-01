// A JSON array of records rewritten as a table: the field names once, then one row each.
// Every table is read back and compared with the source before it is used, so it cannot lose a value.

const MOST_DEPTH: usize = 128;

struct Reader<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(text: &'a str) -> Self {
        Reader { text, at: 0 }
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> Option<()> {
        (self.peek() == Some(byte)).then(|| self.at += 1)
    }

    fn string(&mut self) -> Option<&'a str> {
        let start = self.at;
        self.eat(b'"')?;
        loop {
            match self.peek()? {
                b'"' => {
                    self.at += 1;
                    return Some(&self.text[start..self.at]);
                }
                b'\\' => self.at += 2,
                b'\n' | b'\r' => return None,
                _ => self.at += 1,
            }
        }
    }

    // Moves past one value and returns its source text.
    fn value(&mut self, depth: usize) -> Option<&'a str> {
        if depth > MOST_DEPTH {
            return None;
        }
        self.skip_space();
        let start = self.at;
        match self.peek()? {
            b'"' => {
                self.string()?;
            }
            b'{' => {
                self.entries(depth)?;
            }
            b'[' => {
                self.items(depth, |_| Some(()))?;
            }
            _ => {
                while self.peek().is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'+' | b'.')) {
                    self.at += 1;
                }
                if !is_literal(&self.text[start..self.at]) {
                    return None;
                }
            }
        }

        Some(&self.text[start..self.at])
    }

    fn entries(&mut self, depth: usize) -> Option<Vec<(&'a str, &'a str)>> {
        self.eat(b'{')?;
        let mut entries = Vec::new();
        self.skip_space();
        if self.eat(b'}').is_some() {
            return Some(entries);
        }
        loop {
            self.skip_space();
            let key = self.string()?;
            self.skip_space();
            self.eat(b':')?;
            entries.push((key, self.value(depth + 1)?));
            self.skip_space();
            if self.eat(b'}').is_some() {
                return Some(entries);
            }
            self.eat(b',')?;
        }
    }

    fn items(&mut self, depth: usize, mut each: impl FnMut(&mut Self) -> Option<()>) -> Option<()> {
        self.eat(b'[')?;
        self.skip_space();
        if self.eat(b']').is_some() {
            return Some(());
        }
        loop {
            self.skip_space();
            let before = self.at;
            each(self)?;
            if self.at == before {
                self.value(depth + 1)?;
            }
            self.skip_space();
            if self.eat(b']').is_some() {
                return Some(());
            }
            self.eat(b',')?;
        }
    }

    fn is_done(&mut self) -> bool {
        self.skip_space();
        self.at == self.text.len()
    }
}

fn is_literal(text: &str) -> bool {
    let is_number = text.as_bytes().first().is_some_and(|b| b.is_ascii_digit() || *b == b'-') && text.parse::<f64>().is_ok();

    is_number || matches!(text, "true" | "false" | "null")
}

pub fn is_valid(text: &str) -> bool {
    let mut reader = Reader::new(text);

    reader.value(0).is_some() && reader.is_done()
}

// Nested values on one line, as a row holds exactly one line.
fn compact(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_string = false;
    let mut escaped = false;
    for c in raw.chars() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if !c.is_whitespace() {
            in_string = c == '"';
            out.push(c);
        }
    }

    out
}

// Text that can stand without its quotes and still read back as the same string.
fn bare(inner: &str) -> bool {
    !inner.is_empty()
        && inner.trim() == inner
        && !inner.contains(['\\', ',', '"', '\n', '\r'])
        && !inner.starts_with(['{', '[', '"'])
        && !is_literal(inner)
}

fn cell(raw: &str) -> String {
    let flat = compact(raw);
    match flat.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) {
        Some(inner) if bare(inner) => inner.to_string(),
        _ => flat,
    }
}

type Row = Vec<(String, String)>;

fn rows_of(text: &str) -> Option<Vec<Row>> {
    let mut reader = Reader::new(text);
    reader.skip_space();
    let mut rows: Vec<Row> = Vec::new();
    reader.items(0, |item| {
        let entries = item.entries(1)?;
        if entries.is_empty() {
            return None;
        }
        let mut row: Row = entries.iter().map(|(key, value)| (key.to_string(), compact(value))).collect();
        row.sort();
        // A repeated key cannot be told apart in a table.
        if row.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return None;
        }
        rows.push(row);
        Some(())
    })?;

    (reader.is_done() && rows.len() >= 2).then_some(rows)
}

fn read_back(table: &str) -> Option<Vec<Row>> {
    let (head, body) = table.split_once('\n')?;
    let fields = head.strip_prefix('[')?.split_once("]{")?.1.strip_suffix('}')?;
    let keys: Vec<&str> = fields.split(',').collect();
    let mut rows = Vec::new();
    for line in body.split('\n') {
        let mut reader = Reader::new(line);
        let mut row = Row::new();
        for (n, key) in keys.iter().enumerate() {
            let start = reader.at;
            if matches!(reader.peek(), Some(b'"' | b'{' | b'[')) {
                reader.value(0)?;
            } else {
                while reader.peek().is_some_and(|b| b != b',') {
                    reader.at += 1;
                }
            }
            let value = &line[start..reader.at];
            if !value.is_empty() {
                let is_json = value.starts_with(['"', '{', '[']) || is_literal(value);
                row.push((format!("\"{key}\""), if is_json { value.to_string() } else { format!("\"{value}\"") }));
            }
            if n + 1 < keys.len() {
                reader.eat(b',')?;
            }
        }
        if !reader.is_done() {
            return None;
        }
        row.sort();
        rows.push(row);
    }

    Some(rows)
}

// None when the text is not such an array, or the table would not read back identically.
pub fn table(text: &str) -> Option<String> {
    let rows = rows_of(text)?;
    let mut keys: Vec<&str> = Vec::new();
    let mut order: Vec<&str> = Vec::new();
    let mut reader = Reader::new(text);
    reader.skip_space();
    let mut sources: Vec<Vec<(&str, &str)>> = Vec::new();
    reader.items(0, |item| {
        sources.push(item.entries(1)?);
        Some(())
    })?;
    for (key, _) in sources.iter().flatten() {
        if !order.contains(key) {
            order.push(key);
            let inner = key.strip_prefix('"')?.strip_suffix('"')?;
            // A name that the header could not hold leaves the array as it is.
            if !bare(inner) || inner.contains(['{', '}', '[', ']']) {
                return None;
            }
            keys.push(inner);
        }
    }
    let mut out = format!("[{}]{{{}}}", rows.len(), keys.join(","));
    for source in &sources {
        out.push('\n');
        let cells: Vec<String> = order.iter().map(|key| source.iter().find(|(k, _)| k == key).map_or(String::new(), |(_, value)| cell(value))).collect();
        out.push_str(&cells.join(","));
    }

    (read_back(&out)? == rows && out.len() < text.len()).then_some(out)
}

// The top-level fields of an object, keys with their quotes, values as written.
pub fn entries(text: &str) -> Option<Vec<(String, String)>> {
    let mut reader = Reader::new(text);
    reader.skip_space();
    let found = reader.entries(0)?;

    Some(found.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect())
}

// A top-level string field, unescaped.
pub fn field(text: &str, name: &str) -> Option<String> {
    let wanted = format!("\"{name}\"");
    let (_, raw) = entries(text)?.into_iter().find(|(key, _)| *key == wanted)?;

    unquote(&raw)
}

fn unquote(raw: &str) -> Option<String> {
    let inner = raw.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'u' => {
                let hex: String = chars.by_ref().take(4).collect();
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?).unwrap_or('\u{fffd}'));
            }
            other => out.push(other),
        }
    }

    Some(out)
}

pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_read_and_strings_quote_back() {
        let text = r#"{"source": "compact", "n": 1, "prompt": "a \"b\"\nc\u00e9"}"#;
        assert_eq!(field(text, "source").as_deref(), Some("compact"));
        assert_eq!(field(text, "prompt").as_deref(), Some("a \"b\"\ncé"));
        assert_eq!(field(text, "n"), None);
        assert_eq!(field("not json", "x"), None);
        assert_eq!(unquote(&quote("a\"\\\n\t\u{1}é")).as_deref(), Some("a\"\\\n\t\u{1}é"));
    }

    #[test]
    fn records_become_a_table() {
        let out = table(r#"[{"id": 1, "name": "ann"}, {"id": 2, "name": "bob"}]"#).unwrap();
        assert_eq!(out, "[2]{id,name}\n1,ann\n2,bob");
    }

    #[test]
    fn absent_null_empty_and_types_stay_apart() {
        let text = r#"[{"a": 1, "b": null, "c": ""}, {"a": "1", "c": "null"}, {"a": 1.50, "b": true, "c": "true"}]"#;
        let out = table(text).unwrap();
        assert_eq!(out, "[3]{a,b,c}\n1,null,\"\"\n\"1\",,\"null\"\n1.50,true,\"true\"");
    }

    #[test]
    fn hard_strings_and_nested_values_read_back_whole() {
        let text = r#"[{"s": "a, b", "n": {"x": [1, 2], "y": "q\"r"}}, {"s": " spaced ", "n": [ {"k": "é"} ]}, {"s": "line\nbreak", "n": 0}]"#;
        let out = table(text).unwrap();
        assert_eq!(read_back(&out).unwrap(), rows_of(text).unwrap());
        assert!(out.contains(r#""a, b""#) && out.contains(r#"" spaced ""#) && out.contains(r#"line\nbreak"#) && out.contains('é'));
    }

    #[test]
    fn what_a_table_cannot_hold_is_refused() {
        for text in [r#"{"a": 1}"#, "[1, 2, 3]", r#"[{"a": 1}]"#, r#"[{"a": 1, "a": 2}, {"a": 3}]"#, r#"[{"a,b": 1}, {"a,b": 2}]"#, r#"[{"a": 1}, 2]"#, "[{}, {}]", r#"[{"a": 1}, {"a": 2}] tail"#] {
            assert_eq!(table(text), None, "{text}");
        }
    }

    #[test]
    fn validity_is_strict_and_deep_nesting_does_not_overflow() {
        assert!(is_valid(r#"{"a": [1, -2.5e3, true, null, "x"]}"#));
        for text in ["[INFO] started", "{oops}", "[1,]", "\"open", "{\"a\":1} {\"a\":2}", "nope"] {
            assert!(!is_valid(text), "{text}");
        }
        assert!(!is_valid(&"[".repeat(100_000)));
    }
}

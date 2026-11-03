// Reads one source file into definitions, calls and imports. A scanner, not a parser:
// no grammar and no C compiler to ship, at the cost of some misses on unusual syntax.

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Lang {
    Python,
    Script,
    Rust,
    Java,
    CSharp,
    Go,
    C,
    Kotlin,
}

pub fn lang_of(path: &str) -> Option<Lang> {
    Some(match path.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "py" => Lang::Python,
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Lang::Script,
        "rs" => Lang::Rust,
        "java" => Lang::Java,
        "cs" => Lang::CSharp,
        "go" => Lang::Go,
        "c" | "h" | "cc" | "cpp" | "hpp" | "cxx" => Lang::C,
        "kt" | "kts" => Lang::Kotlin,
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    File,
    Function,
    Type,
    // A document's heading or a settings file's table, and a settings file's top-level key.
    Section,
    Key,
}

#[derive(Debug)]
pub struct Def {
    pub name: String,
    pub kind: Kind,
    pub line: usize,
}

// `owner` is an index into `defs`, None for the file itself.
#[derive(Debug)]
pub struct Call {
    pub owner: Option<usize>,
    pub name: String,
    pub is_member: bool,
    // What the call hangs off: `store` in `self.store.get(`, `Foo` in `Foo::new(`.
    pub receiver: Option<String>,
}

#[derive(Debug, Default)]
pub struct Scan {
    pub defs: Vec<Def>,
    pub calls: Vec<Call>,
    pub bases: Vec<(usize, String)>,
    pub imports: Vec<String>,
    // A variable's name and type, wherever the file makes it plain.
    pub types: Vec<(String, String)>,
}

const NOT_CALLS: [&str; 35] = [
    "if", "for", "while", "switch", "catch", "return", "function", "fn", "func", "def", "class", "match", "elif", "else", "except", "with", "assert", "await", "yield", "sizeof", "typeof", "throw", "using", "lock", "foreach", "not", "and", "or", "in", "is", "lambda", "super", "this", "self", "print",
];

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 128
}

// Comments and string contents become spaces, line for line, so what is left is only code.
pub fn clean(text: &[u8], lang: Lang) -> Vec<u8> {
    let mut out = text.to_vec();
    let mut i = 0;
    let blank = |out: &mut Vec<u8>, from: usize, to: usize| {
        for b in &mut out[from..to] {
            if *b != b'\n' {
                *b = b' ';
            }
        }
    };
    let at = |i: usize, s: &[u8]| text[i..].starts_with(s);
    while i < text.len() {
        let b = text[i];
        if lang == Lang::Python && b == b'#' || lang != Lang::Python && at(i, b"//") {
            let end = text[i..].iter().position(|&c| c == b'\n').map_or(text.len(), |n| i + n);
            blank(&mut out, i, end);
            i = end;
        } else if lang != Lang::Python && at(i, b"/*") {
            let end = text[i + 2..].windows(2).position(|w| w == b"*/").map_or(text.len(), |n| i + n + 4);
            blank(&mut out, i, end);
            i = end;
        } else if lang == Lang::Python && (at(i, b"\"\"\"") || at(i, b"'''")) {
            let quote = &text[i..i + 3];
            let end = text[i + 3..].windows(3).position(|w| w == quote).map_or(text.len(), |n| i + n + 6);
            blank(&mut out, i + 1, end.saturating_sub(1).max(i + 1));
            i = end;
        } else if b == b'"' || b == b'`' && matches!(lang, Lang::Script | Lang::Go) || b == b'\'' && is_string_quote(text, i, lang) {
            let multiline = b == b'`';
            let mut j = i + 1;
            while j < text.len() && text[j] != b && (multiline || text[j] != b'\n') {
                j += if text[j] == b'\\' { 2 } else { 1 };
            }
            let end = j.min(text.len());
            blank(&mut out, i + 1, end);
            i = end + 1;
        } else {
            i += 1;
        }
    }

    out
}

// In Rust and C-likes a quote can be a lifetime or a char, so it only opens a string where it closes soon.
fn is_string_quote(text: &[u8], i: usize, lang: Lang) -> bool {
    match lang {
        Lang::Python | Lang::Script => true,
        _ => text.get(i + 1) == Some(&b'\\') || text.get(i + 2) == Some(&b'\''),
    }
}

fn ident_at(line: &[u8], from: usize) -> Option<(String, usize)> {
    let mut i = from;
    while i < line.len() && (line[i] == b' ' || line[i] == b'\t' || line[i] == b'*' || line[i] == b'&') {
        i += 1;
    }
    let start = i;
    while i < line.len() && is_ident(line[i]) {
        i += 1;
    }
    (i > start).then(|| (String::from_utf8_lossy(&line[start..i]).into_owned(), i))
}

fn find_word(line: &[u8], word: &str) -> Option<usize> {
    let w = word.as_bytes();
    (0..line.len().saturating_sub(w.len() - 1)).find(|&i| {
        line[i..].starts_with(w) && (i == 0 || (!is_ident(line[i - 1]) && line[i - 1] != b'.')) && line.get(i + w.len()).is_none_or(|&c| !is_ident(c))
    })
}

// Every `name(` on a line, with whether it hangs off an object other than self or this.
fn calls_in(line: &[u8], skip: Option<&str>, owner: Option<usize>, out: &mut Vec<Call>) {
    let mut i = 0;
    while i < line.len() {
        if !is_ident(line[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < line.len() && is_ident(line[i]) {
            i += 1;
        }
        let mut next = i;
        if line.get(next) == Some(&b'!') {
            next += 1;
        }
        if line.get(next) != Some(&b'(') {
            continue;
        }
        let name = String::from_utf8_lossy(&line[start..i]);
        if line[start].is_ascii_digit() || NOT_CALLS.contains(&name.as_ref()) || skip == Some(name.as_ref()) {
            continue;
        }
        let is_member = start > 0 && line[start - 1] == b'.' && {
            let head = &line[..start - 1];
            !(head.ends_with(b"self") || head.ends_with(b"this"))
        };
        let separator = if start > 0 && line[start - 1] == b'.' {
            1
        } else if start > 1 && &line[start - 2..start] == b"::" {
            2
        } else {
            0
        };
        let receiver = (separator > 0).then(|| {
            let end = start - separator;
            let from = line[..end].iter().rposition(|&b| !is_ident(b)).map_or(0, |at| at + 1);
            String::from_utf8_lossy(&line[from..end]).into_owned()
        }).filter(|r| !r.is_empty() && r != "self" && r != "this");
        out.push(Call { owner, name: name.into_owned(), is_member, receiver });
    }
}

fn is_type_name(word: &str) -> bool {
    word.chars().next().is_some_and(char::is_uppercase)
}

// `Foo x`, `x: Foo`, `x = Foo(`, `x = new Foo(`, `let x = Foo::new(`: what a later `x.go(` calls into.
fn types_in(line: &[u8], out: &mut Vec<(String, String)>) {
    let mut words: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < line.len() {
        if is_ident(line[i]) {
            let start = i;
            while i < line.len() && is_ident(line[i]) {
                i += 1;
            }
            words.push((start, i));
        } else {
            i += 1;
        }
    }
    let text = |at: usize| String::from_utf8_lossy(&line[words[at].0..words[at].1]).into_owned();
    let gap = |at: usize| String::from_utf8_lossy(&line[words[at].1..words[at + 1].0]).into_owned();
    for at in 0..words.len().saturating_sub(1) {
        let (first, between, second) = (text(at), gap(at), text(at + 1));
        let between = between.trim();
        if between.is_empty() && gap(at) != "" && is_type_name(&first) && !is_type_name(&second) {
            out.push((second, first));
        } else if between == ":" && is_type_name(&second) {
            out.push((first, second));
        } else if between == "=" {
            let is_word = |word: &str| second == word && at + 2 < words.len() && gap(at + 1).trim().is_empty();
            let target = if is_word("new") || is_word("await") { text(at + 2) } else { second };
            if is_type_name(&target) {
                out.push((first, target));
            }
        }
    }
}

fn imports_of(raw: &str, code: &str, lang: Lang, out: &mut Vec<String>) {
    let code = code.trim_start();
    let quoted = |s: &str| s.split(['"', '\'', '`']).nth(1).map(str::to_string);
    let found = match lang {
        Lang::Python => {
            if let Some(rest) = code.strip_prefix("from ") {
                let module = rest.split_whitespace().next().unwrap_or("");
                let names = rest.split(" import ").nth(1).unwrap_or("");
                for name in names.split(',') {
                    let name = name.split_whitespace().next().unwrap_or("").trim_matches(['(', ')']);
                    if !name.is_empty() && name != "*" {
                        out.push(format!("{module}.{name}"));
                    }
                }
                Some(module.to_string())
            } else if let Some(rest) = code.strip_prefix("import ") {
                for module in rest.split(',').skip(1) {
                    out.push(module.split_whitespace().next().unwrap_or("").to_string());
                }
                rest.split(',').next().map(|m| m.split_whitespace().next().unwrap_or("").to_string())
            } else {
                None
            }
        }
        Lang::Script => (code.starts_with("import ") || code.starts_with("export ") && code.contains(" from ") || code.contains("require(") || code.contains("} from ")).then(|| quoted(raw)).flatten(),
        Lang::Rust => code.strip_prefix("use ").or(code.strip_prefix("pub use ")).or(code.strip_prefix("mod ")).or(code.strip_prefix("pub mod ")).map(|rest| rest.trim_end_matches([';', ' ', '{']).split('{').next().unwrap_or("").trim_end_matches(':').to_string()),
        Lang::Java | Lang::Kotlin => code.strip_prefix("import ").map(|rest| rest.trim_start_matches("static ").trim_end_matches([';', ' ']).trim_end_matches(".*").to_string()),
        Lang::CSharp => code.strip_prefix("using ").filter(|rest| !rest.contains('(') && !rest.contains('=')).map(|rest| rest.trim_end_matches([';', ' ']).to_string()),
        Lang::Go => (code.starts_with("import ") || code.trim_end().ends_with('"') && !code.contains('(') && code.len() < raw.len() + 1 && code.starts_with('"')).then(|| quoted(raw)).flatten(),
        Lang::C => code.strip_prefix("#include").and_then(|_| raw.split('"').nth(1)).map(str::to_string),
    };
    if let Some(found) = found.filter(|f| !f.is_empty()) {
        out.push(found);
    }
}

fn scan_python(lines: &[&[u8]], scan: &mut Scan) {
    let mut scopes: Vec<(usize, usize)> = Vec::new();
    let mut open_brackets = 0i32;
    for (n, line) in lines.iter().enumerate() {
        let body = line.iter().position(|&b| b != b' ' && b != b'\t');
        let Some(indent) = body else { continue };
        let was_open = open_brackets > 0;
        for &b in *line {
            match b {
                b'(' | b'[' | b'{' => open_brackets += 1,
                b')' | b']' | b'}' => open_brackets -= 1,
                _ => {}
            }
        }
        open_brackets = open_brackets.max(0);
        // Inside a bracket the indentation means nothing: `) -> int:` does not end its function.
        if was_open {
            calls_in(&line[indent..], None, scopes.last().map(|&(_, def)| def), &mut scan.calls);
            continue;
        }
        while scopes.last().is_some_and(|&(at, _)| indent <= at) {
            scopes.pop();
        }
        let owner = scopes.last().map(|&(_, def)| def);
        let code = &line[indent..];
        let word = if code.starts_with(b"def ") {
            Some((4, Kind::Function))
        } else if code.starts_with(b"async def ") {
            Some((10, Kind::Function))
        } else if code.starts_with(b"class ") {
            Some((6, Kind::Type))
        } else {
            None
        };
        if let Some((name, end)) = word.and_then(|(from, _)| ident_at(code, from)) {
            let kind = word.map_or(Kind::Function, |(_, kind)| kind);
            scan.defs.push(Def { name, kind, line: n + 1 });
            let def = scan.defs.len() - 1;
            if kind == Kind::Type && code.get(end) == Some(&b'(') {
                let inside = &code[end + 1..];
                let close = inside.iter().position(|&b| b == b')').unwrap_or(inside.len());
                for base in String::from_utf8_lossy(&inside[..close]).split(',') {
                    let base = base.trim().rsplit('.').next().unwrap_or("").to_string();
                    if !base.is_empty() && !base.contains('=') {
                        scan.bases.push((def, base));
                    }
                }
            }
            scopes.push((indent, def));
            continue;
        }
        calls_in(code, None, owner, &mut scan.calls);
    }
}

// What a line opens: its name, its kind, and whether it is only a declaration if `;` comes first.
fn def_on(code: &[u8], lang: Lang, in_type: bool, in_function: bool, at_top: bool) -> Option<(String, Kind, bool)> {
    if lang == Lang::Rust {
        let text = String::from_utf8_lossy(code);
        let bare = text.strip_prefix("pub(crate) ").or(text.strip_prefix("pub(super) ")).or(text.strip_prefix("pub ")).unwrap_or(&text);
        if let Some(rest) = bare.strip_prefix("const ").or(bare.strip_prefix("static ")) {
            let rest = rest.strip_prefix("mut ").unwrap_or(rest);
            if !rest.starts_with("fn ") && !rest.starts_with("unsafe ") {
                let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                if !name.is_empty() {
                    return Some((name, Kind::Type, false));
                }
            }
        }
    }
    if lang == Lang::Script {
        let text = String::from_utf8_lossy(code);
        let bare = text.strip_prefix("export default ").or(text.strip_prefix("export ")).unwrap_or(&text);
        let value = bare.strip_prefix("const ").or(bare.strip_prefix("let ")).or(bare.strip_prefix("var "));
        if let (true, Some(rest)) = (at_top, value) {
            let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').collect();
            if !name.is_empty() {
                let right = rest.split_once('=').map_or("", |(_, right)| right.trim_start());
                let is_function = right.contains("=>") || right.starts_with("function") || right.starts_with("async");
                return Some((name, if is_function { Kind::Function } else { Kind::Type }, false));
            }
        }
    }
    let types: &[&str] = match lang {
        Lang::Rust => &["struct", "enum", "trait", "union"],
        Lang::Script => &["class", "interface", "enum"],
        Lang::Java | Lang::CSharp | Lang::Kotlin => &["class", "interface", "enum", "struct", "record"],
        Lang::C => &["class", "struct", "union", "enum"],
        Lang::Go | Lang::Python => &[],
    };
    for word in types {
        if let Some((name, _)) = find_word(code, word).and_then(|at| ident_at(code, at + word.len())) {
            return Some((name, Kind::Type, lang == Lang::C));
        }
    }
    let functions: &[&str] = match lang {
        Lang::Rust => &["fn"],
        Lang::Script => &["function"],
        Lang::Go => &["func"],
        Lang::Kotlin => &["fun"],
        _ => &[],
    };
    for word in functions {
        if let Some(at) = find_word(code, word) {
            let mut from = at + word.len();
            // A Go method: `func (r *T) name(`.
            if lang == Lang::Go && code[from..].iter().find(|&&b| b != b' ') == Some(&b'(') {
                from += code[from..].iter().position(|&b| b == b')')? + 1;
            }
            if lang == Lang::Kotlin && code[from..].iter().find(|&&b| b != b' ') == Some(&b'<') {
                from += code[from..].iter().position(|&b| b == b'>')? + 1;
            }
            if let Some((name, end)) = ident_at(code, from) {
                // A Kotlin extension: `fun Type.name(`.
                if code.get(end) == Some(&b'.') {
                    if let Some((name, _)) = ident_at(code, end + 1) {
                        return Some((name, Kind::Function, false));
                    }
                }
                return Some((name, Kind::Function, false));
            }
        }
    }
    if lang == Lang::Script {
        // `const name = (x) => {`, `name = async function`.
        let text = String::from_utf8_lossy(code);
        if let Some((left, right)) = text.split_once('=') {
            let is_arrow = right.contains("=>") || right.trim_start().starts_with("async") || right.contains("function");
            if is_arrow && !left.contains('(') && !left.contains('.') && !right.trim_start().starts_with('=') {
                if let Some(name) = left.split(':').next().unwrap_or("").split_whitespace().next_back() {
                    if !name.is_empty() && name.bytes().all(is_ident) {
                        return Some((name.to_string(), Kind::Function, false));
                    }
                }
            }
        }
    }
    // A method by its look: `name(...)` ending the way a signature does, directly inside a type.
    let by_look = !in_function && (in_type && matches!(lang, Lang::Java | Lang::CSharp | Lang::Script | Lang::C) || lang == Lang::C);
    if by_look {
        let paren = code.iter().position(|&b| b == b'(')?;
        let head = &code[..paren];
        if head.contains(&b'=') || head.contains(&b'.') {
            return None;
        }
        let end = head.iter().rposition(|&b| b != b' ')? + 1;
        let start = head[..end].iter().rposition(|&b| !is_ident(b)).map_or(0, |at| at + 1);
        // `_fetch<T>(`: the name stands before its type parameters.
        let (end, start) = if head[..end].ends_with(b">") {
            let open = head[..end].iter().position(|&b| b == b'<').unwrap_or(end);
            let close = head[..open].iter().rposition(|&b| b != b' ').map_or(0, |at| at + 1);
            let first = head[..close].iter().rposition(|&b| !is_ident(b)).map_or(0, |at| at + 1);
            (close, first)
        } else {
            (end, start)
        };
        let name = String::from_utf8_lossy(&head[start..end]).into_owned();
        let tail = String::from_utf8_lossy(&code[paren..]);
        let tail = tail.trim_end();
        let shape = tail.ends_with('(') || tail.contains('{') || tail.ends_with(')') || tail.ends_with(',') || tail.ends_with(';') && in_type || tail.contains("throws ") || tail.ends_with("=>");
        if name.is_empty() || !shape || NOT_CALLS.contains(&name.as_str()) || name.as_bytes()[0].is_ascii_digit() {
            return None;
        }
        return Some((name, Kind::Function, lang == Lang::C && !in_type));
    }

    None
}

fn paren_balance(line: &[u8]) -> i32 {
    line.iter().map(|&b| i32::from(b == b'(') - i32::from(b == b')')).sum()
}

fn scan_braces(lines: &[&[u8]], lang: Lang, scan: &mut Scan) {
    // A scope is (the depth it opened at, its definition).
    let mut scopes: Vec<(usize, usize)> = Vec::new();
    let mut depth = 0usize;
    let mut pending: Option<(usize, bool)> = None;
    let mut extra_owner: Option<String> = None;
    let mut annotation_depth = 0i32;
    // Enums still listing their members, as opposed to Java's methods after the `;`.
    let mut listing_enums: Vec<usize> = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        let Some(indent) = line.iter().position(|&b| b != b' ' && b != b'\t') else { continue };
        let code = &line[indent..];
        // An annotation such as `@Path("x")` looks like a signature and opens nothing;
        // it can run over several lines and can stand before the method on one line.
        let mut code = code;
        if annotation_depth > 0 {
            let closes = paren_balance(code);
            annotation_depth += closes;
            if annotation_depth > 0 {
                continue;
            }
            annotation_depth = 0;
            continue;
        }
        while code.first() == Some(&b'@') {
            let mut at = 1;
            while at < code.len() && (is_ident(code[at]) || code[at] == b'.') {
                at += 1;
            }
            let rest = &code[at..];
            if rest.first() == Some(&b'(') {
                let mut depth = 0i32;
                let mut close = None;
                for (n, &b) in rest.iter().enumerate() {
                    depth += i32::from(b == b'(') - i32::from(b == b')');
                    if depth == 0 {
                        close = Some(n);
                        break;
                    }
                }
                match close {
                    Some(n) => at += n + 1,
                    None => {
                        annotation_depth = depth;
                        at = code.len();
                    }
                }
            }
            code = &code[at..];
            let skip = code.iter().position(|&b| b != b' ' && b != b'\t').unwrap_or(code.len());
            code = &code[skip..];
        }
        if code.is_empty() {
            continue;
        }
        let top = scopes.last().map(|&(_, def)| def);
        let in_function = top.is_some_and(|def| scan.defs[def].kind == Kind::Function);
        let in_type = top.is_some_and(|def| scan.defs[def].kind == Kind::Type);
        let mut skip = None;
        if lang == Lang::Rust && find_word(code, "impl") == Some(0) {
            // `impl Foo` and `impl Trait for Foo` both open Foo: its methods belong to it.
            let text = String::from_utf8_lossy(code);
            let target = text.split(" for ").nth(1).unwrap_or(text.get(4..).unwrap_or(""));
            let target = target.trim_start().trim_start_matches('<');
            extra_owner = target.split(|c: char| !c.is_alphanumeric() && c != '_').find(|w| !w.is_empty() && w.chars().next().is_some_and(char::is_uppercase)).map(str::to_string);
        }
        let in_enum = top.is_some_and(|def| listing_enums.contains(&def));
        if in_enum && !code.starts_with(b"#") {
            let text = String::from_utf8_lossy(code).into_owned();
            let list = text.split(';').next().unwrap_or("");
            for member in list.split(',') {
                let name: String = member.trim().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                if is_type_name(&name) {
                    scan.defs.push(Def { name, kind: Kind::Type, line: n + 1 });
                }
            }
            if text.contains(';') {
                listing_enums.retain(|&def| Some(def) != top);
            }
        } else if pending.is_none() {
            if let Some((name, kind, drops)) = def_on(code, lang, in_type, in_function, scopes.is_empty()) {
                if kind == Kind::Type && find_word(code, "enum").is_some() {
                    listing_enums.push(scan.defs.len());
                }
                skip = Some(name.clone());
                scan.defs.push(Def { name, kind, line: n + 1 });
                pending = Some((scan.defs.len() - 1, drops));
                if let (Some((def, _)), Lang::Java | Lang::Kotlin | Lang::CSharp | Lang::Script | Lang::C) = (pending, lang) {
                    if kind == Kind::Type {
                        let text = String::from_utf8_lossy(code);
                        let rest = text.split_once(" extends ").or_else(|| text.split_once(" implements ")).or_else(|| text.split_once(" : ")).map(|(_, rest)| rest);
                        for base in rest.unwrap_or("").split([',', '{', ' ']) {
                            let base = base.trim().split('<').next().unwrap_or("");
                            if !base.is_empty() && !matches!(base, "public" | "private" | "protected" | "implements" | "extends") {
                                scan.bases.push((def, base.rsplit('.').next().unwrap_or("").to_string()));
                            }
                        }
                    }
                }
            }
        }
        // Calls are read between braces, so `fn a() { b(); }` credits b to a.
        let mut from = 0;
        for (at, &b) in code.iter().enumerate() {
            if matches!(b, b'{' | b'}') {
                calls_in(&code[from..at], skip.as_deref(), scopes.last().map(|&(_, def)| def), &mut scan.calls);
                from = at + 1;
            }
            match b {
                b'{' => {
                    depth += 1;
                    if let Some((def, _)) = pending.take() {
                        scopes.push((depth, def));
                    } else if let Some(name) = extra_owner.take() {
                        scan.defs.push(Def { name, kind: Kind::Type, line: n + 1 });
                        scopes.push((depth, scan.defs.len() - 1));
                    }
                }
                b'}' => {
                    if scopes.last().is_some_and(|&(at, _)| at == depth) {
                        scopes.pop();
                    }
                    depth = depth.saturating_sub(1);
                }
                b';' => {
                    if let Some((def, drops)) = pending.take() {
                        // A C prototype or `struct x y;`, not a definition.
                        if drops && def + 1 == scan.defs.len() {
                            scan.defs.pop();
                        }
                    }
                }
                _ => {}
            }
        }
        calls_in(&code[from..], skip.as_deref(), scopes.last().map(|&(_, def)| def), &mut scan.calls);
    }
}

pub fn scan(text: &[u8], lang: Lang) -> Scan {
    let cleaned = clean(text, lang);
    // Without the \r of a Windows file, or a line would never end the way a signature does.
    let chomp = |line: &[u8]| -> Vec<u8> { line.strip_suffix(b"\r").unwrap_or(line).to_vec() };
    let cleaned_lines: Vec<Vec<u8>> = cleaned.split(|&b| b == b'\n').map(chomp).collect();
    let raw_lines: Vec<Vec<u8>> = text.split(|&b| b == b'\n').map(chomp).collect();
    let lines: Vec<&[u8]> = cleaned_lines.iter().map(Vec::as_slice).collect();
    let raw: Vec<&[u8]> = raw_lines.iter().map(Vec::as_slice).collect();
    let mut out = Scan::default();
    for (n, line) in lines.iter().enumerate() {
        types_in(line, &mut out.types);
        imports_of(&String::from_utf8_lossy(raw.get(n).unwrap_or(&&b""[..])), &String::from_utf8_lossy(line), lang, &mut out.imports);
    }
    if lang == Lang::Python {
        scan_python(&lines, &mut out);
    } else {
        scan_braces(&lines, lang, &mut out);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(scan: &Scan) -> Vec<&str> {
        scan.defs.iter().map(|def| def.name.as_str()).collect()
    }

    fn called(scan: &Scan, by: &str) -> Vec<String> {
        let owner = scan.defs.iter().position(|def| def.name == by);
        scan.calls.iter().filter(|call| call.owner == owner).map(|call| call.name.clone()).collect()
    }

    #[test]
    fn comments_and_strings_are_not_code() {
        let s = scan(b"// fn ghost() {}\nfn real() { let a = \"fn fake() {}\"; helper(); }\n", Lang::Rust);
        assert_eq!(names(&s), ["real"]);
        assert_eq!(called(&s, "real"), ["helper"]);
    }

    #[test]
    fn python_scopes_follow_indentation() {
        let s = scan(b"import os, sys\nfrom a.b import c\nclass A(B, m.C):\n    def go(self):\n        self.run()\n        os.exit(1)\n\ndef after():\n    '''def nope(): pass'''\n    go()\n", Lang::Python);
        assert_eq!(names(&s), ["A", "go", "after"]);
        assert_eq!(called(&s, "after"), ["go"]);
        let go = called(&s, "go");
        assert_eq!(go, ["run", "exit"]);
        assert!(!s.calls[0].is_member && s.calls[1].is_member);
        assert_eq!(s.bases, [(0, "B".to_string()), (0, "C".to_string())]);
        assert!(s.imports.contains(&"a.b".to_string()) && s.imports.contains(&"sys".to_string()));
    }

    #[test]
    fn rust_impl_owns_its_methods_and_lifetimes_do_not_open_strings() {
        let s = scan(b"use crate::a::b;\nstruct Foo<'a> { x: &'a str }\nimpl<'a> Display for Foo<'a> {\n    fn fmt(&self) { write_it(); }\n}\nfn free() { Foo::new(); }\n", Lang::Rust);
        assert_eq!(names(&s), ["Foo", "Foo", "fmt", "free"]);
        assert_eq!(called(&s, "fmt"), ["write_it"]);
        assert_eq!(called(&s, "free"), ["new"]);
        assert_eq!(s.imports, ["crate::a::b"]);
    }

    #[test]
    fn scripts_find_arrows_classes_and_methods() {
        let s = scan(b"import { x } from './lib/x'\nexport const add = (a, b) => {\n  return plus(a, b)\n}\nclass Box extends Base {\n  open(key) {\n    this.check(key)\n  }\n}\nfunction top() { new Box().open(1) }\n", Lang::Script);
        assert_eq!(names(&s), ["add", "Box", "open", "top"]);
        assert_eq!(called(&s, "add"), ["plus"]);
        assert_eq!(called(&s, "open"), ["check"]);
        assert_eq!(s.bases, [(1, "Base".to_string())]);
        assert_eq!(s.imports, ["./lib/x"]);
    }

    #[test]
    fn java_methods_are_told_from_field_initialisers_and_control_flow() {
        let s = scan(b"import a.b.Tool;\npublic class Svc extends Base implements Api {\n  private int n = compute();\n  public int run(String s) throws Err {\n    if (ok(s)) { return tool.go(s); }\n    return 0;\n  }\n}\n", Lang::Java);
        assert_eq!(names(&s), ["Svc", "run"]);
        assert_eq!(called(&s, "run"), ["ok", "go"]);
        assert_eq!(s.imports, ["a.b.Tool"]);
        assert_eq!(s.bases.iter().map(|b| b.1.as_str()).collect::<Vec<_>>(), ["Base", "Api"]);
    }

    #[test]
    fn go_and_c_shapes() {
        let g = scan(b"package p\nimport \"x/y\"\nfunc (r *T) Do(a int) {\n  helper(a)\n}\ntype T struct {}\n", Lang::Go);
        assert_eq!(names(&g), ["Do"]);
        assert_eq!(called(&g, "Do"), ["helper"]);
        assert_eq!(g.imports, ["x/y"]);
        let c = scan(b"#include \"a.h\"\nint proto(int x);\nint run(int x) {\n  return proto(x);\n}\n", Lang::C);
        assert_eq!(names(&c), ["run"]);
        assert_eq!(called(&c, "run"), ["proto"]);
        assert_eq!(c.imports, ["a.h"]);
    }

    #[test]
    fn windows_line_ends_change_nothing() {
        let s = scan(b"import a.b.Tool;\r\npublic class Svc {\r\n  public int run() {\r\n    return go();\r\n  }\r\n}\r\n", Lang::Java);
        assert_eq!(names(&s), ["Svc", "run"]);
        assert_eq!(s.imports, ["a.b.Tool"]);
    }

    #[test]
    fn annotations_open_nothing() {
        let s = scan(b"class A {\n  @Path(\"x\")\n  @Column(name = \"y\")\n  public int run() {\n    return go();\n  }\n}\n", Lang::Java);
        assert_eq!(names(&s), ["A", "run"]);
        assert_eq!(called(&s, "run"), ["go"]);
    }

    #[test]
    fn a_python_signature_can_span_lines() {
        let s = scan(b"def f(\n    a,\n    b=g(),\n) -> int:\n    return h(a)\n\ndef k():\n    pass\n", Lang::Python);
        assert_eq!(names(&s), ["f", "k"]);
        assert_eq!(called(&s, "f"), ["g", "h"]);
    }

    #[test]
    fn enum_members_consts_and_generics_are_definitions() {
        let r = scan(b"pub const LIMIT: u32 = 3;\nconst fn make() {}\npub enum Mode {\n    Fast,\n    Slow(u8),\n}\n", Lang::Rust);
        assert_eq!(names(&r), ["LIMIT", "make", "Mode", "Fast", "Slow"]);
        let j = scan(b"public enum Size {\n  SMALL(1), LARGE(2);\n  public int get() {\n    return 1;\n  }\n}\n", Lang::Java);
        assert_eq!(names(&j), ["Size", "SMALL", "LARGE", "get"]);
        let t = scan(b"const TABLE = { a: 1 };\nclass C {\n  private async fetch<T>(p: string) {\n    go();\n  }\n}\n", Lang::Script);
        assert_eq!(names(&t), ["TABLE", "C", "fetch"]);
    }

    #[test]
    fn annotations_on_several_lines_or_before_a_method_open_nothing() {
        let s = scan(b"class A {\n  @Op(summary = \"x\",\n     text = \"y\")\n  @GET public int one() {\n    return go();\n  }\n  Uni<R> two(\n    @Q(\"a\") String a\n  );\n}\n", Lang::Java);
        assert_eq!(names(&s), ["A", "one", "two"]);
        assert_eq!(called(&s, "one"), ["go"]);
    }

    #[test]
    fn receivers_and_variable_types_are_read() {
        let s = scan(b"def f():\n    x = Store()\n    self.y.get(1)\n    x.put(2)\n", Lang::Python);
        assert_eq!(s.types, [("x".to_string(), "Store".to_string())]);
        assert_eq!(s.calls.iter().map(|c| c.receiver.clone()).collect::<Vec<_>>(), [None, Some("y".to_string()), Some("x".to_string())]);
        let j = scan(b"class A { private Svc svc; Tool t = new Tool(); int f(Other o) { return 0; } }", Lang::Java);
        assert!(j.types.contains(&("svc".to_string(), "Svc".to_string())) && j.types.contains(&("t".to_string(), "Tool".to_string())) && j.types.contains(&("o".to_string(), "Other".to_string())));
    }

    #[test]
    fn odd_input_does_not_panic() {
        for lang in [Lang::Python, Lang::Script, Lang::Rust, Lang::Java, Lang::CSharp, Lang::Go, Lang::C, Lang::Kotlin] {
            for text in [&b""[..], b"\"", b"/*", b"'''", b"fn", b"func (", b"}}}}", b"class", b"\xff\xfe(", b"\\"] {
                scan(text, lang);
            }
        }
    }
}

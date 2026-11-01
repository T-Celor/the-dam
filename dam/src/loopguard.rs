// TokenEconomizer's loop guard (Apache-2.0, Microsoft, https://github.com/microsoft/TokenEconomizer),
// rewritten from tokeneconomizer/loopguard: state.py, shell_edits.py, turn.py, rules.py, envelope.py.
// Its saved state is the same JSON, and a replay compares it with the Python on the same calls.
use fancy_regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::VecDeque;
use std::sync::LazyLock;

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a pattern fixed in this file")
}

fn hit(regex: &Regex, text: &str) -> bool {
    regex.is_match(text).unwrap_or(false)
}

static TEST_RUN: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)(runtests(\.py)?|\bpytest\b|py\.test|manage\.py\s+test|python\s+-m\s+unittest|\btox\b|\bnox\b|\btrial\b|python\d?\s+-c\b)"));
static ERR: LazyLock<Regex> = LazyLock::new(|| re(r"(?im)(Traceback \(most recent call last\)|\b\w*Error\b|\bFAILED\b|\bassert\b|Exception|errors during collection|^# fail [1-9]|^not ok \d|\b[1-9]\d* (?:failing|failures?)\b|Failures: [1-9]|^(?-i:--- FAIL: |FAIL\t|FAIL$))"));
static FAILURE_NOISE: LazyLock<Regex> = LazyLock::new(|| re(r"0x[0-9a-f]+|\d+|/\S+"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| re(r"\s+"));
static VERIFY: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)(?:^|[\s;&|(])(?:~?/)?(?:[\w.\-]+/)*(?:pytest|py\.test|tox|nox|python\d?(?:\.\d+)?\s+-m\s+(?:pytest|unittest)|manage\.py\s+test|runtests(?:\.py)?|jest|vitest|mocha|ava|playwright\s+test|(?:npm|pnpm|yarn|bun)\s+(?:run\s+)?test|go\s+test|cargo\s+(?:test|nextest)|(?:mvn|mvnw|\./mvnw)\s+(?:\S+\s+)*test|(?:gradle|gradlew|\./gradlew)\s+(?:\S+\s+)*test|make\s+(?:test|check)|ctest|rspec|phpunit|dotnet\s+test|swift\s+test|mix\s+test|deno\s+test|node\s+--test|bazel\s+test|(?:\./)?bin/test)(?![\w.\-/])"));
static VERIFY_AT_START: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\A(?:^|[\s;&|(])?(?:~?/)?(?:[\w.\-]+/)*(?:pytest|py\.test|tox|nox|python\d?(?:\.\d+)?\s+-m\s+(?:pytest|unittest)|manage\.py\s+test|runtests(?:\.py)?|jest|vitest|mocha|ava|playwright\s+test|(?:npm|pnpm|yarn|bun)\s+(?:run\s+)?test|go\s+test|cargo\s+(?:test|nextest)|(?:mvn|mvnw|\./mvnw)\s+(?:\S+\s+)*test|(?:gradle|gradlew|\./gradlew)\s+(?:\S+\s+)*test|make\s+(?:test|check)|ctest|rspec|phpunit|dotnet\s+test|swift\s+test|mix\s+test|deno\s+test|node\s+--test|bazel\s+test|(?:\./)?bin/test)(?![\w.\-/])"));
static QUOTED: LazyLock<Regex> = LazyLock::new(|| re(r#"'[^']*'|"(?:\\.|[^"\\])*""#));
static NOT_A_RUN: LazyLock<Regex> = LazyLock::new(|| re(r"\s(?:-h|--help|--version|--collect-only|--co|--list\S*)(?!\S)"));
static PREFIX: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*(?:(?:\w+=\S*|time|nice|nohup|command|env|timeout(?:\s+(?:-\S+|[A-Z]+|\d\S*))+)\s+)*"));
static REPORT: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*(?:echo|printf|true|:|exit)(?:\s|$)"));
static RUN_FAILED: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)\b[1-9]\d* (?:failed|errors?)\b.*\bin \d+(?:\.\d+)?s\b|^FAILED \((?:failures|errors)=|^tests finished:.*\b[1-9]\d* (?:failed|exceptions?)\b|^# fail [1-9]|^Tests:.*\b[1-9]\d* failed\b|^\s*Tests\s+[1-9]\d* failed\b|^\s*[1-9]\d* failing\b|^(?:--- FAIL: |FAIL\t|FAIL$)|^test result: FAILED|\b\d+ examples?, [1-9]\d* failures?\b|Tests run: \d+, Failures: (?:[1-9]|0, Errors: [1-9])|BUILD FAIL(?:URE|ED)"));
static RUN_PASSED: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)\b\d+ passed\b.*\bin \d+(?:\.\d+)?s\b|^OK(?: \(.*\))?\s*$|^tests finished: \d+ passed|^# fail 0\b|^Tests:\s+\d+ passed|^\s*Tests\s+\d+ passed|^\s*\d+ passing\b|^ok\s+\S+\s+(?:\d+(?:\.\d+)?s|\(cached\))|^test result: ok\.|\b\d+ examples?, 0 failures\b|BUILD SUCCESS"));
static HEREDOC: LazyLock<Regex> = LazyLock::new(|| re(r#"<<-?\s*(['"]?)([A-Za-z_]\w*)\1"#));
static SOURCE_FILE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\.(?:py|pyi|pyx|pxd|js|jsx|mjs|cjs|ts|tsx|rs|go|java|kt|kts|scala|c|cc|cpp|cxx|h|hh|hpp|rb|php|cs|swift|m|mm|sh|bash|zsh|lua|jl|ex|exs|erl|hs|ml|clj|sql|toml|ya?ml|json|cfg|ini|conf|md|rst|html?|css|scss|vue|svelte|xml|gradle|properties|in)$"));
static PYTHON: LazyLock<Regex> = LazyLock::new(|| re(r"(?:^|[\s/;&|(])python\d?(?:\.\d+)?\b"));
static PY_OPEN_LIT: LazyLock<Regex> = LazyLock::new(|| re(r#"open\(\s*(['"])([^'"\n]+)\1\s*,\s*(['"])([^'"]*)\3"#));
static PY_OPEN_VAR: LazyLock<Regex> = LazyLock::new(|| re(r#"open\(\s*([A-Za-z_]\w*)\s*,\s*(['"])([^'"]*)\2"#));
static PY_WRITE_LIT: LazyLock<Regex> = LazyLock::new(|| re(r#"(?:pathlib\.)?Path\(\s*(['"])([^'"\n]+)\1\s*\)\.write_(?:text|bytes)\("#));
static PY_WRITE_VAR: LazyLock<Regex> = LazyLock::new(|| re(r"\b([A-Za-z_]\w*)\.write_(?:text|bytes)\("));
static PATCH_FILE: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^\*\*\* (?:Update|Add|Delete) File:\s*(.+?)\s*$"));
static ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| re(r"^\w+="));
static SED_INPLACE: LazyLock<Regex> = LazyLock::new(|| re(r"^(?:-[a-zA-Z]*i|--in-place)"));
static PERL_INPLACE: LazyLock<Regex> = LazyLock::new(|| re(r"^-[a-zA-Z]*i"));
static PERL_SCRIPT: LazyLock<Regex> = LazyLock::new(|| re(r"^-[a-zA-Z]*e$"));
static QUOTED_PATH: LazyLock<Regex> = LazyLock::new(|| re(r#"['"]([^'"\n]+)['"]"#));

const NOT_GREEN: &str = "Tests are still NOT green.";
const JUDGE_BASE_GAP: i64 = 5;
const JUDGE_MAX_GAP: i64 = 20;
const MOST_RECENT: usize = 40;
const ENVELOPE_HISTORY: usize = 500;

pub fn canon_tool(name: &str) -> String {
    match name.to_lowercase().as_str() {
        "bash" | "shell" => "bash".to_string(),
        "read" | "view" => "view".to_string(),
        "grep" => "grep".to_string(),
        "glob" => "glob".to_string(),
        "edit" | "multiedit" | "str_replace" | "apply_patch" => "edit".to_string(),
        "write" | "create" => "create".to_string(),
        _ if name.is_empty() => "?".to_string(),
        _ => name.to_string(),
    }
}

pub fn basename(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or("")
}

// The lines as Python's str.splitlines sees them.
fn python_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        match rest.char_indices().find(|(_, c)| matches!(c, '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}')) {
            Some((at, c)) => {
                out.push(&rest[..at]);
                let mut next = at + c.len_utf8();
                if c == '\r' && rest[next..].starts_with('\n') {
                    next += 1;
                }
                rest = &rest[next..];
            }
            None => {
                out.push(rest);
                break;
            }
        }
    }

    out
}

fn is_python_space(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}

fn failure_signature(text: &str) -> Option<String> {
    let first = python_lines(text).into_iter().find(|line| hit(&ERR, line))?;
    let normal = FAILURE_NOISE.replace_all(first, "");
    let digest = md5::compute(normal.trim_matches(is_python_space).as_bytes());

    Some(format!("{digest:x}")[..10].to_string())
}

fn take_chars(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

fn bump(counts: &mut Vec<(String, i64)>, key: &str) {
    match counts.iter_mut().find(|(name, _)| name == key) {
        Some((_, count)) => *count += 1,
        None => counts.push((key.to_string(), 1)),
    }
}

fn text_of<'a>(args: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter().find_map(|key| args.get(key).and_then(Value::as_str).filter(|text| !text.is_empty())).unwrap_or("")
}

// Python's truth, as `round()` and `:.0f` are taken from it too: ties go to the even neighbour.
fn round_even(value: f64) -> f64 {
    let floor = value.floor();
    let diff = value - floor;
    if diff > 0.5 || (diff == 0.5 && floor % 2.0 != 0.0) {
        floor + 1.0
    } else {
        floor
    }
}

fn round3(value: f64) -> f64 {
    format!("{value:.3}").parse().unwrap_or(value)
}

#[derive(Clone, Debug, Default)]
pub struct SpiralState {
    pub turns: i64,
    pub edit_counts: Vec<(String, i64)>,
    pub edit_order: Vec<String>,
    pub error_outputs: i64,
    pub test_runs: i64,
    pub fail_sigs: Vec<(String, i64)>,
    pub repeated_fail_sigs: i64,
    pub recent: Vec<String>,
}

fn counts_to_value(counts: &[(String, i64)]) -> Value {
    Value::Object(counts.iter().map(|(key, count)| (key.clone(), json!(count))).collect())
}

fn counts_from_value(value: Option<&Value>) -> Vec<(String, i64)> {
    value.and_then(Value::as_object).map_or_else(Vec::new, |map| map.iter().map(|(key, count)| (key.clone(), count.as_i64().unwrap_or(0))).collect())
}

fn strings_from_value(value: Option<&Value>) -> Vec<String> {
    value.and_then(Value::as_array).map_or_else(Vec::new, |list| list.iter().filter_map(|item| item.as_str().map(str::to_string)).collect())
}

impl SpiralState {
    pub fn edit_churn(&self) -> i64 {
        self.edit_counts.iter().map(|(_, count)| *count).max().unwrap_or(0)
    }

    pub fn distinct_edit_files(&self) -> i64 {
        self.edit_counts.len() as i64
    }

    pub fn stagnation(&self) -> f64 {
        let total: i64 = self.fail_sigs.iter().map(|(_, count)| *count).sum();
        if total > 0 {
            self.repeated_fail_sigs as f64 / total as f64
        } else {
            0.0
        }
    }

    pub fn update(&mut self, tool: &str, args: &Value, result: &str, failed: Option<bool>) {
        self.turns += 1;
        let tool = canon_tool(tool);
        let mut action = tool.clone();
        if matches!(tool.as_str(), "edit" | "str_replace" | "create") {
            let name = basename(text_of(args, &["path", "filePath", "file_path"]));
            if !name.is_empty() {
                self.edit_order.push(name.to_string());
                bump(&mut self.edit_counts, name);
                action = format!("edit {name}");
            }
        }
        let command = if tool == "bash" { args.get("command").and_then(Value::as_str).unwrap_or("") } else { "" };
        if !command.is_empty() {
            let short = SPACES.replace_all(command.trim_matches(is_python_space), " ");
            action = format!("$ {}", take_chars(&short, 90));
            if hit(&TEST_RUN, command) {
                self.test_runs += 1;
            }
        } else if matches!(tool.as_str(), "grep" | "glob") {
            let pattern = match args.get("pattern") {
                Some(Value::String(text)) => text.clone(),
                Some(Value::Null) | None => String::new(),
                Some(other) => other.to_string(),
            };
            action = format!("{tool} {}", take_chars(&pattern, 40));
        } else if tool == "view" {
            action = format!("view {}", basename(text_of(args, &["path", "file_path"])));
        }
        let mut fail = None;
        let can_fail = if matches!(tool.as_str(), "view" | "grep" | "glob") { failed == Some(true) } else { failed != Some(false) };
        if !result.is_empty() && can_fail && hit(&ERR, result) {
            self.error_outputs += 1;
            if let Some(signature) = failure_signature(result) {
                if self.fail_sigs.iter().any(|(key, _)| *key == signature) {
                    self.repeated_fail_sigs += 1;
                }
                bump(&mut self.fail_sigs, &signature);
                fail = Some(signature);
            }
        }
        let tag = fail.map_or(String::new(), |signature| format!("  [err#{signature}]"));
        self.recent.push(format!("{:>3}: {action}{tag}", self.turns));
        if self.recent.len() > MOST_RECENT {
            self.recent.drain(..self.recent.len() - MOST_RECENT);
        }
    }

    pub fn recent_digest(&self, count: usize, envelope: Option<f64>) -> String {
        let mut head = format!(
            "turns={} edit_churn={} distinct_edit_files={} error_outputs={} test_runs={} repeated_identical_failures={} stagnation={:.2}",
            self.turns,
            self.edit_churn(),
            self.distinct_edit_files(),
            self.error_outputs,
            self.test_runs,
            self.repeated_fail_sigs,
            self.stagnation()
        );
        if let Some(envelope) = envelope.filter(|value| *value != 0.0) {
            let ratio = self.turns as f64 / envelope;
            head.push_str(&format!(
                "\nrepo_context: tasks here usually RESOLVE within ~{envelope:.0} turns; this trajectory is at {} turns = {ratio:.1}x that typical length. {NOT_GREEN} A trajectory far past the typical resolved length with recurring failures is likely a dead-end loop, even if it touches many files.",
                self.turns
            ));
        }
        let from = self.recent.len().saturating_sub(count);

        format!("{head}\nRecent actions (oldest->newest):\n{}", self.recent[from..].join("\n"))
    }

    pub fn snapshot(&self) -> Value {
        json!({"turns": self.turns, "edit_churn": self.edit_churn(), "distinct_edit_files": self.distinct_edit_files(), "error_outputs": self.error_outputs, "test_runs": self.test_runs, "stagnation": round3(self.stagnation()), "repeated_fail_sigs": self.repeated_fail_sigs})
    }

    fn to_value(&self) -> Value {
        json!({"turns": self.turns, "edit_counts": counts_to_value(&self.edit_counts), "edit_order": self.edit_order, "error_outputs": self.error_outputs, "test_runs": self.test_runs, "fail_sigs": counts_to_value(&self.fail_sigs), "repeated_fail_sigs": self.repeated_fail_sigs, "recent": self.recent})
    }

    fn from_value(value: Option<&Value>) -> Self {
        let int = |key: &str| value.and_then(|v| v.get(key)).and_then(Value::as_i64).unwrap_or(0);
        let part = |key: &str| value.and_then(|v| v.get(key));
        SpiralState {
            turns: int("turns"),
            edit_counts: counts_from_value(part("edit_counts")),
            edit_order: strings_from_value(part("edit_order")),
            error_outputs: int("error_outputs"),
            test_runs: int("test_runs"),
            fail_sigs: counts_from_value(part("fail_sigs")),
            repeated_fail_sigs: int("repeated_fail_sigs"),
            recent: strings_from_value(part("recent")),
        }
    }

    fn fold_shell_edits(&mut self, names: &[String]) {
        for name in names {
            self.edit_order.push(name.clone());
            bump(&mut self.edit_counts, name);
        }
        if let (false, Some(last)) = (names.is_empty(), self.recent.last_mut()) {
            let more = if names.len() > 4 { ", ..." } else { "" };
            let shown = names[..names.len().min(4)].join(", ");
            last.push_str(&format!("  [edits {shown}{more}]"));
        }
    }
}

// shlex.split: None where Python raises on an unbalanced quote or a final backslash.
fn shlex_split(text: &str) -> Option<Vec<String>> {
    let (mut out, mut token, mut in_token) = (Vec::new(), String::new(), false);
    let mut chars = text.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        match quote {
            Some('"') if c == '\\' => {
                let next = chars.next()?;
                if next != '"' && next != '\\' {
                    token.push('\\');
                }
                token.push(next);
            }
            Some(q) if c == q => quote = None,
            Some(_) => token.push(c),
            None => match c {
                ' ' | '\t' | '\r' | '\n' => {
                    if in_token {
                        out.push(std::mem::take(&mut token));
                        in_token = false;
                    }
                }
                '\\' => {
                    token.push(chars.next()?);
                    in_token = true;
                }
                '\'' | '"' => {
                    quote = Some(c);
                    in_token = true;
                }
                _ => {
                    token.push(c);
                    in_token = true;
                }
            },
        }
    }
    if quote.is_some() {
        return None;
    }
    if in_token {
        out.push(token);
    }

    Some(out)
}

// shlex.shlex(text, posix=False, punctuation_chars="();<>|&\n") with whitespace " \t\r" and
// whitespace_split, word for word as its read_token reads: None where Python raises.
fn shell_tokens(text: &str) -> Option<Vec<String>> {
    const PUNCTUATION: &str = "();<>|&\n";
    const WHITESPACE: &str = " \t\r";
    let chars: Vec<char> = text.chars().collect();
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_' || "~-./*?=".contains(c);
    let mut at = 0;
    let mut pushback: VecDeque<char> = VecDeque::new();
    let mut out = Vec::new();
    let mut state = Some(' ');
    while state.is_some() {
        let mut token = String::new();
        loop {
            let next = pushback.pop_back().or_else(|| {
                let c = chars.get(at).copied();
                at += usize::from(c.is_some());
                c
            });
            let skip_comment = |at: &mut usize| {
                while *at < chars.len() && chars[*at] != '\n' {
                    *at += 1;
                }
                *at += usize::from(*at < chars.len());
            };
            match state {
                Some(' ') => match next {
                    None => {
                        state = None;
                        break;
                    }
                    Some(c) if WHITESPACE.contains(c) => {
                        if !token.is_empty() {
                            break;
                        }
                    }
                    Some('#') => skip_comment(&mut at),
                    Some(c) if is_word(c) => {
                        token.push(c);
                        state = Some('a');
                    }
                    Some(c) if PUNCTUATION.contains(c) => {
                        token.push(c);
                        state = Some('c');
                    }
                    Some(c @ ('\'' | '"')) => {
                        token.push(c);
                        state = Some(c);
                    }
                    Some(c) => {
                        token.push(c);
                        state = Some('a');
                    }
                },
                Some(q @ ('\'' | '"')) => match next {
                    None => return None,
                    Some(c) => {
                        token.push(c);
                        if c == q {
                            state = Some(' ');
                            break;
                        }
                    }
                },
                _ => match next {
                    None => {
                        state = None;
                        break;
                    }
                    Some(c) if WHITESPACE.contains(c) => {
                        state = Some(' ');
                        if !token.is_empty() {
                            break;
                        }
                    }
                    Some('#') => skip_comment(&mut at),
                    Some(c) if state == Some('c') => {
                        if PUNCTUATION.contains(c) {
                            token.push(c);
                        } else {
                            pushback.push_back(c);
                            state = Some(' ');
                            break;
                        }
                    }
                    Some(c) if !PUNCTUATION.contains(c) => token.push(c),
                    Some(c) => {
                        pushback.push_back(c);
                        state = Some(' ');
                        if !token.is_empty() {
                            break;
                        }
                    }
                },
            }
        }
        if token.is_empty() && state.is_none() {
            break;
        }
        // Python's iterator stops at the first empty token.
        if token.is_empty() {
            break;
        }
        out.push(token);
    }

    Some(out)
}

fn unquote(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() >= 2 && chars[0] == chars[chars.len() - 1] && matches!(chars[0], '\'' | '"') {
        chars[1..chars.len() - 1].iter().collect()
    } else {
        token.to_string()
    }
}

fn split_heredocs(command: &str) -> (String, Vec<String>) {
    let (mut shell, mut bodies, mut tag, mut body): (Vec<&str>, Vec<String>, Option<String>, Vec<&str>) = (Vec::new(), Vec::new(), None, Vec::new());
    for line in command.split('\n') {
        if let Some(open) = &tag {
            if line.trim_matches(is_python_space) == open {
                bodies.push(body.join("\n"));
                tag = None;
                body.clear();
            } else {
                body.push(line);
            }
            continue;
        }
        shell.push(line);
        if let Ok(Some(found)) = HEREDOC.captures(line) {
            tag = found.get(2).map(|m| m.as_str().to_string());
        }
    }
    if tag.is_some() {
        bodies.push(body.join("\n"));
    }

    (shell.join("\n"), bodies)
}

fn shell_commands(shell: &str) -> (Vec<Vec<String>>, Vec<String>) {
    const OPERATORS: [&str; 10] = ["&&", "||", ";", ";;", "|", "|&", "&", "\n", "(", ")"];
    const WRITES: [&str; 5] = [">", ">>", ">|", "&>", "&>>"];
    const SKIPS: [&str; 6] = ["<", "<<", "<<<", "<&", ">&", "<>"];
    let streams: Vec<Vec<String>> = match shell_tokens(shell) {
        Some(tokens) => vec![tokens],
        None => shell
            .split('\n')
            .filter_map(|line| {
                let mut tokens = shell_tokens(line)?;
                tokens.push("\n".to_string());
                Some(tokens)
            })
            .collect(),
    };
    let (mut commands, mut writes) = (Vec::new(), Vec::new());
    for tokens in streams {
        let mut current: Vec<String> = Vec::new();
        let mut i = 0;
        while i < tokens.len() {
            let token = tokens[i].as_str();
            if OPERATORS.contains(&token) {
                if !current.is_empty() {
                    commands.push(std::mem::take(&mut current));
                }
            } else if WRITES.contains(&token) || SKIPS.contains(&token) {
                if current.last().is_some_and(|last| !last.is_empty() && last.chars().all(char::is_numeric)) {
                    current.pop();
                }
                if let (true, Some(next)) = (WRITES.contains(&token), tokens.get(i + 1)) {
                    if !OPERATORS.contains(&next.as_str()) {
                        writes.push(unquote(next));
                    }
                }
                i += 1;
            } else {
                current.push(unquote(token));
            }
            i += 1;
        }
        if !current.is_empty() {
            commands.push(current);
        }
    }

    (commands, writes)
}

fn inplace_files(tokens: &[String], is_inplace: impl Fn(&str) -> bool, is_script: impl Fn(&str) -> bool) -> Vec<String> {
    if !tokens[1..].iter().any(|token| is_inplace(token)) {
        return Vec::new();
    }
    let (mut files, mut have_script, mut skip) = (Vec::new(), false, false);
    for token in &tokens[1..] {
        if skip {
            skip = false;
            have_script = true;
        } else if token.is_empty() || token.starts_with('-') {
            skip = !token.is_empty() && is_script(token);
        } else if !have_script {
            have_script = true;
        } else {
            files.push(token.clone());
        }
    }

    files
}

fn captures_all(regex: &Regex, text: &str, group: usize) -> Vec<String> {
    regex.captures_iter(text).filter_map(Result::ok).filter_map(|found| found.get(group).map(|m| m.as_str().to_string())).collect()
}

fn opens_for_writing(mode: &str) -> bool {
    mode.contains(['w', 'a', 'x', '+'])
}

fn python_writes(text: &str) -> Vec<String> {
    let mut out: Vec<String> = PY_OPEN_LIT
        .captures_iter(text)
        .filter_map(Result::ok)
        .filter(|found| opens_for_writing(found.get(4).map_or("", |m| m.as_str())))
        .filter_map(|found| found.get(2).map(|m| m.as_str().to_string()))
        .collect();
    let mut variables: Vec<String> = PY_OPEN_VAR
        .captures_iter(text)
        .filter_map(Result::ok)
        .filter(|found| opens_for_writing(found.get(3).map_or("", |m| m.as_str())))
        .filter_map(|found| found.get(1).map(|m| m.as_str().to_string()))
        .collect();
    out.extend(captures_all(&PY_WRITE_LIT, text, 2));
    variables.extend(captures_all(&PY_WRITE_VAR, text, 1));
    let mut seen: Vec<String> = Vec::new();
    for variable in variables {
        if seen.contains(&variable) {
            continue;
        }
        seen.push(variable.clone());
        let name = fancy_regex::escape(&variable);
        let assigned = Regex::new(&format!(r#"\b{name}\s*=\s*(?:(?:pathlib\.)?Path\(\s*)?(['"])([^'"\n]+)\1"#)).expect("an escaped name");
        let looped = Regex::new(&format!(r"\bfor\s+{name}\s+in\s+[\[(]([^\])]*)[\])]")).expect("an escaped name");
        let mut paths = captures_all(&assigned, text, 2);
        if let Ok(Some(found)) = looped.captures(text) {
            paths.extend(captures_all(&QUOTED_PATH, found.get(1).map_or("", |m| m.as_str()), 1));
        }
        if paths.is_empty() {
            out.push("(script)".to_string());
        } else {
            out.extend(paths);
        }
    }

    out
}

pub fn shell_edit_targets(command: &str) -> Vec<String> {
    if command.trim_matches(is_python_space).is_empty() {
        return Vec::new();
    }
    let (shell, bodies) = split_heredocs(command);
    let (commands, writes) = shell_commands(&shell);
    let mut found: Vec<String> = writes.into_iter().filter(|write| hit(&SOURCE_FILE, write)).collect();
    for tokens in &commands {
        let mut tokens: &[String] = tokens;
        while tokens.first().is_some_and(|first| matches!(first.as_str(), "sudo" | "command" | "env" | "time" | "nohup") || hit(&ASSIGNMENT, first)) {
            tokens = &tokens[1..];
        }
        let Some(first) = tokens.first() else { continue };
        let head = first.rsplit('/').next().unwrap_or("");
        let word = |at: usize| tokens.get(at).map_or("", String::as_str);
        match head {
            "sed" => found.extend(inplace_files(tokens, |t| hit(&SED_INPLACE, t), |t| matches!(t, "-e" | "-f" | "--expression" | "--file"))),
            "perl" => found.extend(inplace_files(tokens, |t| hit(&PERL_INPLACE, t), |t| hit(&PERL_SCRIPT, t))),
            "tee" => found.extend(tokens[1..].iter().filter(|t| !t.starts_with('-') && hit(&SOURCE_FILE, t)).cloned()),
            "apply_patch" => {
                let mut text: Vec<String> = bodies.clone();
                text.push(tokens[1..].join(" "));
                let files = captures_all(&PATCH_FILE, &text.join("\n"), 1);
                if files.is_empty() {
                    found.push("(patch)".to_string());
                } else {
                    found.extend(files);
                }
            }
            "patch" => found.push("(patch)".to_string()),
            "git" if word(1) == "apply" => found.push("(patch)".to_string()),
            "git" if tokens.len() > 2 && matches!(word(1), "checkout" | "restore") => {
                let rest: &[String] = match tokens.iter().position(|t| t == "--") {
                    Some(at) => &tokens[at + 1..],
                    None if word(1) == "restore" => &tokens[2..],
                    None => &[],
                };
                found.extend(rest.iter().filter(|t| !t.starts_with('-')).cloned());
            }
            _ => {}
        }
    }
    if hit(&PYTHON, &shell) {
        let mut text = vec![shell.clone()];
        text.extend(bodies);
        found.extend(python_writes(&text.join("\n")));
    }
    let mut names: Vec<String> = Vec::new();
    for path in found {
        let is_mark = path.starts_with('(');
        if !is_mark && (path.is_empty() || path.starts_with("/dev/") || path.starts_with("/proc/") || path.starts_with("/tmp/")) {
            continue;
        }
        let name = if is_mark { path.clone() } else { basename(&path).to_string() };
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }

    names
}

fn split_on_separators(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut pieces = vec![String::new()];
    let mut i = 0;
    while i < chars.len() {
        let two: String = chars[i..chars.len().min(i + 2)].iter().collect();
        let separator = if matches!(two.as_str(), "&&" | "||" | "|&") {
            Some(two)
        } else if chars[i] == ';' || chars[i] == '\n' || (chars[i] == '|' && (i == 0 || chars[i - 1] != '>')) {
            Some(chars[i].to_string())
        } else {
            None
        };
        match separator {
            Some(separator) => {
                i += separator.chars().count();
                pieces.push(separator);
                pieces.push(String::new());
            }
            None => {
                pieces.last_mut().expect("never empty").push(chars[i]);
                i += 1;
            }
        }
    }

    pieces
}

#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub enum TestStatus {
    Own,
    Hidden,
    Mixed,
}

pub fn status_of_tests(command: Option<&str>) -> Option<TestStatus> {
    let mut command = command?.to_string();
    if let Some(parts) = shlex_split(&command) {
        if parts.len() >= 3 && matches!(basename_os(&parts[0]), "sh" | "bash" | "zsh" | "dash" | "ksh") && parts[1].starts_with('-') && parts[1].contains('c') {
            command = parts[2].clone();
        }
    }
    let quoted = QUOTED.replace_all(&command, "Q");
    let bare: String = quoted.chars().map(|c| if matches!(c, '(' | ')' | '{' | '}') { ' ' } else { c }).collect();
    let mut pieces = split_on_separators(&bare);
    while pieces.len() > 1 && pieces.last().is_some_and(|piece| piece.trim_matches(is_python_space).is_empty()) {
        pieces.truncate(pieces.len() - 2);
    }
    let pipefail = command.contains("pipefail");
    let mut found = None;
    for i in (0..pieces.len()).step_by(2) {
        let stage = &pieces[i];
        let bare_stage = PREFIX.replace(stage, "");
        if !hit(&VERIFY_AT_START, &bare_stage) || hit(&NOT_A_RUN, stage) {
            continue;
        }
        let mut status = TestStatus::Own;
        let mut at = i + 1;
        while at + 1 < pieces.len() {
            let (separator, then) = (pieces[at].as_str(), pieces[at + 1].as_str());
            at += 2;
            if separator == "&&" || (pipefail && matches!(separator, "|" | "|&")) {
                continue;
            }
            if matches!(separator, "|" | "|&" | "||") {
                status = TestStatus::Hidden;
                break;
            }
            if !hit(&REPORT, then) {
                status = TestStatus::Mixed;
                break;
            }
            status = TestStatus::Hidden;
        }
        if found.is_none_or(|kept| status > kept) {
            found = Some(status);
        }
    }

    found
}

fn basename_os(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or("")
}

pub fn runner_verdict(text: &str) -> Option<bool> {
    if text.is_empty() {
        return None;
    }
    if hit(&RUN_FAILED, text) {
        return Some(true);
    }

    hit(&RUN_PASSED, text).then_some(false)
}

pub type Thresholds = Map<String, Value>;

fn number(thr: &Thresholds, key: &str) -> f64 {
    thr.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

pub fn scaled_thresholds(level: &str) -> Thresholds {
    let mut thr = json!({"min_turns": 20, "edit_churn": 4, "distinct_edit_files": 3, "error_outputs": 6, "stagnation": 0.20, "envelope_margin": 1.25, "envelope_pct": 90, "envelope_min_samples": 5, "judge_confidence": 0.7, "backstop_mult": 2.5, "gate_min_turns": 18, "gate_error_outputs": 8});
    let over = match level {
        "conservative" => json!({"judge_confidence": 0.85, "envelope_margin": 1.5, "backstop_mult": 3.5, "stagnation": 0.30, "gate_min_turns": 24, "min_turns": 28, "edit_churn": 5, "error_outputs": 8, "envelope_min_samples": 7}),
        "aggressive" => json!({"judge_confidence": 0.55, "envelope_margin": 1.0, "backstop_mult": 1.8, "stagnation": 0.12, "gate_min_turns": 12, "min_turns": 14, "edit_churn": 3, "error_outputs": 4, "envelope_min_samples": 3, "nudge_on_ambiguous": true}),
        _ => json!({}),
    };
    let map = thr.as_object_mut().expect("an object");
    map.extend(over.as_object().expect("an object").clone());

    map.clone()
}

pub fn gate_for_judge(state: &SpiralState, thr: &Thresholds) -> bool {
    state.turns as f64 >= number(thr, "gate_min_turns") && (state.repeated_fail_sigs >= 1 || state.error_outputs as f64 >= number(thr, "gate_error_outputs"))
}

pub fn is_stuck(state: &SpiralState, thr: &Thresholds) -> bool {
    if (state.turns as f64) < number(thr, "min_turns") {
        return false;
    }
    let churny = state.edit_churn() as f64 >= number(thr, "edit_churn") || state.distinct_edit_files() as f64 >= number(thr, "distinct_edit_files");

    churny && state.error_outputs as f64 >= number(thr, "error_outputs") && state.stagnation() >= number(thr, "stagnation")
}

#[derive(Clone, Debug)]
pub struct Verdict {
    pub stuck: Option<bool>,
    pub confidence: f64,
    pub reason: String,
    pub tokens: f64,
}

pub fn assess_abort(state: &SpiralState, envelope: Option<f64>, judge: Option<&mut dyn FnMut(&str) -> Option<Verdict>>, thr: &Thresholds) -> (bool, String) {
    let backstop = thr.get("backstop_turns").and_then(Value::as_f64).or(envelope.map(|value| value * number(thr, "backstop_mult")));
    if let Some(turns) = backstop {
        if state.turns as f64 >= turns && is_stuck(state, thr) {
            return (true, format!("backstop:{}>={turns:.0}turns stagnation={:.2}", state.turns, state.stagnation()));
        }
    }
    if let Some(judge) = judge {
        let over_envelope = envelope.is_some_and(|value| state.turns as f64 >= value) && thr.get("gate_over_envelope").and_then(Value::as_bool).unwrap_or(true);
        if !(gate_for_judge(state, thr) || over_envelope) {
            return (false, "gate-closed".to_string());
        }
        let Some(verdict) = judge(&state.recent_digest(24, envelope)) else {
            return (false, "judge-throttled".to_string());
        };
        if verdict.stuck == Some(true) && verdict.confidence >= number(thr, "judge_confidence") {
            return (true, format!("judge-stuck@{:.2}:{}", verdict.confidence, take_chars(&verdict.reason, 60)));
        }
        if verdict.stuck == Some(false) {
            return (false, "judge-productive".to_string());
        }
        return (false, "judge-ambiguous-spared".to_string());
    }
    if !is_stuck(state, thr) {
        return (false, "not-stuck".to_string());
    }
    if let Some(value) = envelope.filter(|value| state.turns as f64 >= *value) {
        return (true, format!("envelope:{}>={value:.0}", state.turns));
    }

    (false, "stuck-but-within-envelope".to_string())
}

pub fn nudge_text(state: &SpiralState) -> String {
    // The first of the busiest, as Python's max() keeps.
    let mut busiest = "one file";
    let mut most = i64::MIN;
    for (name, count) in &state.edit_counts {
        if *count > most {
            (busiest, most) = (name, *count);
        }
    }
    let edits: i64 = state.edit_counts.iter().map(|(_, count)| *count).sum();

    format!(
        "Process check (automated): so far you've made {edits} edits across {} file(s) (most to `{busiest}`) and run {} verification(s), and the same failure has recurred {} time(s) without the tests going green. This looks like a repetitive loop. Do NOT stop — instead step back and try a MATERIALLY DIFFERENT hypothesis: re-read the failing test's expectation, consider a different file or root cause than the ones you've been editing, and verify the new hypothesis directly before editing again.",
        state.distinct_edit_files(),
        state.test_runs,
        state.repeated_fail_sigs
    )
}

fn percentile(values: &[f64], p: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    if n == 1 {
        return Some(sorted[0]);
    }
    let i = (p / 100.0) * (n - 1) as f64;
    let low = i as usize;
    if low + 1 >= n {
        return Some(sorted[n - 1]);
    }

    Some(sorted[low] + (i - low as f64) * (sorted[low + 1] - sorted[low]))
}

#[derive(Debug)]
pub struct EnvelopeTracker {
    pub level: String,
    base: Thresholds,
    margin: f64,
    pct: f64,
    min_samples: usize,
    pub resolved_turns: Vec<i64>,
    pub resolved_snaps: Vec<Value>,
}

impl EnvelopeTracker {
    pub fn new(level: &str, overrides: &Thresholds) -> Self {
        let base = scaled_thresholds(level);
        let chosen = |key: &str| overrides.get(key).and_then(Value::as_f64).unwrap_or_else(|| number(&base, key));
        let margin = overrides.get("envelope_margin").and_then(Value::as_f64).filter(|value| *value != 0.0).unwrap_or_else(|| number(&base, "envelope_margin"));
        let (pct, min_samples) = (chosen("envelope_pct"), chosen("envelope_min_samples") as usize);

        EnvelopeTracker { level: level.to_string(), base, margin, pct, min_samples, resolved_turns: Vec::new(), resolved_snaps: Vec::new() }
    }

    // Reads the same envelope.json the Python writes.
    pub fn load(saved: &Value, level: &str, overrides: &Thresholds) -> Self {
        let mut tracker = Self::new(level, overrides);
        tracker.resolved_turns = saved.get("resolved_turns").and_then(Value::as_array).map_or_else(Vec::new, |list| list.iter().filter_map(Value::as_i64).filter(|turn| *turn != 0).collect());
        tracker.resolved_snaps = saved.get("resolved_snaps").and_then(Value::as_array).map_or_else(Vec::new, |list| list.iter().filter(|item| item.is_object()).cloned().collect());

        tracker
    }

    pub fn to_saved(&self) -> Value {
        let tail = |len: usize| len.saturating_sub(ENVELOPE_HISTORY);
        json!({"level": self.level, "resolved_turns": self.resolved_turns[tail(self.resolved_turns.len())..], "resolved_snaps": self.resolved_snaps[tail(self.resolved_snaps.len())..]})
    }

    pub fn update(&mut self, turns: i64, resolved: bool, snapshot: Value) {
        if resolved && turns != 0 {
            self.resolved_turns.push(turns);
            self.resolved_snaps.push(snapshot);
        }
    }

    fn turns(&self) -> Vec<f64> {
        self.resolved_turns.iter().map(|turn| *turn as f64).collect()
    }

    pub fn envelope(&self) -> Option<f64> {
        if self.resolved_turns.len() < self.min_samples {
            return None;
        }

        percentile(&self.turns(), self.pct).map(|value| value * self.margin)
    }

    pub fn thresholds(&self) -> Thresholds {
        let base = &self.base;
        let mut thr = base.clone();
        if let Some(envelope) = self.envelope() {
            thr.insert("backstop_turns".to_string(), json!(envelope * number(&thr, "backstop_mult")));
        }
        if self.resolved_turns.len() < self.min_samples {
            return thr;
        }
        let p90 = percentile(&self.turns(), 90.0).unwrap_or(0.0);
        let clamp = |value: f64, low: f64, high: f64| round_even(low.max(high.min(round_even(value)))) as i64;
        thr.insert("gate_min_turns".to_string(), json!(clamp(0.6 * p90, 6.0, number(base, "gate_min_turns"))));
        thr.insert("min_turns".to_string(), json!(clamp(0.6 * p90, 8.0, number(base, "min_turns"))));
        if !self.resolved_snaps.is_empty() {
            let signal = |key: &str| -> Vec<f64> { self.resolved_snaps.iter().map(|snap| snap.get(key).and_then(Value::as_f64).unwrap_or(0.0)).collect() };
            let raised = |key: &str| (number(base, key)).max((percentile(&signal(key), 90.0).unwrap_or(0.0) + 1.0).ceil()) as i64;
            thr.insert("edit_churn".to_string(), json!(raised("edit_churn")));
            thr.insert("distinct_edit_files".to_string(), json!(raised("distinct_edit_files")));
            let errors = raised("error_outputs");
            thr.insert("error_outputs".to_string(), json!(errors));
            thr.insert("gate_error_outputs".to_string(), json!((number(base, "gate_error_outputs") as i64).max(errors)));
            thr.insert("stagnation".to_string(), json!(number(base, "stagnation").max(round3(percentile(&signal("stagnation"), 90.0).unwrap_or(0.0)))));
        }

        thr
    }
}

// The thresholds a turn is checked against: learned, then a person's pinned values on top.
pub fn pinned_thresholds(tracker: &EnvelopeTracker, overrides: &Thresholds) -> Thresholds {
    let mut thr = tracker.thresholds();
    for (key, value) in overrides {
        if !matches!(key.as_str(), "envelope_margin" | "envelope_pct" | "envelope_min_samples") {
            thr.insert(key.clone(), value.clone());
        }
    }
    if let (true, Some(envelope)) = (overrides.contains_key("backstop_mult"), tracker.envelope()) {
        thr.insert("backstop_turns".to_string(), json!(envelope * number(&thr, "backstop_mult")));
    }

    thr
}

#[derive(Clone, Debug, Default)]
pub struct TurnGuard {
    pub turn_key: String,
    pub st: SpiralState,
    pub whole: SpiralState,
    pub cum_turns: i64,
    pub segment: i64,
    pub nudges_done: i64,
    pub aborted: bool,
    pub abort_delivered: bool,
    pub events: Vec<Value>,
    pub judge_last: i64,
    pub judge_gap: i64,
    pub judge_calls: i64,
    pub judge_tokens: f64,
    pub writer_tokens: f64,
    pub last_edit: i64,
    pub last_verify: i64,
    pub last_verify_failed: bool,
    pub edits: i64,
    pub verifications: i64,
    pub judge_pending: Value,
    pub nudge_ready: Value,
}

impl TurnGuard {
    pub fn new(turn_key: &str) -> Self {
        TurnGuard { turn_key: turn_key.to_string(), judge_last: -999, judge_gap: JUDGE_BASE_GAP, judge_pending: Value::Null, nudge_ready: Value::Null, ..Default::default() }
    }

    pub fn from_value(data: &Value) -> Self {
        let int = |key: &str, default: i64| data.get(key).and_then(Value::as_i64).unwrap_or(default);
        let flag = |key: &str| data.get(key).and_then(Value::as_bool).unwrap_or(false);
        let float = |key: &str| data.get(key).and_then(Value::as_f64).unwrap_or(0.0);
        let truthy = |key: &str| data.get(key).filter(|value| !value.is_null() && **value != json!("") && **value != json!({})).cloned().unwrap_or(Value::Null);
        TurnGuard {
            turn_key: data.get("turn_key").and_then(Value::as_str).unwrap_or("").to_string(),
            st: SpiralState::from_value(data.get("st")),
            whole: SpiralState::from_value(data.get("whole")),
            cum_turns: int("cum_turns", 0),
            segment: int("segment", 0),
            nudges_done: int("nudges_done", 0),
            aborted: flag("aborted"),
            abort_delivered: flag("abort_delivered"),
            events: data.get("events").and_then(Value::as_array).cloned().unwrap_or_default(),
            judge_last: int("judge_last", -999),
            judge_gap: int("judge_gap", JUDGE_BASE_GAP),
            judge_calls: int("judge_calls", 0),
            judge_tokens: float("judge_tokens"),
            writer_tokens: float("writer_tokens"),
            last_edit: int("last_edit", 0),
            last_verify: int("last_verify", 0),
            last_verify_failed: flag("last_verify_failed"),
            edits: int("edits", 0),
            verifications: int("verifications", 0),
            judge_pending: truthy("judge_pending"),
            nudge_ready: truthy("nudge_ready"),
        }
    }

    pub fn to_value(&self) -> Value {
        let from = self.events.len().saturating_sub(20);
        json!({"turn_key": self.turn_key, "st": self.st.to_value(), "whole": self.whole.to_value(), "cum_turns": self.cum_turns, "segment": self.segment, "nudges_done": self.nudges_done, "aborted": self.aborted, "abort_delivered": self.abort_delivered, "events": self.events[from..], "judge_last": self.judge_last, "judge_gap": self.judge_gap, "judge_calls": self.judge_calls, "judge_tokens": self.judge_tokens, "writer_tokens": self.writer_tokens, "last_edit": self.last_edit, "last_verify": self.last_verify, "last_verify_failed": self.last_verify_failed, "edits": self.edits, "verifications": self.verifications, "judge_pending": self.judge_pending, "nudge_ready": self.nudge_ready})
    }

    fn track(&mut self, canon: &str, command: Option<&str>, result: &str, exit_code: Option<i64>, has_shell_edits: bool) {
        if matches!(canon, "edit" | "create") {
            self.last_edit = self.cum_turns;
            self.edits += 1;
            return;
        }
        let verifies = command.is_some_and(|text| hit(&VERIFY, text));
        if has_shell_edits {
            self.last_edit = self.cum_turns - i64::from(verifies);
            self.edits += 1;
        }
        if verifies {
            self.last_verify = self.cum_turns;
            self.verifications += 1;
            self.last_verify_failed = match exit_code {
                Some(code) => code != 0,
                None => hit(&ERR, result),
            };
        }
    }

    pub fn resolved_proxy(&self, stopped_normally: bool) -> bool {
        stopped_normally && !self.aborted && self.cum_turns != 0 && !(self.last_verify_failed && self.last_verify > self.last_edit)
    }

    pub fn verification_state(&self) -> String {
        if self.verifications == 0 {
            return "No test or verification run has been seen in this turn.".to_string();
        }
        let state = if self.last_verify_failed { "failed" } else { "passed" };
        let since = if self.last_edit > self.last_verify { " There have been edits since." } else { "" };

        format!("The last verification run {state}.{since}")
    }

    pub fn live_digest(&self, digest: &str) -> String {
        digest.replace(NOT_GREEN, &self.verification_state())
    }

    fn backoff(&mut self, verdict: Option<&Verdict>) {
        self.judge_gap = if verdict.is_some_and(|v| v.stuck == Some(false)) { JUDGE_MAX_GAP.min(self.judge_gap + JUDGE_BASE_GAP) } else { JUDGE_BASE_GAP };
    }

    // One completed tool call: None, or ("nudge" | "abort", reason). `judge` is a judge called
    // on the spot under the rate limit; the background one comes with the plugin's port.
    pub fn fold(&mut self, name: &str, args: &Value, result: &str, exit_code: Option<i64>, envelope: Option<f64>, thr: &Thresholds, judge: Option<&mut dyn FnMut(&str) -> Option<Verdict>>, nudge_budget: i64) -> Option<(String, String)> {
        let canon = canon_tool(name);
        let command = if canon == "bash" { args.get("command").and_then(Value::as_str) } else { None };
        let shell_edits = command.map_or_else(Vec::new, shell_edit_targets);
        let mut exit_code = exit_code;
        let status = status_of_tests(command);
        if matches!(status, Some(TestStatus::Hidden | TestStatus::Mixed)) {
            match runner_verdict(result) {
                Some(verdict) => exit_code = Some(i64::from(verdict)),
                None if status == Some(TestStatus::Hidden) => exit_code = None,
                None => {}
            }
        }
        let failed = exit_code.map(|code| code != 0);
        self.st.update(name, args, result, failed);
        self.whole.update(name, args, result, failed);
        self.st.fold_shell_edits(&shell_edits);
        self.whole.fold_shell_edits(&shell_edits);
        self.cum_turns += 1;
        self.track(&canon, command, result, exit_code, !shell_edits.is_empty());
        if self.aborted {
            return None;
        }

        let backstop = thr.get("backstop_turns").and_then(Value::as_f64).or(envelope.map(|value| value * number(thr, "backstop_mult")));
        if let Some(turns) = backstop {
            if self.cum_turns as f64 >= turns && is_stuck(&self.st, thr) {
                let reason = format!("cum-backstop:{}>={turns:.0}t", self.cum_turns);
                let action = if self.nudges_done < nudge_budget { "nudge" } else { "abort" };
                return Some(self.stop(action, reason, envelope));
            }
        }

        let state = self.st.clone();
        let (abort, reason) = match judge {
            Some(judge) => {
                let mut sampled = |digest: &str| -> Option<Verdict> {
                    if self.st.turns - self.judge_last < self.judge_gap {
                        return None;
                    }
                    self.judge_last = self.st.turns;
                    self.judge_calls += 1;
                    let verdict = judge(&self.live_digest(digest));
                    if let Some(found) = &verdict {
                        self.judge_tokens += found.tokens;
                    }
                    self.backoff(verdict.as_ref());
                    verdict
                };
                assess_abort(&state, envelope, Some(&mut sampled), thr)
            }
            None => assess_abort(&state, envelope, None, thr),
        };
        if !abort {
            if thr.get("nudge_on_ambiguous").and_then(Value::as_bool).unwrap_or(false) && reason == "judge-ambiguous-spared" && self.nudges_done < nudge_budget {
                return Some(self.stop("nudge", "ambiguous-past-envelope".to_string(), envelope));
            }
            return None;
        }
        let action = if self.nudges_done < nudge_budget { "nudge" } else { "abort" };

        Some(self.stop(action, reason, envelope))
    }

    fn stop(&mut self, action: &str, reason: String, envelope: Option<f64>) -> (String, String) {
        self.events.push(json!({"segment": self.segment, "action": action, "turn_in_segment": self.st.turns, "cum_turns": self.cum_turns, "envelope": envelope, "reason": reason, "snapshot": self.st.snapshot()}));
        self.aborted |= action == "abort";

        (action.to_string(), reason)
    }

    pub fn nudge_body(&mut self) -> String {
        match std::mem::replace(&mut self.nudge_ready, Value::Null) {
            Value::String(text) => text,
            _ => nudge_text(&self.st),
        }
    }

    pub fn after_nudge(&mut self) {
        self.nudges_done += 1;
        self.segment += 1;
        self.st = SpiralState::default();
        self.judge_last = -999;
        self.judge_gap = JUDGE_BASE_GAP;
        self.judge_pending = Value::Null;
        self.nudge_ready = Value::Null;
    }

    pub fn summary(&self, stopped_normally: bool) -> Value {
        json!({"turn_key": self.turn_key, "tool_calls": self.cum_turns, "nudges": self.nudges_done, "aborted": self.aborted, "resolved_proxy": self.resolved_proxy(stopped_normally), "edits": self.edits, "verifications": self.verifications, "judge_tokens": self.judge_tokens, "writer_tokens": self.writer_tokens})
    }
}

fn scripted_verdict(turns: i64) -> Option<Verdict> {
    let reason = format!("scripted {turns}");
    match turns % 4 {
        0 => Some(Verdict { stuck: Some(true), confidence: 0.9, reason, tokens: 11.0 }),
        1 => Some(Verdict { stuck: Some(false), confidence: 0.2, reason, tokens: 7.0 }),
        2 => Some(Verdict { stuck: None, confidence: 0.0, reason, tokens: 3.0 }),
        _ => None,
    }
}

// One JSON case per line on stdin, one result per line on stdout, for the comparison with Python.
pub fn replay(line: &str) -> Value {
    let case: Value = serde_json::from_str(line).unwrap_or(Value::Null);
    if let Some(command) = case.get("command").and_then(Value::as_str) {
        let status = status_of_tests(Some(command)).map(|found| match found {
            TestStatus::Own => "own",
            TestStatus::Hidden => "hidden",
            TestStatus::Mixed => "mixed",
        });
        return json!({"targets": shell_edit_targets(command), "status": status, "verifies": hit(&VERIFY, command)});
    }
    let empty = Map::new();
    let overrides = case.get("overrides").and_then(Value::as_object).unwrap_or(&empty);
    let level = case.get("level").and_then(Value::as_str).unwrap_or("default");
    let tracker = EnvelopeTracker::load(case.get("saved").unwrap_or(&Value::Null), level, overrides);
    let envelope = tracker.envelope();
    let mut thr = pinned_thresholds(&tracker, overrides);
    let learned = Value::Object(thr.clone());
    if case.get("plugin").and_then(Value::as_bool).unwrap_or(false) {
        thr.insert("gate_over_envelope".to_string(), json!(false));
        let gate = number(&thr, "gate_min_turns").max(number(&thr, "min_turns"));
        thr.insert("gate_min_turns".to_string(), json!(gate));
    }
    let use_judge = case.get("judge").and_then(Value::as_bool).unwrap_or(false);
    let budget = case.get("budget").and_then(Value::as_i64).unwrap_or(1);
    let mut guard = TurnGuard::new("t");
    let mut decisions = Vec::new();
    let mut digests = Vec::new();
    for call in case.get("calls").and_then(Value::as_array).into_iter().flatten() {
        // Saved and read back every call, as each one is a separate hook process.
        guard = TurnGuard::from_value(&guard.to_value());
        let name = call.get("name").and_then(Value::as_str).unwrap_or("");
        let args = call.get("args").cloned().unwrap_or(Value::Null);
        let result = call.get("result").and_then(Value::as_str).unwrap_or("");
        let code = call.get("code").and_then(Value::as_i64);
        let turns_next = guard.st.turns + 1;
        let mut judge = |digest: &str| -> Option<Verdict> {
            digests.push(digest.to_string());
            scripted_verdict(turns_next)
        };
        let wired: Option<&mut dyn FnMut(&str) -> Option<Verdict>> = if use_judge { Some(&mut judge) } else { None };
        let decision = guard.fold(name, &args, result, code, envelope, &thr, wired, budget);
        let mut body = Value::Null;
        if let Some((action, _)) = &decision {
            if action == "nudge" {
                body = json!(guard.nudge_body());
                guard.after_nudge();
            }
        }
        decisions.push(json!({"decision": decision, "body": body}));
    }
    let mut after = tracker;
    after.update(guard.cum_turns, guard.resolved_proxy(true), guard.whole.snapshot());

    json!({"decisions": decisions, "digests": digests, "guard": guard.to_value(), "thr": learned, "envelope": envelope, "summary": guard.summary(true), "saved_after": after.to_saved(), "digest": guard.st.recent_digest(24, envelope), "state": guard.verification_state()})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repeated_failure_counts_as_a_repeat_whatever_its_numbers() {
        let mut st = SpiralState::default();
        st.update("Bash", &json!({"command": "pytest"}), "E  ValueError at 0x1f line 12\n", None);
        st.update("Bash", &json!({"command": "pytest"}), "E  ValueError at 0xaa line 99\n", None);
        assert_eq!((st.error_outputs, st.repeated_fail_sigs, st.test_runs), (2, 1, 2));
        assert_eq!(st.stagnation(), 0.5);
    }

    #[test]
    fn a_file_read_back_is_not_a_failure() {
        let mut st = SpiralState::default();
        st.update("Read", &json!({"file_path": "a/t.py"}), "assert x\n", None);
        assert_eq!(st.error_outputs, 0);
        assert_eq!(st.recent, ["  1: view t.py"]);
    }

    #[test]
    fn edits_through_the_shell_are_seen() {
        let targets = |command: &str| shell_edit_targets(command);
        assert_eq!(targets("sed -i 's/a/b/' src/x.py && pytest"), ["x.py"]);
        assert_eq!(targets("echo hi > out.log"), Vec::<String>::new());
        assert_eq!(targets("cat <<'EOF' > a.rs\nfn main() {}\nEOF"), ["a.rs"]);
        assert_eq!(targets("git apply p.diff"), ["(patch)"]);
        assert_eq!(targets("python -c \"open('a.py','w').write('x')\""), ["a.py"]);
        assert_eq!(targets("echo 'unbalanced"), Vec::<String>::new());
    }

    #[test]
    fn a_hidden_exit_status_is_told_apart() {
        let status = |command: &str| status_of_tests(Some(command));
        assert!(status("pytest -q") == Some(TestStatus::Own));
        assert!(status("npm test 2>&1 | tail -n 8") == Some(TestStatus::Hidden));
        assert!(status("pytest -q; git diff") == Some(TestStatus::Mixed));
        assert!(status("set -o pipefail; pytest | tail") == Some(TestStatus::Own));
        assert!(status("ls -la") == None);
        assert!(status("pytest --help") == None);
    }

    #[test]
    fn the_guard_redirects_then_warns() {
        let thr = scaled_thresholds("default");
        let mut guard = TurnGuard::new("t");
        let mut seen = Vec::new();
        for n in 0..60 {
            let (name, args, result) = if n % 2 == 0 { ("Edit", json!({"file_path": "a.py"}), "") } else { ("Bash", json!({"command": "pytest"}), "E ValueError: same\n") };
            if let Some((action, _)) = guard.fold(name, &args, result, None, Some(10.0), &thr, None, 1) {
                if action == "nudge" {
                    assert!(guard.nudge_body().contains("MATERIALLY DIFFERENT"));
                    guard.after_nudge();
                }
                seen.push(action);
            }
        }
        assert_eq!(seen, ["nudge", "abort"]);
    }

    #[test]
    fn the_state_survives_a_round_trip_through_json() {
        let mut guard = TurnGuard::new("k");
        guard.fold("Bash", &json!({"command": "pytest"}), "E ValueError\n", Some(1), None, &scaled_thresholds("default"), None, 1);
        assert_eq!(TurnGuard::from_value(&guard.to_value()).to_value(), guard.to_value());
    }

    #[test]
    fn the_envelope_waits_for_samples_then_learns() {
        let saved = |turns: Vec<i64>| json!({"resolved_turns": turns});
        assert_eq!(EnvelopeTracker::load(&saved(vec![5, 6]), "default", &Map::new()).envelope(), None);
        let tracker = EnvelopeTracker::load(&saved(vec![10, 10, 10, 10, 10]), "default", &Map::new());
        assert_eq!(tracker.envelope(), Some(12.5));
        assert_eq!(tracker.thresholds()["gate_min_turns"], json!(6));
    }
}

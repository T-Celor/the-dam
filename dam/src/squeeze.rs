// Reads a command's output on stdin and writes a shorter one on stdout, without dropping
// anything: repeated lines become a template and a table of what varies, shared prefixes are
// written once. `--decode` gives the original back, and every result is decoded and compared
// before it is used, so a mistake here can only cost compression.
use crate::{folds, json};
use std::collections::HashMap;
use std::io::{self, Read, Write};

const WORTH_SQUEEZING_CHARS: usize = 6000;
const SHORTEST_RUN: usize = 4;
const SHORTEST_PREFIX: usize = 10;
const SHORTEST_PREFIX_GROUP: usize = 4;
// A value this far above the shape's usual ones is the line someone is looking for: it stays a plain line.
const OUTLIER_TIMES: f64 = 5.0;
const RARE_SHARE: f64 = 0.02;
const JSON_NOTE: &str = "JSON array as a table: the first line names the fields, then one row per record. An empty cell means the field is absent.";
const NOTE_START: &str = "Condensed without loss.";
const NOTE_RUNS: &str = " [x7] L: line L seven times. [run 9] T: nine lines of template T, each § filled from one row below, §5+2 counting up from 5 by 2.";
const NOTE_UNDER: &str = " [under P]: each following line starting with · begins with P.";
const NOTE_THEN: &str = " [then 5] L: line L again after each of the next 5 lines.";
const NOTE_BLOCKS: &str = " ... (repeats K lines from D lines back): those K lines again.";
const NOTE_SEARCH: &str = " A line naming a file heads the line:text rows of its search hits.";
const NOTE_DIRS: &str = " A line ending in / heads the file:line:text rows of its search hits.";
const NOTE_PATHS: &str = " A line ending in / heads the file names under that directory.";
const SLOT: char = '§';
const ROW_MARK: char = '·';

const ALARMS: [&str; 13] = [
    "error", "fail", "fatal", "panic", "exception", "traceback", "warning", "denied", "not found", "cannot", "assert", "timeout", "timed out",
];

fn is_alarm(line: &str) -> bool {
    let lower = line.to_lowercase();
    ALARMS.iter().any(|word| lower.contains(word))
}

// A number, a number with its unit (`12ms`) or a hash: not a name such as `user233`, which stays itself.
fn is_value(word: &str) -> bool {
    let digits = word.trim_end_matches(|c: char| !c.is_ascii_digit());
    let is_hash = word.len() >= 7 && word.chars().all(|c| c.is_ascii_hexdigit());

    !digits.is_empty() && (digits.chars().all(|c| c.is_ascii_digit()) || is_hash)
}

// Lines that differ only by values have one shape. The values come back, as written.
fn shape_of(line: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(line.len());
    let mut values = Vec::new();
    let mut word = String::new();
    let mut flush = |word: &mut String, out: &mut String| {
        if is_value(word) {
            out.push(SLOT);
            values.push(std::mem::take(word));
        } else {
            out.push_str(word);
            word.clear();
        }
    };
    for c in line.chars() {
        if c.is_alphanumeric() {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);

    (out, values)
}

fn number_in(value: &str) -> f64 {
    value.trim_end_matches(|c: char| !c.is_ascii_digit()).parse().unwrap_or(f64::NAN)
}

// Lines of one run whose value stands out: a rare value beside a dominant one (a 503 among 200s),
// or one far above the rest (48211ms among 15ms). They stay plain so that nobody has to spot them in a table.
fn stand_outs(rows: &[Vec<String>]) -> Vec<bool> {
    let mut found = vec![false; rows.len()];
    for slot in 0..rows[0].len() {
        let numbers: Vec<f64> = rows.iter().map(|row| number_in(&row[slot])).collect();
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for row in rows {
            *counts.entry(row[slot].as_str()).or_insert(0) += 1;
        }
        if counts.values().copied().max().unwrap_or(0) * 2 >= rows.len() {
            let rare = ((rows.len() as f64 * RARE_SHARE) as usize).max(1);
            for (n, row) in rows.iter().enumerate() {
                found[n] |= counts[row[slot].as_str()] <= rare;
            }
        }
        let mut sorted: Vec<f64> = numbers.iter().copied().filter(|v| v.is_finite()).collect();
        if sorted.len() == rows.len() {
            sorted.sort_by(f64::total_cmp);
            let usual = sorted[sorted.len() * 9 / 10];
            for (n, value) in numbers.iter().enumerate() {
                found[n] |= usual > 0.0 && *value > usual * OUTLIER_TIMES;
            }
        }
    }

    found
}

// A column as `start+step`, when every value is exactly that count written plainly.
fn counting(column: &[&str]) -> Option<(i64, i64)> {
    let parsed: Vec<i64> = column.iter().map(|value| value.parse().ok()).collect::<Option<_>>()?;
    let step = parsed.get(1)? - parsed[0];
    let is_plain = column.iter().zip(&parsed).all(|(text, value)| *text == value.to_string());

    (is_plain && step != 0 && parsed.windows(2).all(|pair| pair[1] - pair[0] == step)).then_some((parsed[0], step))
}

// One run of a shape as text: identical lines, or the template and only the columns that vary.
fn encode_run(shape: &str, rows: &[Vec<String>], out: &mut Vec<String>) {
    let columns = rows[0].len();
    let mut template = String::new();
    let mut varying = Vec::new();
    let mut pieces = shape.split(SLOT);
    template.push_str(pieces.next().unwrap_or(""));
    for (slot, piece) in pieces.enumerate() {
        let column: Vec<&str> = rows.iter().map(|row| row[slot].as_str()).collect();
        if column.iter().all(|value| *value == column[0]) {
            template.push_str(column[0]);
        } else if let Some((start, step)) = counting(&column) {
            template.push_str(&format!("{SLOT}{start}{step:+}"));
        } else {
            template.push(SLOT);
            varying.push(slot);
        }
        template.push_str(piece);
    }
    let _ = columns;
    if !template.contains(SLOT) {
        out.push(format!("[x{}] {template}", rows.len()));
        return;
    }
    out.push(format!("[run {}] {template}", rows.len()));
    if !varying.is_empty() {
        for row in rows {
            let cells: Vec<&str> = varying.iter().map(|&slot| row[slot].as_str()).collect();
            out.push(cells.join(" "));
        }
    }
}

fn decode_run(count: usize, template: &str, rows: &mut std::slice::Iter<'_, &str>) -> Option<Vec<String>> {
    let pieces: Vec<&str> = template.split(SLOT).collect();
    let needs_row = pieces.iter().skip(1).any(|piece| !starts_with_count(piece));
    let mut out = Vec::with_capacity(count);
    for n in 0..count {
        let row = if needs_row { rows.next()?.split(' ').collect() } else { Vec::new() };
        let mut cells = row.iter();
        let mut line = pieces[0].to_string();
        for piece in &pieces[1..] {
            if let Some((start, step, rest)) = count_at(piece) {
                line.push_str(&(start + step * n as i64).to_string());
                line.push_str(rest);
            } else {
                line.push_str(cells.next()?);
                line.push_str(piece);
            }
        }
        out.push(line);
    }

    Some(out)
}

fn starts_with_count(piece: &str) -> bool {
    count_at(piece).is_some()
}

// `5+2 rest` after a slot: a counting column.
fn count_at(piece: &str) -> Option<(i64, i64, &str)> {
    let number_end = |text: &str, from: usize| text[from..].find(|c: char| !c.is_ascii_digit()).map_or(text.len(), |n| from + n);
    let first = usize::from(piece.starts_with('-'));
    let start_end = number_end(piece, first);
    if start_end == first {
        return None;
    }
    let sign = piece[start_end..].chars().next().filter(|c| matches!(c, '+' | '-'))?;
    let step_end = number_end(piece, start_end + 1);
    if step_end == start_end + 1 {
        return None;
    }
    let step: i64 = piece[start_end + 1..step_end].parse().ok()?;

    Some((piece[..start_end].parse().ok()?, if sign == '-' { -step } else { step }, &piece[step_end..]))
}

fn common_prefix<'a>(a: &'a str, b: &str) -> &'a str {
    let end = a.char_indices().zip(b.chars()).take_while(|((_, x), y)| x == y).last().map_or(0, |((at, x), _)| at + x.len_utf8());

    &a[..end]
}

// Lines sharing a long start (a path, a search hit's file), written once. Indented lines are
// left out: code must keep its shape to be read.
fn fold_prefixes(lines: &[String]) -> Vec<String> {
    let is_candidate = |line: &str| !line.starts_with([' ', '\t', '[']);
    let mut out = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let mut prefix = lines[i].as_str();
        let mut end = i + 1;
        if is_candidate(prefix) {
            while end < lines.len() && is_candidate(&lines[end]) {
                let shared = common_prefix(prefix, &lines[end]);
                if shared.chars().count() < SHORTEST_PREFIX {
                    break;
                }
                prefix = shared;
                end += 1;
            }
        }
        let count = end - i;
        // A trailing space would be invisible in the header, so the prefix stops before it.
        let prefix = prefix.trim_end_matches(' ');
        let header = format!("[under {prefix}]");
        let saved = count as i64 * (prefix.len() as i64 - ROW_MARK.len_utf8() as i64) - header.len() as i64;
        if count >= SHORTEST_PREFIX_GROUP && prefix.chars().count() >= SHORTEST_PREFIX && saved > 0 {
            out.push(header);
            for line in &lines[i..end] {
                out.push(format!("{ROW_MARK}{}", &line[prefix.len()..]));
            }
            i = end;
        } else {
            out.push(lines[i].clone());
            i += 1;
        }
    }

    out
}

// What the output means once trailing spaces and line ends are set aside: the comparison's baseline.
fn normal(text: &str) -> Vec<String> {
    text.lines().map(|line| folds::strip_ansi(line).trim_end().to_string()).collect()
}

fn encode(lines: &[String]) -> Vec<String> {
    let parts: Vec<(String, Vec<String>)> = lines.iter().map(|line| shape_of(line)).collect();
    let mut out = Vec::with_capacity(lines.len());
    let mut plain: Vec<String> = Vec::new();
    let flush = |plain: &mut Vec<String>, out: &mut Vec<String>| {
        out.extend(fold_prefixes(plain));
        plain.clear();
    };
    let mut start = 0;
    while start < lines.len() {
        let mut end = start + 1;
        while end < lines.len() && parts[end].0 == parts[start].0 {
            end += 1;
        }
        let rows: Vec<Vec<String>> = parts[start..end].iter().map(|part| part.1.clone()).collect();
        if end - start >= SHORTEST_RUN && !lines[start].trim().is_empty() {
            // Alarms and stand-outs split the run and stay plain lines.
            let plain_line: Vec<bool> = stand_outs(&rows).iter().zip(&lines[start..end]).map(|(found, line)| *found || is_alarm(line)).collect();
            let mut from = 0;
            while from < rows.len() {
                let mut to = from;
                while to < rows.len() && !plain_line[to] {
                    to += 1;
                }
                let mut encoded = Vec::new();
                if to - from >= SHORTEST_RUN {
                    encode_run(&parts[start].0, &rows[from..to], &mut encoded);
                }
                let raw_len: usize = lines[start + from..start + to].iter().map(|line| line.len() + 1).sum();
                let encoded_len: usize = encoded.iter().map(|line| line.len() + 1).sum();
                if !encoded.is_empty() && encoded_len < raw_len {
                    flush(&mut plain, &mut out);
                    out.append(&mut encoded);
                } else {
                    plain.extend_from_slice(&lines[start + from..start + to]);
                }
                if to < rows.len() {
                    plain.push(lines[start + to].clone());
                }
                from = to + 1;
            }
        } else {
            plain.extend_from_slice(&lines[start..end]);
        }
        start = end;
    }
    flush(&mut plain, &mut out);

    out
}

fn decode_marks(lines: Vec<&str>) -> Option<Vec<String>> {
    let mut rest = lines.iter();
    let mut out = Vec::with_capacity(lines.len());
    let mut under: Option<&str> = None;
    while let Some(&line) = rest.next() {
        if let Some(row) = line.strip_prefix(ROW_MARK).filter(|_| under.is_some()) {
            out.push(format!("{}{row}", under?));
            continue;
        }
        under = None;
        if let Some(prefix) = line.strip_prefix("[under ").and_then(|tail| tail.strip_suffix(']')) {
            under = Some(prefix);
        } else if let Some((count, template)) = line.strip_prefix("[x").and_then(|tail| tail.split_once("] ")) {
            let count: usize = count.parse().ok()?;
            out.extend(std::iter::repeat_n(template.to_string(), count));
        } else if let Some((count, template)) = line.strip_prefix("[run ").and_then(|tail| tail.split_once("] ")) {
            let count: usize = count.parse().ok()?;
            out.extend(decode_run(count, template, &mut rest)?);
        } else {
            out.push(line.to_string());
        }
    }

    Some(out)
}

// What every note says it used decides how the text is read back.
fn decode(text: &str) -> Option<Vec<String>> {
    let (note, body) = text.split_once('\n')?;
    if !note.starts_with(NOTE_START) {
        return Some(text.lines().map(str::to_string).collect());
    }
    let unfolded = folds::unfold_blocks(&body.lines().map(str::to_string).collect::<Vec<_>>())?;
    let marks = decode_marks(unfolded.iter().map(String::as_str).collect())?;
    let lines = folds::unfold_alternating(&marks)?;

    Some(if note.contains(NOTE_SEARCH) {
        folds::search_unheading(&lines)
    } else if note.contains(NOTE_DIRS) {
        folds::dir_unheading(&lines)
    } else if note.contains(NOTE_PATHS) {
        folds::path_unheading(&lines)
    } else {
        lines
    })
}

fn note_for(heading: &str, body: &[String]) -> String {
    let mut note = format!("{NOTE_START}{heading}");
    if body.iter().any(|line| line.starts_with("[x") || line.starts_with("[run ")) {
        note.push_str(NOTE_RUNS);
    }
    if body.iter().any(|line| line.starts_with("[then ")) {
        note.push_str(NOTE_THEN);
    }
    if body.iter().any(|line| line.starts_with("[under ")) {
        note.push_str(NOTE_UNDER);
    }
    if body.iter().any(|line| line.starts_with("... (repeats ")) {
        note.push_str(NOTE_BLOCKS);
    }

    note
}

fn squeeze(text: &str) -> String {
    if text.chars().count() < WORTH_SQUEEZING_CHARS {
        return text.to_string();
    }
    // Whole JSON: as a table when it is an array of records and reads back identically, else as it came.
    if json::is_valid(text) {
        return json::table(text).map_or_else(|| text.to_string(), |table| format!("{JSON_NOTE}\n{table}\n"));
    }
    let lines = normal(text);
    // An output that already holds these marks could not be told apart from them.
    let holds_marks = lines.iter().any(|line| line.contains(SLOT) || line.starts_with("[x") || line.starts_with("[run ") || line.starts_with("[under ") || line.starts_with("[then ") || line.starts_with(ROW_MARK) || line.starts_with("... (repeats ") || line.starts_with(NOTE_START));
    if holds_marks {
        return text.to_string();
    }
    // Every shape of heading is tried and the shortest that reads back identical wins.
    let shapes: [(&str, Vec<String>); 4] = [("", lines.clone()), (NOTE_SEARCH, folds::search_heading(&lines)), (NOTE_DIRS, folds::dir_heading(&lines)), (NOTE_PATHS, folds::path_heading(&lines))];
    let mut best: Option<String> = None;
    for (heading, shaped) in &shapes {
        let body = folds::fold_blocks(&encode(&folds::fold_alternating(shaped)));
        let result = format!("{}\n{}\n", note_for(heading, &body), body.join("\n"));
        let is_better = best.as_ref().is_none_or(|kept| result.len() < kept.len());
        if is_better && decode(&result).as_deref() == Some(lines.as_slice()) {
            best = Some(result);
        }
    }

    best.filter(|result| result.len() < text.len()).unwrap_or_else(|| text.to_string())
}

// Keeps the `keep` newest files of `dir`: the originals every session saves there.
pub fn prune(dir: &std::path::Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries.flatten().filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path()))).collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in files.into_iter().skip(keep) {
        let _ = std::fs::remove_file(path);
    }
}

pub fn run(arg: Option<&str>) -> io::Result<()> {
    let mut bytes = Vec::new();
    io::stdin().read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let out = if arg == Some("--decode") {
        decode(&text).map_or_else(|| text.to_string(), |lines| lines.join("\n") + "\n")
    } else {
        squeeze(&text)
    };

    io::stdout().write_all(out.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(n: usize) -> String {
        (0..n).map(|i| format!("INFO worker handled request {i} status 200 in {}ms\n", 10 + i % 9)).collect()
    }

    // Nothing may be lost, whatever the input.
    fn round_trips(text: &str) -> String {
        let out = squeeze(text);
        if out != text {
            assert_eq!(decode(&out).unwrap(), normal(text));
        }
        out
    }

    #[test]
    fn short_output_passes_untouched() {
        assert_eq!(squeeze("a\nb\n"), "a\nb\n");
    }

    #[test]
    fn a_log_shrinks_with_every_value_recoverable() {
        let text = format!("{}ERROR database refused the connection\n{}", log(2000), log(2000));
        let out = round_trips(&text);
        assert!(out.contains("\nERROR database refused the connection\n"));
        assert!(out.len() * 3 < text.len(), "{} of {}", out.len(), text.len());
    }

    #[test]
    fn a_rare_status_and_a_slow_request_stay_plain_lines() {
        let mut lines: Vec<String> = log(1000).lines().map(String::from).collect();
        lines[300] = "INFO worker handled request 300 status 200 in 48211ms".into();
        lines[420] = "INFO worker handled request 420 status 503 in 13ms".into();
        let out = round_trips(&(lines.join("\n") + "\n"));
        assert!(out.contains(&format!("\n{}\n", lines[300])));
        assert!(out.contains(&format!("\n{}\n", lines[420])));
    }

    #[test]
    fn counting_lines_need_no_rows() {
        let text: String = (0..=1000).map(|i| format!("Downloading {i}%\n")).collect();
        let out = round_trips(&text);
        assert!(out.ends_with("[run 1001] Downloading §0+1%\n"), "{out}");
        assert!(!out.contains(NOTE_BLOCKS));
    }

    #[test]
    fn identical_lines_are_counted() {
        let out = round_trips(&"still waiting for the server\n".repeat(500));
        assert!(out.ends_with("[x500] still waiting for the server\n"), "{out}");
    }

    #[test]
    fn search_hits_group_under_their_file() {
        let text: String = (0..400).map(|i| format!("src/app/module.py:{i}: value = word{}x()\n", ["a", "b", "c", "d", "e", "f", "g"][i % 7])).collect();
        let out = round_trips(&text);
        assert!(out.len() * 10 < text.len() * 8, "{} of {}", out.len(), text.len());
    }

    #[test]
    fn paths_fold_under_their_directory() {
        let words = ["alpha", "bravo", "carol", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet"];
        let text: String = (0..800).map(|i| format!("assets/project/textures/{}_{}.png\n", words[i % 10], words[i / 10 % 10])).collect();
        let out = round_trips(&text);
        assert!(out.contains("assets/project/textures/"));
        assert!(out.len() * 2 < text.len());
    }

    #[test]
    fn source_code_keeps_its_shape() {
        let names = ["read", "write", "parse", "print", "merge", "split", "sort", "load", "save", "find"];
        let text: String = (0..900).map(|i| format!("fn {}_{}() {{\n    {}_{}();\n}}\n\n", names[i % 10], names[i / 10 % 10], names[(i + 3) % 10], names[i / 100 % 10])).collect();
        round_trips(&text);
        assert!(squeeze(&text).contains("    "));
    }

    #[test]
    fn json_lines_and_marks_in_the_input_are_safe() {
        round_trips(&(0..800).map(|i| format!("{{\"id\": {i}, \"ok\": true}}\n")).collect::<String>());
        let marked = format!("[x3] not ours\n{}", log(500));
        assert_eq!(squeeze(&marked), marked);
        let slot = format!("a § b\n{}", log(500));
        assert_eq!(squeeze(&slot), slot);
    }

    #[test]
    fn every_json_record_stays_as_a_table_row() {
        let text = format!("[{}]", (0..1000).map(|i| format!("{{\"id\": {i}, \"plan\": \"pro\"}}")).collect::<Vec<_>>().join(",\n"));
        let out = squeeze(&text);
        assert!(out.len() < text.len() / 2);
        for i in 0..1000 {
            assert!(out.contains(&format!("\n{i},pro\n")), "{i}");
        }
    }

    #[test]
    fn json_that_is_no_table_is_left_alone() {
        let text = format!("{{\"values\": [{}]}}", (0..3000).map(|i| i.to_string()).collect::<Vec<_>>().join(",\n"));
        assert_eq!(squeeze(&text), text);
    }

    #[test]
    fn odd_values_round_trip() {
        let zeros: String = (0..500).map(|i| format!("at 12:{:02}:{:02} id {:x}abc1 count -{i}\n", i / 60, i % 60, i * 7919)).collect();
        round_trips(&zeros);
        let unicode: String = (0..800).map(|i| format!("é {i} ünï {} 日本\n", i * 2)).collect();
        round_trips(&unicode);
        let countdown: String = (0..800).map(|i| format!("left {}\n", 800 - i)).collect();
        round_trips(&countdown);
        let plus: String = (0..800).map(|i| format!("v{i}+1 and {i}+2\n")).collect();
        round_trips(&plus);
    }

    #[test]
    fn empty_input_does_not_panic() {
        assert_eq!(squeeze(""), "");
        round_trips(&"\n".repeat(7000));
        round_trips(&"é".repeat(9000));
    }
}

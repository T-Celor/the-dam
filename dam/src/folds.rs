// Reversible folds taken from the rules of Headroom's lossless_compaction.py (Apache-2.0,
// https://github.com/headroomlabs-ai/headroom), rewritten here. Each has an exact inverse, which the
// caller checks before using the result.
use std::collections::HashMap;

const MOST_BLOCK: usize = 64;
const SHORTEST_BLOCK: usize = 3;
const CANDIDATES_PER_LINE: usize = 8;
const SHORTEST_ALTERNATION: usize = 4;

pub fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find("\x1b[") {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 2..];
        let end = tail.find(|c: char| !c.is_ascii_digit() && c != ';');
        match end {
            Some(n) if tail[n..].starts_with('m') => rest = &tail[n + 1..],
            _ => {
                out.push_str("\x1b[");
                rest = tail;
            }
        }
    }
    out.push_str(rest);

    out
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

// A clock has the shape of `path:line:content` too, and folding it would split the time.
fn looks_like_time(path: &str) -> bool {
    let path = path.trim_start().trim_start_matches('[');
    path.contains(char::is_whitespace) || (path.starts_with(|c: char| c.is_ascii_digit()) && path.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | '/' | 'T')))
}

fn grep_row(line: &str) -> Option<(&str, &str, &str)> {
    let (path, rest) = line.split_once(':')?;
    let (number, content) = rest.split_once(':')?;

    (!path.is_empty() && is_digits(number) && !looks_like_time(path)).then_some((path, number, content))
}

// `path:12:text` rows under one line naming the file.
pub fn search_heading(lines: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut current: Option<&str> = None;
    for line in lines {
        if let Some((path, number, content)) = grep_row(line) {
            if current != Some(path) {
                out.push(path.to_string());
                current = Some(path);
            }
            out.push(format!("{number}:{content}"));
        } else {
            out.push(line.clone());
            current = None;
        }
    }

    out
}

fn heading_row(line: &str) -> bool {
    line.split_once(':').is_some_and(|(number, _)| is_digits(number))
}

pub fn search_unheading(lines: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut current: Option<&str> = None;
    for (i, line) in lines.iter().enumerate() {
        let is_data = heading_row(line);
        if let (Some(path), true) = (current, is_data) {
            out.push(format!("{path}:{line}"));
        } else if !is_data && lines.get(i + 1).is_some_and(|next| heading_row(next)) {
            current = Some(line);
        } else {
            current = None;
            out.push(line.clone());
        }
    }

    out
}

// The same rows under one line naming their directory: the `grep -rn` case of one hit per file.
pub fn dir_heading(lines: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut current: Option<&str> = None;
    for line in lines {
        match grep_row(line).and_then(|(path, ..)| path.rfind('/').map(|at| line.split_at(at + 1))) {
            Some((dir, rest)) => {
                if current != Some(dir) {
                    out.push(dir.to_string());
                    current = Some(dir);
                }
                out.push(rest.to_string());
            }
            None => {
                out.push(line.clone());
                current = None;
            }
        }
    }

    out
}

fn dir_row(line: &str) -> bool {
    let mut cells = line.splitn(3, ':');
    let (Some(base), Some(number), Some(_)) = (cells.next(), cells.next(), cells.next()) else { return false };

    !base.is_empty() && !base.contains('/') && is_digits(number)
}

pub fn dir_unheading(lines: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut current: Option<&str> = None;
    for (i, line) in lines.iter().enumerate() {
        let is_data = dir_row(line);
        if let (Some(dir), true) = (current, is_data) {
            out.push(format!("{dir}{line}"));
        } else if line.ends_with('/') && lines.get(i + 1).is_some_and(|next| dir_row(next)) {
            current = Some(line);
        } else {
            current = None;
            out.push(line.clone());
        }
    }

    out
}

fn path_row(line: &str) -> Option<usize> {
    let at = line.rfind('/')?;
    let is_clean = !line.contains(char::is_whitespace) && !line.contains(':');

    (is_clean && at + 1 < line.len() && at > 0).then_some(at + 1)
}

// A plain listing of paths: each directory once, then the bare names under it.
pub fn path_heading(lines: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut current: Option<&str> = None;
    for line in lines {
        if let Some(at) = path_row(line) {
            let (dir, base) = line.split_at(at);
            if current != Some(dir) {
                out.push(dir.to_string());
                current = Some(dir);
            }
            out.push(base.to_string());
        } else {
            out.push(line.clone());
            current = None;
        }
    }

    out
}

pub fn path_unheading(lines: &[String]) -> Vec<String> {
    let is_base = |line: &str| !line.is_empty() && !line.contains('/');
    let mut out = Vec::with_capacity(lines.len());
    let mut current: Option<&str> = None;
    for (i, line) in lines.iter().enumerate() {
        if let (Some(dir), true) = (current, is_base(line)) {
            out.push(format!("{dir}{line}"));
        } else if line.ends_with('/') && lines.get(i + 1).is_some_and(|next| is_base(next)) {
            current = Some(line);
        } else {
            current = None;
            out.push(line.clone());
        }
    }

    out
}

// The same line after every other line, as `git add` writes its warnings: said once, with a count.
pub fn fold_alternating(lines: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let same = lines.get(i + 1);
        let pairs = same.map_or(0, |same| (0..).take_while(|n| lines.get(i + 1 + n * 2) == Some(same) && i + n * 2 < lines.len()).count());
        if pairs >= SHORTEST_ALTERNATION && lines.get(i) != same {
            out.push(format!("[then {pairs}] {}", same.map_or("", String::as_str)));
            out.extend((0..pairs).map(|n| lines[i + n * 2].clone()));
            i += pairs * 2;
        } else {
            out.push(lines[i].clone());
            i += 1;
        }
    }

    out
}

pub fn unfold_alternating(lines: &[String]) -> Option<Vec<String>> {
    let mut out = Vec::with_capacity(lines.len());
    let mut rest = lines.iter();
    while let Some(line) = rest.next() {
        if let Some((count, same)) = line.strip_prefix("[then ").and_then(|tail| tail.split_once("] ")) {
            for _ in 0..count.parse::<usize>().ok()? {
                out.push(rest.next()?.clone());
                out.push(same.to_string());
            }
        } else {
            out.push(line.clone());
        }
    }

    Some(out)
}

// A block of 3 lines or more already seen earlier becomes a reference to it.
pub fn fold_blocks(lines: &[String]) -> Vec<String> {
    let mut seen: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut out = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let (mut best, mut distance) = (0, 0);
        for &earlier in seen.get(lines[i].as_str()).into_iter().flatten().rev() {
            let most = MOST_BLOCK.min(lines.len() - i).min(i - earlier);
            let length = (0..most).take_while(|&k| lines[earlier + k] == lines[i + k]).count();
            if length > best {
                (best, distance) = (length, i - earlier);
            }
        }
        let marker = format!("... (repeats {best} lines from {distance} lines back)");
        let block: usize = lines[i..i + best].iter().map(|line| line.len() + 1).sum();
        let step = if best >= SHORTEST_BLOCK && block > marker.len() + 1 {
            out.push(marker);
            best
        } else {
            out.push(lines[i].clone());
            1
        };
        for (k, line) in lines[i..i + step].iter().enumerate() {
            let places = seen.entry(line).or_default();
            places.push(i + k);
            if places.len() > CANDIDATES_PER_LINE {
                places.remove(0);
            }
        }
        i += step;
    }

    out
}

pub fn unfold_blocks(lines: &[String]) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    for line in lines {
        if let Some(rest) = line.strip_prefix("... (repeats ").and_then(|tail| tail.strip_suffix(" lines back)")) {
            let (length, distance) = rest.split_once(" lines from ")?;
            let (length, distance): (usize, usize) = (length.parse().ok()?, distance.parse().ok()?);
            let start = out.len().checked_sub(distance)?;
            let block = out.get(start..start + length)?.to_vec();
            out.extend(block);
        } else {
            out.push(line.clone());
        }
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| line.to_string()).collect()
    }

    #[test]
    fn colour_goes_and_the_text_stays() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m plain \x1b[2J"), "red plain \x1b[2J");
    }

    #[test]
    fn search_rows_group_and_come_back() {
        let lines = own(&["src/a.py:1:x", "src/a.py:9:y", "src/b.py:2:z", "note"]);
        let folded = search_heading(&lines);
        assert_eq!(folded, own(&["src/a.py", "1:x", "9:y", "src/b.py", "2:z", "note"]));
        assert_eq!(search_unheading(&folded), lines);
        let by_dir = dir_heading(&lines);
        assert_eq!(by_dir, own(&["src/", "a.py:1:x", "a.py:9:y", "b.py:2:z", "note"]));
        assert_eq!(dir_unheading(&by_dir), lines);
    }

    #[test]
    fn a_clock_is_not_a_search_row() {
        for line in ["2026-09-02 14:30:00 [FATAL] x", "15:03:53 up", "Aug 16 11:02:01 host", "2026-09-02T14:30:00"] {
            assert_eq!(grep_row(line), None, "{line}");
        }
    }

    #[test]
    fn paths_group_and_come_back() {
        let lines = own(&["a/b/c.txt", "a/b/d.txt", "e/f.txt", "", "a/b/g.txt"]);
        let folded = path_heading(&lines);
        assert_eq!(folded, own(&["a/b/", "c.txt", "d.txt", "e/", "f.txt", "", "a/b/", "g.txt"]));
        assert_eq!(path_unheading(&folded), lines);
    }

    #[test]
    fn an_alternating_line_is_said_once() {
        let mut lines = own(&["head"]);
        for n in 0..6 {
            lines.push(format!("warning: LF in file{n}"));
            lines.push("The file will keep its line endings".to_string());
        }
        lines.push("tail".to_string());
        let folded = fold_alternating(&lines);
        assert!(folded.iter().any(|line| line == "[then 6] The file will keep its line endings"));
        assert!(folded.len() < 10);
        assert_eq!(unfold_alternating(&folded).unwrap(), lines);
    }

    #[test]
    fn repeated_blocks_fold_and_come_back() {
        let block = ["image: registry.example/app:latest", "imagePullPolicy: IfNotPresent", "restartPolicy: Always"];
        let mut lines = own(&["name: a"]);
        lines.extend(own(&block));
        lines.extend(own(&["name: b"]));
        lines.extend(own(&block));
        let folded = fold_blocks(&lines);
        assert_eq!(folded.last().unwrap(), "... (repeats 3 lines from 4 lines back)");
        assert_eq!(unfold_blocks(&folded).unwrap(), lines);
    }
}

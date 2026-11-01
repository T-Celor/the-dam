// Reads a command's output on stdin and writes a shorter one on stdout. Lossy by design:
// the caller keeps the full output and tells the model where it is.
use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};

const WORTH_SQUEEZING_CHARS: usize = 6000;
const HEAD_LINES: usize = 15;
const TAIL_LINES: usize = 15;
// About 6000 tokens: a listing or a source file stays readable, and the caller holds the rest.
const MOST_KEPT_CHARS: usize = 24_000;
const REPETITIVE_BELOW_DISTINCT: f64 = 0.5;
const MOST_LINE_CHARS: usize = 400;
const REPEATS_OF_AN_ALARM: usize = 3;
const OMISSION_MARK_CHARS: usize = 22;

const ALARMS: [&str; 13] = [
    "error", "fail", "fatal", "panic", "exception", "traceback", "warning", "denied", "not found", "cannot", "assert", "timeout", "timed out",
];

fn is_alarm(line: &str) -> bool {
    let lower = line.to_lowercase();
    ALARMS.iter().any(|word| lower.contains(word))
}

// Lines that differ only by numbers, ids or hashes count as the same line.
fn template(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if word.chars().any(|c| c.is_ascii_digit()) {
            out.push('#');
        } else {
            out.push_str(word);
        }
        word.clear();
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

    out
}

fn shorten(line: &str) -> String {
    let count = line.chars().count();
    if count <= MOST_LINE_CHARS {
        return line.to_string();
    }
    let head: String = line.chars().take(MOST_LINE_CHARS * 3 / 4).collect();
    let tail: String = line.chars().skip(count - MOST_LINE_CHARS / 4).collect();

    format!("{head} [{} chars omitted] {tail}", count - MOST_LINE_CHARS)
}

fn squeeze(text: &str) -> String {
    if text.chars().count() < WORTH_SQUEEZING_CHARS {
        return text.to_string();
    }
    let lines: Vec<&str> = text.lines().collect();
    let templates: Vec<String> = lines.iter().map(|line| template(line)).collect();
    let mut keep = vec![false; lines.len()];
    for (i, kept) in keep.iter_mut().enumerate() {
        *kept = i < HEAD_LINES || i + TAIL_LINES >= lines.len();
    }

    let mut alarms: HashMap<&str, usize> = HashMap::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut first_of_shape = Vec::new();
    let mut ordinary = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let shape = templates[i].as_str();
        let is_first = seen.insert(shape);
        if is_alarm(line) {
            let repeats = alarms.entry(shape).or_insert(0);
            *repeats += 1;
            if *repeats <= REPEATS_OF_AN_ALARM {
                keep[i] = true;
            }
        } else if !keep[i] && !line.trim().is_empty() {
            ordinary.push(i);
            if is_first {
                first_of_shape.push(i);
            }
        }
    }

    let chars_of = |i: usize| lines[i].chars().count().min(MOST_LINE_CHARS) + 1;
    if (seen.len() as f64) < REPETITIVE_BELOW_DISTINCT * lines.len() as f64 {
        // A log: one line of each kind, evenly spaced if even those do not fit.
        let total: usize = first_of_shape.iter().map(|&i| chars_of(i) + OMISSION_MARK_CHARS).sum();
        let step = total / MOST_KEPT_CHARS + 1;
        for &i in first_of_shape.iter().step_by(step) {
            keep[i] = true;
        }
    } else {
        // Content, where every line counts and even a lone brace matters: an unbroken start and end.
        let mut room = MOST_KEPT_CHARS * 2 / 3;
        for &i in &ordinary {
            if chars_of(i) > room {
                break;
            }
            room -= chars_of(i);
            keep[i] = true;
        }
        room += MOST_KEPT_CHARS / 3;
        for &i in ordinary.iter().rev() {
            if keep[i] {
                break;
            }
            if chars_of(i) > room {
                break;
            }
            room -= chars_of(i);
            keep[i] = true;
        }
        // Blank lines inside what is kept are not worth a marker of their own.
        for i in 1..lines.len().saturating_sub(1) {
            if lines[i].trim().is_empty() && keep[i - 1] && keep[i + 1] {
                keep[i] = true;
            }
        }
    }

    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        if keep[i] {
            out.push_str(&shorten(lines[i]));
            out.push('\n');
            i += 1;
            continue;
        }
        let start = i;
        while i < lines.len() && !keep[i] {
            i += 1;
        }
        out.push_str(&format!("[{} lines omitted]\n", i - start));
    }

    out
}

fn main() -> io::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("dam-squeeze {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let mut bytes = Vec::new();
    io::stdin().read_to_end(&mut bytes)?;
    io::stdout().write_all(squeeze(&String::from_utf8_lossy(&bytes)).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(n: usize) -> String {
        (0..n).map(|i| format!("INFO worker handled request {i} in {}ms\n", i % 50)).collect()
    }

    #[test]
    fn short_output_passes_untouched() {
        assert_eq!(squeeze("a\nb\n"), "a\nb\n");
    }

    #[test]
    fn repetition_collapses_and_the_buried_error_survives() {
        let text = format!("{}ERROR database refused the connection\n{}", log(2000), log(2000));
        let out = squeeze(&text);
        assert!(out.contains("ERROR database refused the connection"));
        assert!(out.contains("lines omitted"));
        assert!(out.len() * 10 < text.len());
    }

    #[test]
    fn first_and_last_lines_stay() {
        let out = squeeze(&format!("command started\n{}exit code 3\n", log(1000)));
        assert!(out.starts_with("command started\n"));
        assert!(out.ends_with("exit code 3\n"));
    }

    fn prose(n: usize) -> String {
        let words = ["alpha", "bravo", "carol", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet"];
        (0..n).map(|i| format!("{} {} {}\n", words[i % 10], words[i / 10 % 10], words[i / 100 % 10])).collect()
    }

    #[test]
    fn distinct_content_that_fits_is_left_whole() {
        let text = prose(900);
        assert_eq!(squeeze(&text), text);
    }

    #[test]
    fn distinct_content_is_bounded_keeping_an_unbroken_start_and_end() {
        let text = format!("{}{}{}", "x".repeat(10), "\n", (0..50).map(|_| prose(1000)).collect::<String>().lines().enumerate().map(|(i, l)| format!("{l} {}\n", "abcdefghij".chars().nth(i / 1000 % 10).unwrap())).collect::<String>());
        let out = squeeze(&text);
        assert!(out.chars().count() < MOST_KEPT_CHARS + 2000);
    }

    #[test]
    fn source_code_keeps_its_lone_braces() {
        let names = ["read", "write", "parse", "print", "merge", "split", "sort", "load", "save", "find", "open", "close", "start", "stop", "draw", "send", "take", "give", "push", "pull"];
        let text: String = (0..300).map(|i| format!("fn {}_{}() {{
    {}_{}();
}}

", names[i % 20], names[i / 20], names[(i + 7) % 20], names[i / 15])).collect();
        assert!(squeeze(&text).matches("}\n").count() > 200);
    }

    #[test]
    fn an_endless_line_is_cut_on_character_boundaries() {
        let out = squeeze(&"é".repeat(9000));
        assert!(out.contains("chars omitted"));
        assert!(out.chars().count() < 500);
    }

    #[test]
    fn invalid_or_empty_input_does_not_panic() {
        assert_eq!(squeeze(""), "");
        assert_eq!(squeeze(&"\n".repeat(7000)).lines().count(), HEAD_LINES + TAIL_LINES + 1);
    }
}

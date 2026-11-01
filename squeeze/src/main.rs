// Reads a command's output on stdin and writes a shorter one on stdout. It only removes
// what repeats: a line goes when many lines share its exact shape and it says nothing new.
// The caller keeps the full output and tells the model where it is.
use std::collections::HashMap;
use std::io::{self, Read, Write};

const WORTH_SQUEEZING_CHARS: usize = 6000;
const HEAD_LINES: usize = 15;
const TAIL_LINES: usize = 15;
// Under this many lines of one shape, the lines are content and all stay.
const REPEATED_FROM: usize = 20;
const KEPT_OF_A_SHAPE: usize = 3;
const MOST_ALARMS_OF_A_SHAPE: usize = 20;
// A value this far above the shape's usual ones is the line someone is looking for.
const OUTLIER_TIMES: f64 = 5.0;
const RARE_SHARE: f64 = 0.02;

const ALARMS: [&str; 13] = [
    "error", "fail", "fatal", "panic", "exception", "traceback", "warning", "denied", "not found", "cannot", "assert", "timeout", "timed out",
];

fn is_alarm(line: &str) -> bool {
    let lower = line.to_lowercase();
    ALARMS.iter().any(|word| lower.contains(word))
}

// Lines that differ only by numbers, ids or hashes have one shape. Their numbers come back to be compared.
fn shape_of(line: &str) -> (String, Vec<f64>) {
    let mut out = String::with_capacity(line.len());
    let mut numbers = Vec::new();
    let mut word = String::new();
    let mut flush = |word: &mut String, out: &mut String| {
        // A number, a number with its unit (`12ms`) or a hash: not a name such as `user233`, which stays itself.
        let digits = word.trim_end_matches(|c: char| !c.is_ascii_digit());
        let is_hash = word.len() >= 7 && word.chars().all(|c| c.is_ascii_hexdigit());
        if !digits.is_empty() && (digits.chars().all(|c| c.is_ascii_digit()) || is_hash) {
            out.push('#');
            numbers.push(digits.parse().unwrap_or(f64::NAN));
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

    (out, numbers)
}

// Lines of one shape whose number stands out: a rare value beside a dominant one (a 503 among 200s),
// or one far above the rest (48211ms among 15ms).
fn stand_outs(members: &[usize], numbers: &[Vec<f64>]) -> Vec<usize> {
    let mut found = Vec::new();
    for slot in 0..numbers[members[0]].len() {
        let mut values: Vec<(f64, usize)> = members.iter().filter_map(|&i| numbers[i].get(slot).filter(|v| v.is_finite()).map(|&v| (v, i))).collect();
        if values.len() < REPEATED_FROM {
            continue;
        }
        let mut counts: HashMap<u64, usize> = HashMap::new();
        for (value, _) in &values {
            *counts.entry(value.to_bits()).or_insert(0) += 1;
        }
        if counts.values().copied().max().unwrap_or(0) * 2 >= values.len() {
            let rare = ((values.len() as f64 * RARE_SHARE) as usize).max(1);
            found.extend(values.iter().filter(|(v, _)| counts[&v.to_bits()] <= rare).map(|&(_, i)| i));
        }
        values.sort_by(|a, b| a.0.total_cmp(&b.0));
        let usual = values[values.len() * 9 / 10].0;
        if usual > 0.0 {
            found.extend(values.iter().filter(|(v, _)| *v > usual * OUTLIER_TIMES).map(|&(_, i)| i));
        }
    }

    found
}

// `path:12: text` is a search hit, and each hit is an answer.
fn is_search_hit(line: &str) -> bool {
    let mut cells = line.splitn(3, ':');
    let (Some(path), Some(number), Some(_)) = (cells.next(), cells.next(), cells.next()) else { return false };
    !path.is_empty() && !path.contains(' ') && !number.is_empty() && number.chars().all(|c| c.is_ascii_digit())
}

fn looks_like_json(text: &str) -> bool {
    let start = text.trim_start();
    start.starts_with('[') || start.starts_with('{')
}

fn squeeze(text: &str) -> String {
    // Every record of a JSON answer is data of its own, however alike they look.
    if text.chars().count() < WORTH_SQUEEZING_CHARS || looks_like_json(text) {
        return text.to_string();
    }
    let lines: Vec<&str> = text.lines().collect();
    let (shapes, numbers): (Vec<String>, Vec<Vec<f64>>) = lines.iter().map(|line| shape_of(line)).unzip();
    let mut by_shape: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, shape) in shapes.iter().enumerate() {
        by_shape.entry(shape).or_default().push(i);
    }
    let mut keep = vec![true; lines.len()];
    for members in by_shape.values().filter(|members| members.len() >= REPEATED_FROM) {
        for &i in members {
            keep[i] = is_search_hit(lines[i]);
        }
        for &i in members.iter().take(KEPT_OF_A_SHAPE - 1).chain(members.last()) {
            keep[i] = true;
        }
        for i in stand_outs(members, &numbers) {
            keep[i] = true;
        }
        for &i in members.iter().filter(|&&i| is_alarm(lines[i])).take(MOST_ALARMS_OF_A_SHAPE) {
            keep[i] = true;
        }
    }
    for (i, kept) in keep.iter_mut().enumerate() {
        *kept |= i < HEAD_LINES || i + TAIL_LINES >= lines.len();
    }

    let mut out = String::with_capacity(text.len() / 2);
    let mut i = 0;
    while i < lines.len() {
        if keep[i] {
            out.push_str(lines[i]);
            out.push('\n');
            i += 1;
            continue;
        }
        let start = i;
        while i < lines.len() && !keep[i] {
            i += 1;
        }
        out.push_str(&format!("[{} similar lines omitted]\n", i - start));
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
        (0..n).map(|i| format!("INFO worker handled request {i} status 200 in {}ms\n", 10 + i % 9)).collect()
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
        assert!(out.contains("similar lines omitted"));
        assert!(out.len() * 10 < text.len());
    }

    #[test]
    fn first_and_last_lines_stay() {
        let out = squeeze(&format!("command started\n{}exit code 3\n", log(1000)));
        assert!(out.starts_with("command started\n"));
        assert!(out.ends_with("exit code 3\n"));
    }

    #[test]
    fn a_rare_status_and_a_slow_request_survive() {
        let mut lines: Vec<String> = log(1000).lines().map(String::from).collect();
        lines[300] = "INFO worker handled request 300 status 200 in 48211ms".into();
        lines[420] = "INFO worker handled request 420 status 503 in 13ms".into();
        let out = squeeze(&(lines.join("\n") + "\n"));
        assert!(out.contains("48211ms"));
        assert!(out.contains("status 503"));
        assert!(out.lines().count() < 60, "{}", out.lines().count());
    }

    #[test]
    fn distinct_content_of_any_size_is_left_whole() {
        let words = ["alpha", "bravo", "carol", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet"];
        let text: String = (0..1000).map(|i| format!("{} {} {}\n", words[i % 10], words[i / 10 % 10], words[i / 100 % 10])).collect();
        assert_eq!(squeeze(&text), text);
    }

    #[test]
    fn every_search_hit_stays_even_in_one_shape() {
        let text: String = (0..400).map(|i| format!("src/app.py:{i}: result = charge(order_{i})
")).collect();
        assert_eq!(squeeze(&text), text);
    }

    #[test]
    fn json_is_never_cut() {
        let text = format!("[{}]", (0..1000).map(|i| format!("{{\"id\": {i}}}")).collect::<Vec<_>>().join(",\n"));
        assert_eq!(squeeze(&text), text);
    }

    #[test]
    fn invalid_or_empty_input_does_not_panic() {
        assert_eq!(squeeze(""), "");
        squeeze(&"\n".repeat(7000));
        squeeze(&"é".repeat(9000));
    }
}

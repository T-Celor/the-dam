// Follows the projects the dam builds on: `UPSTREAM` says which release or commit each was ported from, `dam upstream`
// says which have moved since and what changed under the paths that were ported. Uses git (and gh, where it is there).
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub repo: String,
    pub followed: String,
    pub paths: Vec<String>,
}

pub fn parse(text: &str) -> Vec<Row> {
    text.lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .filter_map(|line| {
            let mut cells = line.split_whitespace();
            let (name, repo, followed) = (cells.next()?, cells.next()?, cells.next()?);
            let paths = cells.filter(|path| *path != "-").map(str::to_string).collect();
            Some(Row { name: name.to_string(), repo: repo.to_string(), followed: followed.to_string(), paths })
        })
        .collect()
}

fn is_commit(reference: &str) -> bool {
    reference.len() >= 7 && reference.chars().all(|c| c.is_ascii_hexdigit())
}

// The numbers of a release tag (`v5.3.3` is 5, 3, 3); None for a tag with anything else in it (a candidate, a branch).
fn version_of(tag: &str) -> Option<Vec<u64>> {
    let digits = tag.strip_prefix('v').unwrap_or(tag);
    digits.split('.').map(|part| part.parse().ok()).collect()
}

// The newest release among `tags`.
pub fn newest_tag<'a>(tags: impl Iterator<Item = &'a str>) -> Option<String> {
    tags.filter_map(|tag| version_of(tag).map(|version| (version, tag))).max().map(|(_, tag)| tag.to_string())
}

fn git_lines(args: &[&str]) -> Option<Vec<String>> {
    let run = Command::new("git").args(args).stdin(std::process::Stdio::null()).output().ok().filter(|run| run.status.success())?;

    Some(String::from_utf8_lossy(&run.stdout).lines().map(str::to_string).collect())
}

// Where the project is now: its HEAD for a project followed by commit, its newest release for one followed by release.
fn latest(row: &Row) -> Option<String> {
    let url = format!("https://github.com/{}", row.repo);
    if is_commit(&row.followed) {
        let lines = git_lines(&["ls-remote", &url, "HEAD"])?;
        return lines.first()?.split_whitespace().next().map(str::to_string);
    }
    let lines = git_lines(&["ls-remote", "--tags", "--refs", &url])?;

    newest_tag(lines.iter().filter_map(|line| line.rsplit_once("refs/tags/").map(|(_, tag)| tag)))
}

// The subjects of the commits since `followed`, and the changed files that sit under the ported paths; needs gh.
fn changes(row: &Row, latest: &str) -> Option<(Vec<String>, Vec<String>)> {
    let endpoint = format!("repos/{}/compare/{}...{}", row.repo, row.followed, latest);
    let ask = |query: &str| -> Option<Vec<String>> {
        let run = Command::new("gh").args(["api", &endpoint, "--jq", query]).stdin(std::process::Stdio::null()).output().ok().filter(|run| run.status.success())?;
        Some(String::from_utf8_lossy(&run.stdout).lines().map(str::to_string).collect())
    };
    let subjects = ask(".commits[].commit.message | split(\"\\n\")[0]")?;
    let files = ask(".files[].filename")?
        .into_iter()
        .filter(|file| row.paths.is_empty() || row.paths.iter().any(|path| file.starts_with(path.trim_end_matches('/'))))
        .collect();

    Some((subjects, files))
}

fn find_file() -> Option<PathBuf> {
    [crate::plugin_root(), std::env::current_dir().ok()].into_iter().flatten().map(|dir| dir.join("UPSTREAM")).find(|file| file.is_file())
}

fn check(strict: bool) -> i32 {
    let Some(file) = find_file() else {
        eprintln!("no UPSTREAM file in the plugin folder or here");
        return 2;
    };
    let rows = parse(&fs::read_to_string(&file).unwrap_or_default());
    let mut moved = 0;
    for row in &rows {
        let short = |reference: &str| if is_commit(reference) { reference.chars().take(10).collect::<String>() } else { reference.to_string() };
        let Some(now) = latest(row) else {
            println!("{:<18} {} -> unreachable", row.name, short(&row.followed));
            continue;
        };
        if now == row.followed {
            println!("{:<18} {} up to date", row.name, short(&row.followed));
            continue;
        }
        moved += 1;
        println!("{:<18} {} -> {}  https://github.com/{}/compare/{}...{}", row.name, short(&row.followed), short(&now), row.repo, row.followed, now);
        if let Some((subjects, files)) = changes(row, &now) {
            println!("  {} commits; {} files under the ported paths", subjects.len(), files.len());
            for subject in subjects.iter().rev().take(5) {
                println!("  - {subject}");
            }
            for file in files.iter().take(8) {
                println!("  * {file}");
            }
        }
    }
    if moved > 0 {
        println!("{moved} moved. After porting, run: dam upstream pin NAME");
    }

    i32::from(strict && moved > 0)
}

// Records that `name` has been ported up to where it is now.
fn pin(name: &str) -> i32 {
    let Some(file) = find_file() else {
        eprintln!("no UPSTREAM file in the plugin folder or here");
        return 2;
    };
    let text = fs::read_to_string(&file).unwrap_or_default();
    let Some(row) = parse(&text).into_iter().find(|row| row.name == name) else {
        eprintln!("{name} is not in UPSTREAM");
        return 1;
    };
    let Some(now) = latest(&row) else {
        eprintln!("could not reach {}", row.repo);
        return 1;
    };
    let rewritten: Vec<String> = text
        .lines()
        .map(|line| {
            let mut cells = line.split_whitespace();
            if cells.next() == Some(name) && !line.trim_start().starts_with('#') {
                line.replacen(&row.followed, &now, 1)
            } else {
                line.to_string()
            }
        })
        .collect();
    match fs::write(&file, rewritten.join("\n") + "\n") {
        Ok(()) => {
            println!("{name}: {} -> {now}", row.followed);
            0
        }
        Err(error) => {
            eprintln!("could not write {}: {error}", file.display());
            1
        }
    }
}

pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        None | Some("check") => check(false),
        Some("--check") => check(true),
        Some("pin") if args.len() > 1 => pin(&args[1]),
        _ => {
            eprintln!("usage: dam upstream [check | --check | pin NAME]");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rows_and_skips_comments() {
        let rows = parse("# note\n\nponytail  a/b  v5.3.3  skills hooks\ngraphify  c/d  v0.9.99  -\n");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], Row { name: "ponytail".into(), repo: "a/b".into(), followed: "v5.3.3".into(), paths: vec!["skills".into(), "hooks".into()] });
        assert!(rows[1].paths.is_empty());
    }

    #[test]
    fn newest_tag_compares_numbers_not_text() {
        let tags = ["v5.3.3", "v5.10.0", "v5.9.9", "v6.0.0-rc1", "nightly"];
        assert_eq!(newest_tag(tags.into_iter()), Some("v5.10.0".to_string()));
    }

    #[test]
    fn tells_a_commit_from_a_release() {
        assert!(is_commit("e9e1f06aa4e26e95cd4ed37210a0632202c429e9"));
        assert!(!is_commit("v5.3.3"));
    }
}

// dam-graph: a code graph made and queried locally. `update` writes .dam-graph/graph.tsv,
// `query "question"` answers from it. No network and no model, by construction.
use crate::scan::{self, lang_of, Kind, Scan};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;

const GRAPH_FILE: &str = ".dam-graph/graph.tsv";
const MOST_FILE_BYTES: u64 = 2_000_000;
const USUAL_METHODS: [&str; 34] = [
    "add", "get", "put", "set", "size", "length", "values", "keys", "map", "filter", "reduce", "sum", "log", "push", "pop", "append", "remove", "delete", "clear", "contains", "join", "split", "sort", "find", "of", "stream", "collect", "text", "write", "read", "close", "open", "toString", "equals",
];
const SKIPPED_DIRS: [&str; 7] = ["node_modules", "target", "dist", "build", "vendor", "__pycache__", "venv"];

struct Node {
    name: String,
    kind: Kind,
    file: usize,
    line: usize,
}

#[derive(Default)]
struct Graph {
    files: Vec<String>,
    nodes: Vec<Node>,
    // from, to, relation: c(alls), i(mports), b(ase), d(efines).
    edges: Vec<(usize, usize, char)>,
}

fn walk(dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !name.starts_with('.') && !SKIPPED_DIRS.contains(&name.as_str()) {
                walk(&path, out);
            }
        } else {
            out.push(path.to_string_lossy().replace('\\', "/").trim_start_matches("./").to_string());
        }
    }
}

// What git tracks, so ignored and generated files stay out; every file where there is no git.
pub fn source_files() -> Vec<String> {
    let listed = Command::new("git").args(["ls-files", "-z"]).output().ok().filter(|run| run.status.success());
    let mut files = match listed {
        Some(run) => String::from_utf8_lossy(&run.stdout).split('\0').filter(|f| !f.is_empty()).map(str::to_string).collect(),
        None => {
            let mut found = Vec::new();
            walk(Path::new("."), &mut found);
            found
        }
    };
    // A minified bundle is someone else's code on one line: nothing a graph can say about it helps.
    files.retain(|f| lang_of(f).is_some() && !f.contains(".min.") && !f.split('/').any(|part| SKIPPED_DIRS.contains(&part)));
    files.sort();

    files
}

fn scan_all(files: &[String]) -> Vec<Scan> {
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
    let chunk = files.len().div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = files
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .map(|file| {
                            let lang = lang_of(file).expect("source_files keeps only known languages");
                            let is_small = fs::metadata(file).is_ok_and(|m| m.len() <= MOST_FILE_BYTES);
                            match fs::read(file) {
                                Ok(text) if is_small => scan::scan(&text, lang),
                                _ => Scan::default(),
                            }
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|handle| handle.join().unwrap_or_default()).collect()
    })
}

fn graph_file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn stem(path: &str) -> &str {
    path.rsplit_once('.').map_or(path, |(head, _)| head)
}

// Finds the file an import names, by the longest path ending that matches.
// shortcut: `../` is dropped rather than resolved, so two same-named neighbours can be confused; resolve it if that shows.
fn import_targets(import: &str, by_last: &HashMap<&str, Vec<usize>>, files: &[String]) -> Option<usize> {
    let cleaned = import.replace("::", "/").replace('\\', "/");
    let is_path = cleaned.contains('/') && !cleaned.contains("./") || cleaned.starts_with('.') && cleaned.contains('/');
    let dotted = if is_path { cleaned.clone() } else { cleaned.trim_start_matches('.').replace('.', "/") };
    let mut parts: Vec<&str> = dotted.split('/').filter(|p| !p.is_empty() && *p != "." && *p != ".." && *p != "crate" && *p != "self" && *p != "super").collect();
    while let Some(last) = parts.last().copied() {
        let wanted = parts.join("/");
        let last = stem(last);
        for candidate in [last, "__init__", "index", "mod"] {
            let exact = if candidate == last { wanted.clone() } else { format!("{wanted}/{candidate}") };
            let exact = stem(&exact).to_string();
            if let Some(found) = by_last.get(if candidate == last { last } else { candidate }).and_then(|list| {
                list.iter().copied().find(|&f| {
                    let path = stem(&files[f]);
                    path == exact || path.ends_with(&format!("/{exact}"))
                })
            }) {
                return Some(found);
            }
        }
        parts.pop();
    }

    None
}

fn build() -> Graph {
    let files = source_files();
    let scans = scan_all(&files);
    let mut graph = Graph { files, ..Graph::default() };
    let mut by_last: HashMap<&str, Vec<usize>> = HashMap::new();
    for (f, file) in graph.files.iter().enumerate() {
        by_last.entry(stem(file.rsplit('/').next().unwrap_or(file))).or_default().push(f);
    }

    // One node per file first, then each definition: same name and file means the same node.
    let mut first_def: Vec<HashMap<&str, usize>> = Vec::with_capacity(scans.len());
    let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut nodes = Vec::with_capacity(graph.files.len());
    let mut edges = Vec::new();
    for (f, file) in graph.files.iter().enumerate() {
        nodes.push(Node { name: file.rsplit('/').next().unwrap_or(file).to_string(), kind: Kind::File, file: f, line: 1 });
    }
    for (f, scan) in scans.iter().enumerate() {
        let mut here: HashMap<&str, usize> = HashMap::new();
        for def in &scan.defs {
            let id = *here.entry(def.name.as_str()).or_insert_with(|| {
                nodes.push(Node { name: def.name.clone(), kind: def.kind, file: f, line: def.line });
                edges.push((f, nodes.len() - 1, 'd'));
                by_name.entry(def.name.as_str()).or_default().push(nodes.len() - 1);
                nodes.len() - 1
            });
            let _ = id;
        }
        first_def.push(here);
    }

    let mut imported: Vec<HashSet<usize>> = vec![HashSet::new(); scans.len()];
    for (f, scan) in scans.iter().enumerate() {
        for import in &scan.imports {
            if let Some(target) = import_targets(import, &by_last, &graph.files).filter(|&t| t != f && lang_of(&graph.files[t]) == lang_of(&graph.files[f])) {
                imported[f].insert(target);
            }
        }
        for &target in &imported[f] {
            edges.push((f, target, 'i'));
        }
    }

    let families: Vec<Option<scan::Lang>> = graph.files.iter().map(|file| lang_of(file)).collect();
    let file_types: Vec<HashMap<&str, &str>> = scans.iter().map(|scan| scan.types.iter().rev().map(|(var, kind)| (var.as_str(), kind.as_str())).collect()).collect();
    let same_lang = |a: usize, b: usize| families[a] == families[b];
    // By the receiver's type when the file says it, then the same file, imported files,
    // and a name that is unique everywhere; otherwise no edge, as a wrong one misleads.
    let resolve = |f: usize, name: &str, is_member: bool, receiver: Option<&str>, nodes: &[Node]| -> Option<usize> {
        if let Some(receiver) = receiver {
            let type_name = file_types[f].get(receiver).copied().or(receiver.chars().next().filter(|c| c.is_uppercase()).map(|_| receiver));
            if let Some(places) = type_name.and_then(|t| by_name.get(t)) {
                let places = places.iter().map(|&id| nodes[id].file).filter(|&file| same_lang(f, file));
                let mut found: Vec<usize> = places.filter_map(|file| first_def[file].get(name).copied()).collect();
                found.sort_by_key(|&id| !imported[f].contains(&nodes[id].file));
                if let Some(&id) = found.first() {
                    return Some(id);
                }
            }
        }
        if !is_member {
            if let Some(&id) = first_def[f].get(name) {
                return Some(id);
            }
        }
        // `list.add(` is rarely the project's own `add`, whichever single one exists.
        if is_member && USUAL_METHODS.contains(&name) {
            return None;
        }
        let all: Vec<usize> = by_name.get(name)?.iter().copied().filter(|&id| same_lang(f, nodes[id].file)).collect();
        // A module used by name: `hook.main(` after importing `hook`.
        let named = receiver.and_then(|r| all.iter().copied().find(|&id| imported[f].contains(&nodes[id].file) && stem(graph_file_name(&graph.files[nodes[id].file])) == r));
        if named.is_some() {
            return named;
        }
        if let Some(&id) = all.iter().find(|&&id| imported[f].contains(&nodes[id].file)) {
            return Some(id);
        }

        (all.len() == 1).then(|| all[0])
    };
    let mut seen = HashSet::new();
    for (f, scan) in scans.iter().enumerate() {
        let id_of = |def: usize| first_def[f].get(scan.defs[def].name.as_str()).copied();
        for call in &scan.calls {
            let from = call.owner.and_then(id_of).unwrap_or(f);
            // Calling itself is recursion when bare, and only a same-named method elsewhere when on an object.
            if let Some(to) = resolve(f, &call.name, call.is_member, call.receiver.as_deref(), &nodes).filter(|&to| to != from || !call.is_member) {
                if seen.insert((from, to, 'c')) {
                    edges.push((from, to, 'c'));
                }
            }
        }
        for (def, base) in &scan.bases {
            if let (Some(from), Some(to)) = (id_of(*def), resolve(f, base, false, None, &nodes)) {
                if from != to && seen.insert((from, to, 'b')) {
                    edges.push((from, to, 'b'));
                }
            }
        }
    }
    graph.nodes = nodes;
    graph.edges = edges;

    graph
}

fn kind_letter(kind: Kind) -> char {
    match kind {
        Kind::File => 'F',
        Kind::Function => 'f',
        Kind::Type => 't',
    }
}

fn save(graph: &Graph) -> std::io::Result<()> {
    let mut out = String::new();
    for file in &graph.files {
        out.push_str(&format!("P\t{file}\n"));
    }
    for node in &graph.nodes {
        out.push_str(&format!("N\t{}\t{}\t{}\t{}\n", kind_letter(node.kind), node.name, node.file, node.line));
    }
    for (from, to, relation) in &graph.edges {
        out.push_str(&format!("E\t{from}\t{to}\t{relation}\n"));
    }
    fs::create_dir_all(".dam-graph")?;
    // Written aside then moved, so a query never reads half a graph.
    let temp = format!("{GRAPH_FILE}.tmp");
    fs::write(&temp, out)?;
    fs::rename(temp, GRAPH_FILE)
}

fn load(path: &str) -> std::io::Result<Graph> {
    let text = fs::read_to_string(path)?;
    let mut graph = Graph::default();
    for line in text.lines() {
        let mut cells = line.split('\t');
        match cells.next() {
            Some("P") => graph.files.push(cells.next().unwrap_or("").to_string()),
            Some("N") => {
                let kind = match cells.next() {
                    Some("F") => Kind::File,
                    Some("t") => Kind::Type,
                    _ => Kind::Function,
                };
                let name = cells.next().unwrap_or("").to_string();
                let file = cells.next().and_then(|c| c.parse().ok()).unwrap_or(0);
                let line = cells.next().and_then(|c| c.parse().ok()).unwrap_or(1);
                graph.nodes.push(Node { name, kind, file, line });
            }
            Some("E") => {
                let from = cells.next().and_then(|c| c.parse().ok()).unwrap_or(0);
                let to = cells.next().and_then(|c| c.parse().ok()).unwrap_or(0);
                graph.edges.push((from, to, cells.next().and_then(|c| c.chars().next()).unwrap_or('c')));
            }
            _ => {}
        }
    }

    Ok(graph)
}

const STOP_WORDS: [&str; 44] = [
    "the", "how", "does", "what", "where", "when", "why", "which", "who", "and", "for", "from", "with", "that", "this", "are", "work", "works", "code", "function", "file", "class", "comment", "est", "que", "qui", "les", "des", "une", "dans", "pour", "fonctionne", "fait", "quoi", "avec", "sur", "par", "the", "use", "used", "uses", "get", "gets", "call",
];

// Lowercased pieces of a name: snake_case, camelCase and kebab-case all split.
fn pieces(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut was_lower = false;
    for c in name.chars() {
        if !c.is_alphanumeric() {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            was_lower = false;
            continue;
        }
        if c.is_uppercase() && was_lower {
            out.push(std::mem::take(&mut word));
        }
        word.extend(c.to_lowercase());
        was_lower = c.is_lowercase() || c.is_ascii_digit();
    }
    if !word.is_empty() {
        out.push(word);
    }

    out
}

fn question_words(question: &str) -> Vec<String> {
    let words: Vec<String> = pieces(question).into_iter().filter(|w| w.len() >= 3 && !STOP_WORDS.contains(&w.as_str())).collect();
    // "deny list" also finds `denylist`.
    let joined: Vec<String> = words.windows(2).map(|pair| pair.concat()).collect();

    // A name asked for as written (`compute_total_7`) must find exactly it.
    let whole = question.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|word| word.len() >= 3).map(str::to_lowercase).filter(|word| !words.contains(word));

    words.iter().cloned().chain(joined).chain(whole).collect()
}

fn score(node: &Node, file: &str, words: &[String]) -> usize {
    let name = node.name.to_lowercase();
    let own = pieces(&node.name);
    let place = pieces(stem(file));
    let mut total = 0;
    let mut matched = 0;
    for word in words {
        let points = if name == *word || stem(&name) == word {
            8
        } else if own.contains(word) {
            5
        } else if word.len() >= 5 && name.contains(word.as_str()) {
            3
        } else if place.contains(word) {
            1
        } else {
            0
        };
        total += points;
        matched += usize::from(points > 1);
    }

    // A name that answers several words beats one that answers a single word well.
    total * matched.max(1)
}

const SEEDS: usize = 12;
const MOST_NEIGHBOURS: usize = 8;
const MOST_ANSWER_CHARS: usize = 8_000;

fn label(graph: &Graph, id: usize) -> String {
    let node = &graph.nodes[id];
    match node.kind {
        Kind::File => graph.files[node.file].clone(),
        Kind::Function => format!("{}()", node.name),
        Kind::Type => node.name.clone(),
    }
}

fn answer(graph: &Graph, question: &str) -> String {
    let words = question_words(question);
    let mut ranked: Vec<(usize, usize)> = graph.nodes.iter().enumerate().map(|(id, node)| (score(node, &graph.files[node.file], &words), id)).filter(|&(points, _)| points > 1).collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    if ranked.is_empty() {
        return format!("No node matches: {}\n", words.join(" "));
    }
    let mut out = format!("{} nodes, {} edges. Best matches of {}:\n", graph.nodes.len(), graph.edges.len(), ranked.len());
    let names = |ids: Vec<usize>| {
        let more = ids.len().saturating_sub(MOST_NEIGHBOURS);
        let mut shown: Vec<String> = ids.iter().take(MOST_NEIGHBOURS).map(|&id| label(graph, id)).collect();
        if more > 0 {
            shown.push(format!("+{more} more"));
        }
        shown.join(", ")
    };
    for &(_, id) in ranked.iter().take(SEEDS) {
        let node = &graph.nodes[id];
        out.push_str(&format!("\n{} {}:{}\n", label(graph, id), graph.files[node.file], node.line));
        let related = |from: bool, relation: char| -> Vec<usize> {
            graph.edges.iter().filter(|e| e.2 == relation && if from { e.0 == id } else { e.1 == id }).map(|e| if from { e.1 } else { e.0 }).collect()
        };
        for (title, ids) in [
            ("calls", related(true, 'c')),
            ("called by", related(false, 'c')),
            ("extends", related(true, 'b')),
            ("extended by", related(false, 'b')),
            ("imports", related(true, 'i')),
            ("imported by", related(false, 'i')),
            ("defines", related(true, 'd')),
        ] {
            if !ids.is_empty() {
                out.push_str(&format!("  {title}: {}\n", names(ids)));
            }
        }
        if out.len() > MOST_ANSWER_CHARS {
            out.push_str("\n[answer cut: ask about one of these names for more]\n");
            break;
        }
    }

    out
}

pub fn run(args: &[String]) {
    match args.first().map(String::as_str) {
        Some("update") => {
            let graph = build();
            match save(&graph) {
                Ok(()) => println!("{} files, {} nodes, {} edges", graph.files.len(), graph.nodes.len(), graph.edges.len()),
                Err(error) => {
                    eprintln!("could not write {GRAPH_FILE}: {error}");
                    std::process::exit(1);
                }
            }
        }
        Some("query") if args.len() > 1 => match load(GRAPH_FILE) {
            Ok(graph) => print!("{}", answer(&graph, &args[1..].join(" "))),
            Err(error) => {
                eprintln!("no graph here, run `dam-graph update`: {error}");
                std::process::exit(1);
            }
        },
        _ => {
            eprintln!("usage: dam-graph update | dam-graph query \"question\"");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Graph {
        Graph {
            files: vec!["src/deny_list.py".into(), "src/cli.py".into()],
            nodes: vec![
                Node { name: "deny_list.py".into(), kind: Kind::File, file: 0, line: 1 },
                Node { name: "cli.py".into(), kind: Kind::File, file: 1, line: 1 },
                Node { name: "apply_deny".into(), kind: Kind::Function, file: 0, line: 7 },
                Node { name: "run".into(), kind: Kind::Function, file: 1, line: 3 },
            ],
            edges: vec![(3, 2, 'c'), (1, 0, 'i'), (0, 2, 'd')],
        }
    }

    #[test]
    fn names_split_every_way() {
        assert_eq!(pieces("applyDenyList"), ["apply", "deny", "list"]);
        assert_eq!(pieces("deny_list-v2"), ["deny", "list", "v2"]);
    }

    #[test]
    fn a_question_finds_the_function_and_who_calls_it() {
        let out = answer(&sample(), "how does the deny list get applied, apply deny");
        assert!(out.contains("apply_deny() src/deny_list.py:7"));
        assert!(out.contains("called by: run()"));
    }

    #[test]
    fn a_name_as_written_comes_first() {
        let mut graph = sample();
        graph.nodes.push(Node { name: "compute_total_1".into(), kind: Kind::Function, file: 0, line: 1 });
        graph.nodes.push(Node { name: "compute_total_7".into(), kind: Kind::Function, file: 1, line: 9 });
        let out = answer(&graph, "who defines compute_total_7");
        assert!(out.find("compute_total_7()").unwrap() < out.find("compute_total_1()").unwrap_or(usize::MAX));
    }

    #[test]
    fn no_match_says_so() {
        assert!(answer(&sample(), "zebra").starts_with("No node matches"));
    }

    #[test]
    fn imports_resolve_by_longest_ending() {
        let files: Vec<String> = ["a/b/tool.py", "c/tool.py", "web/src/lib/x.ts", "pkg/__init__.py"].map(String::from).to_vec();
        let mut by_last: HashMap<&str, Vec<usize>> = HashMap::new();
        for (f, file) in files.iter().enumerate() {
            by_last.entry(stem(file.rsplit('/').next().unwrap())).or_default().push(f);
        }
        assert_eq!(import_targets("a.b.tool", &by_last, &files), Some(0));
        assert_eq!(import_targets("c.tool.helper", &by_last, &files), Some(1));
        assert_eq!(import_targets("./lib/x", &by_last, &files), Some(2));
        assert_eq!(import_targets("pkg", &by_last, &files), Some(3));
        assert_eq!(import_targets("os", &by_last, &files), None);
    }

    #[test]
    fn a_saved_graph_reads_back_the_same() {
        let dir = std::env::temp_dir().join(format!("dam-graph-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("g.tsv");
        let graph = sample();
        let mut text = String::new();
        for file in &graph.files {
            text.push_str(&format!("P\t{file}\n"));
        }
        for node in &graph.nodes {
            text.push_str(&format!("N\t{}\t{}\t{}\t{}\n", kind_letter(node.kind), node.name, node.file, node.line));
        }
        for e in &graph.edges {
            text.push_str(&format!("E\t{}\t{}\t{}\n", e.0, e.1, e.2));
        }
        fs::write(&path, text).unwrap();
        let back = load(path.to_str().unwrap()).unwrap();
        assert_eq!(answer(&back, "apply deny"), answer(&graph, "apply deny"));
        fs::remove_dir_all(dir).unwrap();
    }
}

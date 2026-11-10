// A use case in, the right pieces called: each prompt is read for what the person wants (a feature, a fix, a refactor, a
// review, documents, a question) and the agent is told, in a few lines, which of the dam's pieces to use and in what order.
// The pieces that matter are also enforced elsewhere (the review hook, the Stop gate); this says what to do before they have to.
use serde_json::{json, Value};
use std::fs;
use std::io::Read;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Case {
    Review,
    Docs,
    Bugfix,
    Refactor,
    Feature,
    Question,
}

pub const ALL: [Case; 6] = [Case::Feature, Case::Bugfix, Case::Refactor, Case::Review, Case::Docs, Case::Question];

impl Case {
    pub fn name(self) -> &'static str {
        match self {
            Case::Review => "review",
            Case::Docs => "docs",
            Case::Bugfix => "bugfix",
            Case::Refactor => "refactor",
            Case::Feature => "feature",
            Case::Question => "question",
        }
    }

    pub fn from_name(name: &str) -> Option<Case> {
        ALL.into_iter().find(|case| case.name() == name.trim().to_lowercase())
    }

    // What to do, in order, with the dam's pieces. Short on purpose: it is paid for on every prompt it applies to.
    pub fn playbook(self) -> &'static str {
        match self {
            Case::Feature => "Use case: new feature. Call the Skill tool now with `the-dam:beaver`. 1) Ask the map (`graph query`) what exists and who calls it; reuse before writing. 2) Touching several files or needing a choice: Skill `the-dam:design` writes the design, Skill `the-dam:goldfish` has fresh reviewers challenge it, Skill `the-dam:resolve` settles the findings; do that before implementing. 3) Implement the smallest complete change and run the tests. 4) Before you finish, the advisor reviews it (mandatory from 3 code files or any sensitive one). Broad searches go to a worker.",
            Case::Bugfix => "Use case: bug fix. Call the Skill tool now with `the-dam:beaver`. 1) Ask the map who calls the failing code. 2) Reproduce it first, with a failing test or the exact command. 3) Fix the root cause once, in the shared code, not each caller. 4) Run the test. After two failed attempts, ask the advisor before a third; it also reviews before you finish if the fix touches several files or something sensitive.",
            Case::Refactor => "Use case: refactor or rename. Call the Skill tool now with `the-dam:beaver`. 1) Ask the map for every caller and definition of what moves; list them before editing. 2) Hand the mechanical edits to a worker with that list. 3) Run the tests, then the advisor reviews the whole change before you finish (mandatory from 3 code files). No behaviour change: if one is needed, say so first.",
            Case::Review => "Use case: review. Call the Skill tool with `the-dam:beaver-review` for a change, `the-dam:beaver-audit` for the repo, `the-dam:goldfish` for a design document (then `the-dam:resolve`, or `the-dam:goldfish-loop` until two clean rounds). Read the change and the code it touches, with the map for callers. Findings first, ranked Blocker, Major, Nit, one line each with file:line. Change nothing.",
            Case::Docs => "Use case: documentation. Write from the code, not from memory: the map gives what a folder defines and who uses it. Call the Skill tool with `the-dam:bootstrap` for a README per folder, `the-dam:design` for a spec, `the-dam:drift` to find what documents say that the code no longer backs, then `the-dam:goldfish` on a design. Fix every missing file or name before finishing.",
            Case::Question => "Use case: question about the code. Answer from the map first (`graph query`), then read only the lines it points to. Change nothing, no design, no review.",
        }
    }
}

fn words(prompt: &str) -> Vec<String> {
    prompt.to_lowercase().split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|word| !word.is_empty()).map(str::to_string).collect()
}

fn has(words: &[String], prompt: &str, candidates: &[&str]) -> bool {
    candidates.iter().any(|candidate| if candidate.contains(' ') { prompt.contains(candidate) } else { words.iter().any(|word| word == candidate) })
}

// What the prompt asks for, or None when it is not a request the playbooks cover (a reply, a command, a chat).
pub fn classify(prompt: &str) -> Option<Case> {
    let text = prompt.trim().to_lowercase();
    // A command, a reply, or a message a tool sent (a sub-agent's report arrives as `<...>`), not a request.
    if text.is_empty() || text.starts_with('/') || text.starts_with('<') || text.chars().count() < 12 {
        return None;
    }
    // A path names a file, it does not ask for documents (`docs/design.md`).
    let text = text.split_whitespace().filter(|token| !token.contains(['/', '\\'])).collect::<Vec<_>>().join(" ");
    let words = words(&text);
    if has(&words, &text, &["review", "audit", "relis", "relire", "revois", "revoir", "critique", "critiquer", "code review"]) {
        return Some(Case::Review);
    }
    if has(&words, &text, &["fix", "bug", "bugs", "broken", "crash", "crashes", "error", "errors", "fails", "failing", "regression", "corrige", "corriger", "répare", "reparer", "répare", "erreur", "erreurs", "plante", "cassé", "casse", "marche pas", "fonctionne pas", "doesn't work", "not working"]) {
        return Some(Case::Bugfix);
    }
    if has(&words, &text, &["refactor", "refactoring", "rename", "extract", "migrate", "rewrite", "renomme", "renommer", "déplace", "deplace", "refactorise", "réécris", "reecris", "migre", "clean up", "move to"]) {
        return Some(Case::Refactor);
    }
    if has(&words, &text, &["add", "implement", "create", "build", "support", "ajoute", "ajouter", "implémente", "implemente", "crée", "cree", "construis", "développe", "developpe", "feature", "fonctionnalité", "new endpoint", "nouvelle"]) {
        return Some(Case::Feature);
    }
    if has(&words, &text, &["readme", "document", "documente", "documenter", "documentation", "docs", "docstring", "docstrings", "spec", "specification", "design", "out of date", "outdated", "périmé", "perime", "conception"]) {
        return Some(Case::Docs);
    }
    let asks = text.ends_with('?') || has(&words, &text, &["where", "how", "what", "why", "which", "who", "où", "comment", "pourquoi", "quoi", "quel", "quelle", "qui", "explain", "explique"]);

    asks.then_some(Case::Question)
}

fn mode() -> String {
    std::env::var("DAM_USECASE").ok().map(|value| value.to_lowercase()).unwrap_or_default()
}

pub fn enabled() -> bool {
    !matches!(mode().as_str(), "off" | "0")
}

// The playbook to give for this prompt in this session, or None: not twice in a row for the same case.
pub fn note(input: &Value, state_dir: &std::path::Path) -> Option<String> {
    let prompt = input["prompt"].as_str().or_else(|| input["user_prompt"].as_str())?;
    let case = classify(prompt)?;
    let session: String = input["session_id"].as_str().or_else(|| input["sessionId"].as_str()).unwrap_or("session").chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    let file = state_dir.join("goldfish").join(format!("usecase-{session}.txt"));
    if fs::read_to_string(&file).is_ok_and(|last| last.trim() == case.name()) {
        return None;
    }
    if let Some(folder) = file.parent() {
        let _ = fs::create_dir_all(folder);
    }
    let _ = fs::write(&file, case.name());

    Some(case.playbook().to_string())
}

// `dam usecase hook`: a UserPromptSubmit payload on stdin; the playbook as context on stdout, or nothing.
pub fn hook() {
    crate::watchdog(25);
    if !enabled() {
        return;
    }
    let mut bytes = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    let Ok(input) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else { return };
    // The map is there before the first search: built now where the project is small enough.
    if let Some(cwd) = input["cwd"].as_str().filter(|cwd| !cwd.is_empty()).filter(|_| input["prompt"].as_str().is_some_and(|prompt| classify(prompt).is_some())) {
        if std::env::set_current_dir(cwd).is_ok() {
            crate::graph::ensure_built();
        }
    }
    let Some(playbook) = note(&input, &crate::economizer::state_root()) else { return };
    print!("{}", json!({"hookSpecificOutput": {"hookEventName": "UserPromptSubmit", "additionalContext": playbook}}));
}

pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("hook") => {
            hook();
            0
        }
        // `dam usecase show [NAME]`: the playbooks; `dam usecase classify TEXT`: which one a prompt gets.
        Some("show") => {
            for case in args.get(1).and_then(|name| Case::from_name(name)).map_or_else(|| ALL.to_vec(), |case| vec![case]) {
                println!("{}: {}\n", case.name(), case.playbook());
            }
            0
        }
        Some("classify") if args.len() > 1 => {
            println!("{}", classify(&args[1..].join(" ")).map_or("none", Case::name));
            0
        }
        _ => {
            eprintln!("usage: dam usecase show [NAME] | classify TEXT | hook");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_get_the_use_case_they_ask_for() {
        let cases = [
            ("Add retry with backoff to our HTTP calls", Case::Feature),
            ("Ajoute un champ email à la page de profil", Case::Feature),
            ("The login crashes when the password is empty", Case::Bugfix),
            ("corrige le bug sur le paiement", Case::Bugfix),
            ("Rename charge_customer to bill_customer everywhere", Case::Refactor),
            ("renomme cette fonction partout", Case::Refactor),
            ("Review my last commit please", Case::Review),
            ("relis ce changement avant que je pousse", Case::Review),
            ("Update the README with the new install steps", Case::Docs),
            ("Where is split_line defined?", Case::Question),
            ("comment fonctionne le cache ici", Case::Question),
            // A path in the prompt names a file; the verbs decide.
            ("Add idempotency keys to charges. Write a short design in docs/idempotency-design.md, then implement it.", Case::Feature),
        ];
        for (prompt, expected) in cases {
            assert_eq!(classify(prompt), Some(expected), "{prompt}");
        }
    }

    #[test]
    fn replies_commands_and_chat_get_nothing() {
        for prompt in ["ok", "oui merci", "/the-dam:goldfish docs/a.md", "yes go ahead", "", "<task-notification>Review is back, fix the findings</task-notification>"] {
            assert_eq!(classify(prompt), None, "{prompt}");
        }
        // A fix that mentions documents is still a fix.
        assert_eq!(classify("fix the bug in the docs build"), Some(Case::Bugfix));
    }

    #[test]
    fn the_playbook_is_given_once_per_case_in_a_session() {
        let dir = std::env::temp_dir().join(format!("usecase-{}", std::process::id()));
        let input = |prompt: &str| json!({"session_id": "s1", "prompt": prompt});
        assert!(note(&input("Add retry with backoff to our HTTP calls"), &dir).unwrap().contains("new feature"));
        assert!(note(&input("Add a second endpoint for invoices too"), &dir).is_none(), "same case twice");
        assert!(note(&input("The invoices crash on empty input"), &dir).unwrap().contains("bug fix"));
        assert!(note(&input("Add another feature now"), &dir).is_some(), "the case changed back");
        fs::remove_dir_all(&dir).ok();
    }
}

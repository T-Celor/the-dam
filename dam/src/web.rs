// A web page as Markdown, the way markdowner (MIT, https://github.com/supermemoryai/markdowner) gives it to a model:
// the page's main content picked as Mozilla's Readability does, then written out as Turndown would, with GitHub
// tables. Where no main content stands out, the whole page without its scripts and styles, as markdowner's
// detailed mode. It reads what a tool already fetched: no network, no browser and no model, and its own lenient
// HTML reader, so the programs carry no parser library.
use std::collections::HashMap;
use std::sync::LazyLock;

static UNLIKELY: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(r"(?i)-ad-|ai2html|banner|breadcrumbs|combx|comment|community|cover-wrap|disqus|extra|footer|gdpr|header|legends|menu|related|remark|replies|rss|shoutbox|sidebar|skyscraper|social|sponsor|supplemental|ad-break|agegate|pagination|pager|popup|yom-remote|cookie|consent|newsletter|share").expect("fixed"));
static MAYBE: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(r"(?i)and|article|body|column|content|main|shadow").expect("fixed"));
static POSITIVE: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(r"(?i)article|body|content|entry|hentry|h-entry|main|page|pagination|post|text|blog|story").expect("fixed"));
static NEGATIVE: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(r"(?i)-ad-|hidden|^hid$| hid$| hid |^hid |banner|combx|comment|com-|contact|footer|gdpr|masthead|media|meta|outbrain|promo|related|scroll|share|shoutbox|sidebar|skyscraper|sponsor|shopping|tags|widget").expect("fixed"));

// Never part of what a reader sees.
const DROPPED: [&str; 9] = ["script", "style", "noscript", "iframe", "template", "svg", "canvas", "object", "embed"];
// Around the content, not in it, once the main content is looked for.
const CHROME: [&str; 6] = ["nav", "aside", "footer", "header", "form", "dialog"];
const MOST_HTML_BYTES: usize = 8 * 1024 * 1024;
const LEAST_ARTICLE_CHARS: usize = 500;

#[derive(Debug, PartialEq)]
pub struct Page {
    pub title: String,
    pub markdown: String,
    // True where no main content stood out and the whole page was kept.
    pub whole: bool,
}

struct Node {
    tag: String,
    attrs: Vec<(String, String)>,
    text: String,
    kids: Vec<usize>,
    parent: Option<usize>,
}

struct Tree {
    nodes: Vec<Node>,
    title: String,
}

impl Tree {
    fn attr(&self, at: usize, name: &str) -> Option<&str> {
        self.nodes[at].attrs.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    fn class_and_id(&self, at: usize) -> String {
        format!("{} {}", self.attr(at, "class").unwrap_or(""), self.attr(at, "id").unwrap_or(""))
    }

    fn text_of(&self, at: usize, out: &mut String) {
        let node = &self.nodes[at];
        if node.tag.is_empty() {
            out.push_str(&node.text);
        }
        for &kid in &node.kids {
            self.text_of(kid, out);
        }
    }

    fn text_len(&self, at: usize) -> usize {
        let mut text = String::new();
        self.text_of(at, &mut text);

        text.split_whitespace().map(|word| word.chars().count() + 1).sum()
    }

    fn link_density(&self, at: usize) -> f64 {
        let all = self.text_len(at);
        if all == 0 {
            return 0.0;
        }
        let mut links = 0;
        self.each(at, &mut |tree, node| {
            if tree.nodes[node].tag == "a" {
                links += tree.text_len(node);
                return false;
            }
            true
        });

        links as f64 / all as f64
    }

    // Visits `at` and below, depth first; `visit` returning false skips a node's children.
    fn each(&self, at: usize, visit: &mut dyn FnMut(&Tree, usize) -> bool) {
        if visit(self, at) {
            for &kid in &self.nodes[at].kids {
                self.each(kid, visit);
            }
        }
    }

    fn find(&self, tag: &str) -> Vec<usize> {
        let mut found = Vec::new();
        self.each(0, &mut |tree, node| {
            if tree.nodes[node].tag == tag {
                found.push(node);
            }
            true
        });

        found
    }
}

// Elements that hold no content and have no end tag.
const VOID: [&str; 14] = ["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];
// Elements whose content is text up to their own end tag, whatever it holds.
const RAW: [&str; 5] = ["script", "style", "textarea", "title", "xmp"];
// Elements that end an open paragraph, as HTML parsing does.
const CLOSES_P: [&str; 27] = ["address", "article", "aside", "blockquote", "details", "div", "dl", "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hr", "main", "nav", "ol", "p", "pre", "section", "table"];

fn entity(name: &str) -> Option<char> {
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(code).filter(|c| *c != '\0');
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "laquo" => '«',
        "raquo" => '»',
        "bull" => '•',
        "middot" => '·',
        "times" => '×',
        "divide" => '÷',
        "euro" => '€',
        "pound" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "deg" => '°',
        "plusmn" => '±',
        "para" => '¶',
        "sect" => '§',
        "rarr" => '→',
        "larr" => '←',
        "uarr" => '↑',
        "darr" => '↓',
        "harr" => '↔',
        "eacute" => 'é',
        "egrave" => 'è',
        "ecirc" => 'ê',
        "agrave" => 'à',
        "acirc" => 'â',
        "ccedil" => 'ç',
        "ocirc" => 'ô',
        "ucirc" => 'û',
        "ugrave" => 'ù',
        "icirc" => 'î',
        "iuml" => 'ï',
        "euml" => 'ë',
        "uuml" => 'ü',
        "ouml" => 'ö',
        "auml" => 'ä',
        "szlig" => 'ß',
        "zwj" | "zwnj" | "shy" => '\u{200b}',
        "thinsp" | "ensp" | "emsp" => ' ',
        _ => return None,
    })
}

// `&amp;`, `&#233;` and `&eacute;` as their characters; an unknown or unterminated one stays as written.
fn decode(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let end = tail.char_indices().take(32).find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '#')).map(|(n, c)| (n, c == ';'));
        match end.and_then(|(n, semi)| entity(&tail[..n]).map(|c| (n, semi, c))) {
            Some((n, semi, c)) => {
                if c != '\u{200b}' {
                    out.push(c);
                }
                rest = &tail[n + usize::from(semi)..];
            }
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);

    out
}

// The attributes in a start tag's text after its name: `a=1 b='2' c="3" d`.
fn attributes(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == '/') {
            i += 1;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() && !matches!(chars[i], '=' | '>' | '/') {
            i += 1;
        }
        if start == i {
            i += 1;
            continue;
        }
        let name: String = chars[start..i].iter().collect::<String>().to_ascii_lowercase();
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < chars.len() && chars[i] == '=' {
            i += 1;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i < chars.len() && matches!(chars[i], '"' | '\'') {
                let quote = chars[i];
                i += 1;
                let from = i;
                while i < chars.len() && chars[i] != quote {
                    i += 1;
                }
                value = chars[from..i.min(chars.len())].iter().collect();
                i += 1;
            } else {
                let from = i;
                while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '>' {
                    i += 1;
                }
                value = chars[from..i].iter().collect();
            }
        }
        if !out.iter().any(|(known, _): &(String, String)| *known == name) {
            out.push((name, decode(&value)));
        }
    }

    out
}

struct Builder {
    tree: Tree,
    // Open elements, innermost last; the root is always open.
    open: Vec<usize>,
    // Inside an element left out: its whole content goes with it.
    dropped: usize,
}

impl Builder {
    fn current(&self) -> usize {
        *self.open.last().unwrap_or(&0)
    }

    fn is_open(&self, tag: &str) -> Option<usize> {
        self.open.iter().rposition(|&at| self.tree.nodes[at].tag == tag)
    }

    fn close_to(&mut self, depth: usize) {
        self.open.truncate(depth.max(1));
    }

    // HTML's implied end tags: a new paragraph, item, row or cell ends the open one.
    fn imply_end(&mut self, tag: &str) {
        let ends = |open: &str| match tag {
            "li" => open == "li",
            "dt" | "dd" => matches!(open, "dt" | "dd"),
            "tr" => matches!(open, "tr" | "td" | "th"),
            "td" | "th" => matches!(open, "td" | "th"),
            "option" => open == "option",
            "thead" | "tbody" | "tfoot" => matches!(open, "thead" | "tbody" | "tfoot" | "tr" | "td" | "th"),
            _ => false,
        };
        let stops = |open: &str| match tag {
            "li" => matches!(open, "ul" | "ol" | "menu"),
            "dt" | "dd" => open == "dl",
            "tr" | "td" | "th" | "thead" | "tbody" | "tfoot" => open == "table",
            _ => false,
        };
        if CLOSES_P.contains(&tag) {
            if let Some(depth) = self.is_open("p") {
                if !self.open[depth..].iter().any(|&at| matches!(self.tree.nodes[at].tag.as_str(), "button" | "table")) {
                    self.close_to(depth);
                }
            }
        }
        if matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
            if let Some(&at) = self.open.last() {
                if matches!(self.tree.nodes[at].tag.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
                    self.open.pop();
                }
            }
        }
        for depth in (1..self.open.len()).rev() {
            let open = self.tree.nodes[self.open[depth]].tag.clone();
            if stops(&open) {
                break;
            }
            if ends(&open) {
                self.close_to(depth);
                break;
            }
        }
    }

    fn start(&mut self, tag: String, attrs: Vec<(String, String)>) {
        if tag == "meta" && self.tree.title.is_empty() && attrs.iter().any(|(key, value)| key == "property" && value == "og:title") {
            self.tree.title = attrs.iter().find(|(key, _)| key == "content").map(|(_, value)| value.clone()).unwrap_or_default();
        }
        if self.dropped > 0 {
            if !VOID.contains(&tag.as_str()) {
                self.dropped += 1;
            }
            return;
        }
        let hidden = attrs.iter().any(|(key, value)| key == "hidden" || key == "aria-hidden" && value == "true" || key == "style" && value.replace(' ', "").contains("display:none"));
        if DROPPED.contains(&tag.as_str()) || hidden {
            if !VOID.contains(&tag.as_str()) {
                self.dropped = 1;
                self.open.push(usize::MAX);
            }
            return;
        }
        self.imply_end(&tag);
        let parent = self.current();
        let is_void = VOID.contains(&tag.as_str());
        self.tree.nodes.push(Node { tag, attrs, text: String::new(), kids: Vec::new(), parent: Some(parent) });
        let at = self.tree.nodes.len() - 1;
        self.tree.nodes[parent].kids.push(at);
        if !is_void {
            self.open.push(at);
        }
    }

    fn end(&mut self, tag: &str) {
        if self.dropped > 0 {
            self.dropped -= 1;
            if self.dropped == 0 {
                if let Some(depth) = self.open.iter().rposition(|&at| at == usize::MAX) {
                    self.open.truncate(depth);
                }
            }
            return;
        }
        if let Some(depth) = self.open.iter().rposition(|&at| at != usize::MAX && self.tree.nodes[at].tag == tag) {
            if depth > 0 {
                self.close_to(depth);
            }
        }
    }

    fn text(&mut self, text: &str) {
        if self.dropped > 0 || text.is_empty() {
            return;
        }
        let parent = self.current();
        if let Some(&last) = self.tree.nodes[parent].kids.last() {
            if self.tree.nodes[last].tag.is_empty() {
                self.tree.nodes[last].text.push_str(&decode(text));
                return;
            }
        }
        self.tree.nodes.push(Node { tag: String::new(), attrs: Vec::new(), text: decode(text), kids: Vec::new(), parent: Some(parent) });
        let at = self.tree.nodes.len() - 1;
        self.tree.nodes[parent].kids.push(at);
    }
}

// A lenient reading of HTML into a tree: tags, attributes, comments and raw-text elements, with the end tags HTML
// lets a page leave out. What it does not understand it reads as text.
fn parse(html: &str) -> Tree {
    let root = Node { tag: "#root".into(), attrs: Vec::new(), text: String::new(), kids: Vec::new(), parent: None };
    let mut builder = Builder { tree: Tree { nodes: vec![root], title: String::new() }, open: vec![0], dropped: 0 };
    let lower = html.to_ascii_lowercase();
    let mut at = 0;
    while at < html.len() {
        let Some(found) = html[at..].find('<') else {
            builder.text(&html[at..]);
            break;
        };
        let lt = at + found;
        builder.text(&html[at..lt]);
        let rest = &html[lt..];
        if rest.starts_with("<!--") {
            at = rest[4..].find("-->").map_or(html.len(), |end| lt + 4 + end + 3);
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            at = rest.find('>').map_or(html.len(), |end| lt + end + 1);
            continue;
        }
        let is_end = rest.starts_with("</");
        let name_from = lt + if is_end { 2 } else { 1 };
        let name_len = html[name_from..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == ':')).unwrap_or(html.len() - name_from);
        if name_len == 0 || !html[name_from..].starts_with(|c: char| c.is_ascii_alphabetic()) {
            builder.text("<");
            at = lt + 1;
            continue;
        }
        let tag = html[name_from..name_from + name_len].to_ascii_lowercase();
        // The tag ends at the first `>` outside quotes.
        let mut quote = None;
        let mut close = None;
        for (n, c) in html[name_from + name_len..].char_indices() {
            match (quote, c) {
                (None, '"' | '\'') => quote = Some(c),
                (Some(open), c) if c == open => quote = None,
                (None, '>') => {
                    close = Some(name_from + name_len + n);
                    break;
                }
                _ => {}
            }
        }
        let Some(close) = close else {
            builder.text(rest);
            break;
        };
        at = close + 1;
        if is_end {
            builder.end(&tag);
            continue;
        }
        let attrs = attributes(&html[name_from + name_len..close]);
        let self_closing = html[..close].ends_with('/');
        if RAW.contains(&tag.as_str()) {
            let end = lower[at..].find(&format!("</{tag}")).map_or(html.len(), |n| at + n);
            let content = &html[at..end];
            if tag == "title" {
                if builder.tree.title.is_empty() {
                    builder.tree.title = decode(content).split_whitespace().collect::<Vec<_>>().join(" ");
                }
            } else if tag == "textarea" || tag == "xmp" {
                builder.start(tag.clone(), attrs);
                builder.text(content);
                builder.end(&tag);
            }
            at = html[end..].find('>').map_or(html.len(), |n| end + n + 1);
            continue;
        }
        builder.start(tag.clone(), attrs);
        if self_closing && !VOID.contains(&tag.as_str()) {
            builder.end(&tag);
        }
    }

    builder.tree
}

// The body of an HTTP response `curl -i` printed, or the text itself.
fn split_headers(text: &str) -> (Option<&str>, &str) {
    if !text.starts_with("HTTP/") {
        return (None, text);
    }
    let end = text.find("\r\n\r\n").map(|at| (at, 4)).or_else(|| text.find("\n\n").map(|at| (at, 2)));
    match end {
        Some((at, skip)) => (text.lines().next(), &text[at + skip..]),
        None => (None, text),
    }
}

// Whether `text` is a web page rather than an output that mentions HTML.
pub fn is_html(text: &str) -> bool {
    let (_, body) = split_headers(text.trim_start_matches('\u{feff}'));
    let head: String = body.trim_start().chars().take(2048).collect::<String>().to_ascii_lowercase();

    head.starts_with("<!doctype html") || head.starts_with("<html") || head.starts_with("<?xml") && head.contains("<html") || head.starts_with("<head") || head.starts_with("<!--") && head.contains("<html")
}

fn tag_weight(tag: &str) -> f64 {
    match tag {
        "div" => 5.0,
        "pre" | "td" | "blockquote" => 3.0,
        "address" | "ol" | "ul" | "dl" | "dd" | "dt" | "li" | "form" => -3.0,
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "th" => -5.0,
        _ => 0.0,
    }
}

fn class_weight(tree: &Tree, at: usize) -> f64 {
    let names = tree.class_and_id(at);
    let mut weight = 0.0;
    if NEGATIVE.is_match(&names).unwrap_or(false) {
        weight -= 25.0;
    }
    if POSITIVE.is_match(&names).unwrap_or(false) {
        weight += 25.0;
    }

    weight
}

// Readability's choice of the main content: paragraphs score their parents, the best-scoring container wins,
// and its siblings that read like more of it join it.
fn main_content(tree: &Tree) -> Option<Vec<usize>> {
    let is_chrome = |at: usize| {
        let node = &tree.nodes[at];
        let names = tree.class_and_id(at);
        CHROME.contains(&node.tag.as_str()) || tree.attr(at, "role").is_some_and(|role| matches!(role, "navigation" | "banner" | "contentinfo" | "complementary" | "dialog")) || node.tag != "body" && node.tag != "html" && UNLIKELY.is_match(&names).unwrap_or(false) && !MAYBE.is_match(&names).unwrap_or(false)
    };
    let mut skipped = vec![false; tree.nodes.len()];
    tree.each(0, &mut |tree, at| {
        if !tree.nodes[at].tag.is_empty() && is_chrome(at) {
            let mut stack = vec![at];
            while let Some(node) = stack.pop() {
                skipped[node] = true;
                stack.extend(&tree.nodes[node].kids);
            }
            return false;
        }
        true
    });
    // A page that marks its main content says so: one <main>, one <article>, or role=main, holding real text.
    for marked in [tree.find("main"), tree.find("article"), (0..tree.nodes.len()).filter(|&at| tree.attr(at, "role") == Some("main")).collect()] {
        if marked.len() == 1 && tree.text_len(marked[0]) >= LEAST_ARTICLE_CHARS {
            return Some(marked);
        }
    }
    let mut scores: HashMap<usize, f64> = HashMap::new();
    for at in 0..tree.nodes.len() {
        let node = &tree.nodes[at];
        if skipped[at] || !matches!(node.tag.as_str(), "p" | "pre" | "td" | "blockquote") {
            continue;
        }
        let mut text = String::new();
        tree.text_of(at, &mut text);
        let length = text.trim().chars().count();
        if length < 25 {
            continue;
        }
        let score = 1.0 + text.matches([',', '，']).count() as f64 + (length as f64 / 100.0).min(3.0);
        let mut ancestor = node.parent;
        for level in 0..3 {
            let Some(up) = ancestor.filter(|&up| up != 0 && !tree.nodes[up].tag.starts_with('#')) else { break };
            let entry = scores.entry(up).or_insert_with(|| tag_weight(&tree.nodes[up].tag) + class_weight(tree, up));
            *entry += match level {
                0 => score,
                1 => score / 2.0,
                _ => score / 6.0,
            };
            ancestor = tree.nodes[up].parent;
        }
    }
    let (top, best) = scores.iter().map(|(&at, &score)| (at, score * (1.0 - tree.link_density(at)))).max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))?;
    let Some(parent) = tree.nodes[top].parent else { return Some(vec![top]) };
    let threshold = (best * 0.2).max(10.0);
    let mut picked = Vec::new();
    for &sibling in &tree.nodes[parent].kids {
        if sibling == top {
            picked.push(sibling);
            continue;
        }
        if tree.nodes[sibling].tag.is_empty() || skipped[sibling] {
            continue;
        }
        let bonus = if !tree.attr(top, "class").unwrap_or("").is_empty() && tree.attr(sibling, "class") == tree.attr(top, "class") { best * 0.2 } else { 0.0 };
        let score = scores.get(&sibling).map_or(0.0, |score| score * (1.0 - tree.link_density(sibling))) + bonus;
        let reads_like_text = tree.nodes[sibling].tag == "p" && {
            let (length, density) = (tree.text_len(sibling), tree.link_density(sibling));
            length > 80 && density < 0.25 || length > 0 && length <= 80 && density == 0.0
        };
        if score >= threshold || reads_like_text {
            picked.push(sibling);
        }
    }

    Some(picked)
}

// ---- Turndown: elements to Markdown ------------------------------------------------------------

struct Writer<'a> {
    tree: &'a Tree,
    base: Option<&'a str>,
    // The main content's own elements, which its cleaning leaves alone; empty for a whole page, which is not cleaned.
    roots: Vec<usize>,
}

// Marks the writer leaves for its last pass, which sees whole lines: where a text's first character would read as
// Markdown at the start of a line, a hard line break, and indentation a list or quote added, which no trimming removes.
const ESCAPE: char = '\u{1}';
const BREAK: char = '\u{2}';
const PAD: char = '\u{3}';

fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    if space {
        out.push(' ');
    }

    out
}

// html-to-markdown's smart escaping: a backslash only where the text would otherwise read as Markdown. `*` and a
// backtick always; `_` where it could open or close emphasis, not inside `snake_case`; and the first character of a
// line that would start a heading, quote, list, rule or setext underline, decided once lines are known.
fn escape_text(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 4);
    let first = chars.iter().position(|c| *c != ' ');
    for (n, &c) in chars.iter().enumerate() {
        if Some(n) == first {
            let digits = chars[n..].iter().take_while(|c| c.is_ascii_digit()).count();
            let numbered = digits > 0 && matches!(chars.get(n + digits), Some('.' | ')')) && chars.get(n + digits + 1).is_none_or(|next| *next == ' ');
            if matches!(c, '#' | '>' | '-' | '+' | '=') || numbered {
                out.push(ESCAPE);
            }
        }
        match c {
            '*' | '`' | '[' => {
                out.push('\\');
                out.push(c);
            }
            '\\' => out.push_str("\\\\"),
            '_' => {
                let word = |at: Option<&char>| at.is_some_and(|c| c.is_alphanumeric());
                if !(word(n.checked_sub(1).and_then(|before| chars.get(before))) && word(chars.get(n + 1))) {
                    out.push('\\');
                }
                out.push(c);
            }
            _ => out.push(c),
        }
    }

    out
}

// A block's own lines: indentation from the HTML and runs of spaces gone, at most one blank line in a row; code
// fences as they are.
fn normalize(content: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut in_fence = false;
    for line in content.lines() {
        let fence = line.trim_start_matches([' ', PAD]).starts_with("```");
        if in_fence || fence {
            if fence {
                in_fence = !in_fence;
            }
            lines.push(line.to_string());
            continue;
        }
        let mut text = String::with_capacity(line.len());
        let mut space = false;
        for c in line.trim_matches(' ').chars() {
            if c == ' ' {
                space = true;
                continue;
            }
            if space {
                text.push(' ');
            }
            space = false;
            text.push(c);
        }
        let is_blank = text.trim_matches([' ', PAD, BREAK]).is_empty();
        if is_blank && lines.last().is_none_or(|last| last.is_empty()) {
            continue;
        }
        lines.push(if is_blank { String::new() } else { text });
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }

    lines.join("\n")
}

fn block(content: &str) -> String {
    let content = normalize(content);

    if content.is_empty() { String::new() } else { format!("\n\n{content}\n\n") }
}

fn indent(content: &str, first: &str) -> String {
    let pad: String = std::iter::repeat_n(PAD, first.chars().count()).collect();
    let mut out = String::new();
    for (n, line) in content.lines().enumerate() {
        if n == 0 {
            out.push_str(first);
        } else if !line.is_empty() {
            out.push_str(&pad);
        }
        out.push_str(line);
        out.push('\n');
    }

    out
}

fn longest_ticks(code: &str) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for c in code.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }

    longest
}

fn fence_for(code: &str) -> String {
    "`".repeat((longest_ticks(code) + 1).max(3))
}

// Rows of a table as a grid: a cell spanning columns or rows leaves empty cells where it reaches, as html-to-markdown
// writes them.
fn grid(rows: Vec<Vec<(String, usize, usize)>>) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    // Per column, how many rows below are still taken by a cell above.
    let mut taken: Vec<usize> = Vec::new();
    for row in rows {
        let mut line = Vec::new();
        let mut column = 0;
        let mut cells = row.into_iter();
        let mut next = cells.next();
        while next.is_some() || taken.get(column).is_some_and(|rows| *rows > 0) {
            if taken.get(column).is_some_and(|rows| *rows > 0) {
                taken[column] -= 1;
                line.push(String::new());
                column += 1;
                continue;
            }
            let Some((text, cols, rows)) = next.take() else { break };
            for span in 0..cols.max(1) {
                line.push(if span == 0 { text.clone() } else { String::new() });
                if taken.len() <= column {
                    taken.resize(column + 1, 0);
                }
                taken[column] = rows.max(1) - 1;
                column += 1;
            }
            next = cells.next();
        }
        out.push(line);
    }

    out
}

impl Writer<'_> {
    fn href(&self, link: &str) -> String {
        let link = link.trim().replace('\n', "%0A").replace('\r', "").replace('\t', "%09");
        let link = link.as_str();
        let Some(base) = self.base else { return link.to_string() };
        if link.contains("://") || link.starts_with("mailto:") || link.starts_with("tel:") {
            return link.to_string();
        }
        let scheme_end = base.find("://").map_or(0, |at| at + 3);
        let origin_end = base[scheme_end..].find('/').map_or(base.len(), |at| scheme_end + at);
        if let Some(rest) = link.strip_prefix("//") {
            return format!("{}{rest}", &base[..scheme_end]);
        }
        if link.starts_with('/') {
            return format!("{}{link}", &base[..origin_end]);
        }
        let path_end = base.find(['?', '#']).unwrap_or(base.len());
        let dir_end = base[..path_end].rfind('/').filter(|&at| at >= origin_end).map_or(path_end, |at| at + 1);
        let dir = &base[..dir_end];

        if dir.ends_with('/') { format!("{dir}{link}") } else { format!("{dir}/{link}") }
    }

    // Readability's cleaning of the main content: what sits around content inside it (menus, language lists, share
    // bars), and lists, tables and blocks that are mostly links.
    fn is_clutter(&self, at: usize) -> bool {
        let tree = self.tree;
        let tag = tree.nodes[at].tag.as_str();
        let names = tree.class_and_id(at);
        if CHROME.contains(&tag) || tree.attr(at, "role").is_some_and(|role| matches!(role, "navigation" | "banner" | "contentinfo" | "complementary" | "dialog")) {
            return true;
        }
        if !matches!(tag, "div" | "section" | "ul" | "ol" | "table" | "dl" | "span" | "p") {
            return false;
        }
        if UNLIKELY.is_match(&names).unwrap_or(false) && !MAYBE.is_match(&names).unwrap_or(false) {
            return true;
        }
        if tag == "p" || tag == "span" {
            return false;
        }
        let mut text = String::new();
        tree.text_of(at, &mut text);
        let commas = text.matches(',').count();

        commas < 10 && (class_weight(tree, at) < 0.0 || tree.link_density(at) > 0.5)
    }

    fn kids(&self, at: usize, pre: bool) -> String {
        self.tree.nodes[at].kids.iter().map(|&kid| self.node(kid, pre)).collect()
    }

    fn text(&self, at: usize) -> String {
        let mut text = String::new();
        self.tree.text_of(at, &mut text);

        text
    }

    fn list(&self, at: usize, ordered: bool) -> String {
        let start = self.tree.attr(at, "start").and_then(|start| start.trim().parse::<usize>().ok()).unwrap_or(1);
        let mut out = String::new();
        let mut n = start;
        for &kid in &self.tree.nodes[at].kids {
            if self.tree.nodes[kid].tag != "li" {
                continue;
            }
            let marker = if ordered { format!("{n}. ") } else { "- ".to_string() };
            n += 1;
            let content = normalize(&self.kids(kid, false));
            // A list inside an item follows its line, with no blank line between, as Turndown writes it.
            let lines: Vec<&str> = content.lines().collect();
            let content: Vec<&str> = lines.iter().enumerate().filter(|(n, line)| !line.is_empty() || !lines.get(n + 1).is_some_and(|next| is_list_line(next))).map(|(_, line)| *line).collect();
            let content = content.join("\n");
            out.push_str(&indent(if content.is_empty() { " " } else { &content }, &marker));
        }

        block(&out)
    }

    fn table(&self, at: usize) -> String {
        // A table inside a link, emphasis or a heading is read as its text.
        let mut up = self.tree.nodes[at].parent;
        while let Some(parent) = up {
            if matches!(self.tree.nodes[parent].tag.as_str(), "a" | "b" | "strong" | "em" | "i" | "button" | "span" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
                return format!(" {} ", escape_text(collapse(&self.text(at)).trim()));
            }
            up = self.tree.nodes[parent].parent;
        }
        // A table used for layout (one column, or cells holding blocks or tables) is read as its contents.
        let mut is_layout = false;
        let mut columns = 0;
        self.tree.each(at, &mut |tree, node| {
            let tag = tree.nodes[node].tag.as_str();
            if node != at && matches!(tag, "table" | "p" | "div" | "ul" | "ol" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "pre") {
                is_layout = true;
            }
            if tag == "tr" {
                columns = columns.max(tree.nodes[node].kids.iter().filter(|&&cell| matches!(tree.nodes[cell].tag.as_str(), "td" | "th")).count());
            }
            true
        });
        if is_layout || columns == 1 {
            let mut out = String::new();
            self.tree.each(at, &mut |tree, node| {
                if matches!(tree.nodes[node].tag.as_str(), "td" | "th" | "caption") {
                    out.push_str(&block(&self.kids(node, false)));
                    return false;
                }
                true
            });
            return out;
        }
        let mut rows: Vec<Vec<(String, usize, usize)>> = Vec::new();
        let mut caption = String::new();
        let mut header_row = false;
        self.tree.each(at, &mut |tree, node| {
            match tree.nodes[node].tag.as_str() {
                "caption" => {
                    caption = normalize(&self.kids(node, false)).replace('\n', " ");
                    false
                }
                "tr" => {
                    let cells: Vec<usize> = tree.nodes[node].kids.iter().copied().filter(|&cell| matches!(tree.nodes[cell].tag.as_str(), "td" | "th")).collect();
                    if rows.is_empty() {
                        header_row = !cells.is_empty() && cells.iter().all(|&cell| tree.nodes[cell].tag == "th") || tree.nodes[node].parent.is_some_and(|up| tree.nodes[up].tag == "thead");
                    }
                    let span = |cell: usize, name: &str| tree.attr(cell, name).and_then(|n| n.trim().parse::<usize>().ok()).unwrap_or(1).min(100);
                    rows.push(cells.iter().map(|&cell| (normalize(&self.kids(cell, false)).replace(BREAK, "").replace('\n', " ").replace('|', "\\|"), span(cell, "colspan"), span(cell, "rowspan"))).collect());
                    false
                }
                _ => node == at || tree.nodes[node].tag != "table",
            }
        });
        let mut grid = grid(rows);
        let width = grid.iter().map(Vec::len).max().unwrap_or(0);
        if width == 0 || grid.iter().all(|row| row.iter().all(String::is_empty)) {
            return block(&caption);
        }
        // Markdown needs a header row: an empty one where the table has none.
        if !header_row {
            grid.insert(0, Vec::new());
        }
        let mut out = String::new();
        if !caption.is_empty() {
            out.push_str(&caption);
            out.push_str("\n\n");
        }
        for (n, row) in grid.iter().enumerate() {
            let cells: Vec<&str> = (0..width).map(|cell| row.get(cell).map_or("", String::as_str)).collect();
            out.push_str(&format!("| {} |\n", cells.join(" | ")));
            if n == 0 {
                out.push_str(&format!("|{}\n", " --- |".repeat(width)));
            }
        }

        block(&out)
    }

    fn node(&self, at: usize, pre: bool) -> String {
        let node = &self.tree.nodes[at];
        if node.tag.is_empty() {
            return if pre { node.text.clone() } else { escape_text(&collapse(&node.text)) };
        }
        if !pre && !self.roots.is_empty() && !self.roots.contains(&at) && self.is_clutter(at) {
            return String::new();
        }
        let inner = || self.kids(at, pre);
        match node.tag.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = node.tag[1..].parse::<usize>().unwrap_or(1);
                // A `#` at either end of a heading's text would be read as part of the marker.
                let text = normalize(&inner()).replace(['\n', BREAK], " ").replace(ESCAPE, "");
                let text = text.trim();
                let text = if text.starts_with('#') { format!("\\{text}") } else { text.to_string() };
                let text = if text.ends_with('#') && !text.ends_with("\\#") { format!("{}\\#", &text[..text.len() - 1]) } else { text };
                if text.is_empty() { String::new() } else { format!("\n\n{} {text}\n\n", "#".repeat(level)) }
            }
            "p" | "div" | "section" | "article" | "main" | "header" | "footer" | "nav" | "aside" | "figure" | "figcaption" | "address" | "details" | "summary" | "center" | "fieldset" | "body" | "html" => block(&inner()),
            "br" => format!("{BREAK}\n"),
            "hr" => "\n\n* * *\n\n".to_string(),
            "strong" | "b" => wrap(&inner(), "**"),
            "em" | "i" | "cite" => wrap(&inner(), "*"),
            "del" | "s" | "strike" => wrap(&inner(), "~~"),
            "code" | "kbd" | "samp" | "tt" if !pre => {
                let code = self.text(at);
                let code = code.replace('\n', " ");
                if code.trim().is_empty() {
                    return String::new();
                }
                let fence = "`".repeat(longest_ticks(&code) + 1);
                let pad = if code.starts_with('`') || code.ends_with('`') { " " } else { "" };
                format!("{fence}{pad}{code}{pad}{fence}")
            }
            "pre" => {
                let code = self.text(at);
                let code = code.trim_end_matches(['\n', ' ']).trim_start_matches('\n');
                let class = node.kids.iter().find(|&&kid| self.tree.nodes[kid].tag == "code").and_then(|&kid| self.tree.attr(kid, "class")).or_else(|| self.tree.attr(at, "class")).unwrap_or("");
                let language = class.split_whitespace().find_map(|name| name.strip_prefix("language-").or_else(|| name.strip_prefix("lang-"))).unwrap_or("");
                let fence = fence_for(code);
                format!("\n\n{fence}{language}\n{code}\n{fence}\n\n")
            }
            "a" => {
                let text = normalize(&inner()).replace('\n', " ").replace(ESCAPE, "");
                let text = text.trim();
                let href = node.attrs.iter().find(|(key, _)| key == "href").map(|(_, value)| value.as_str()).unwrap_or("");
                if text.is_empty() {
                    String::new()
                } else if href.is_empty() || href.starts_with('#') || href.starts_with("javascript:") {
                    text.to_string()
                } else {
                    let title = self.tree.attr(at, "title").map(|title| collapse(title).trim().replace('"', "\\\"")).filter(|title| !title.is_empty());
                    let title = title.map_or(String::new(), |title| format!(" \"{title}\""));
                    format!("[{}]({}{title})", text.replace('[', "\\[").replace(']', "\\]"), self.href(href).replace(' ', "%20").replace('(', "%28").replace(')', "%29"))
                }
            }
            "img" => {
                let alt = collapse(self.tree.attr(at, "alt").unwrap_or("")).trim().replace(['[', ']'], "");
                let src = self.tree.attr(at, "src").unwrap_or("");
                // Icons and badges (at most 32 pixels a side) say nothing a reader needs.
                let is_icon = ["width", "height"].iter().any(|side| self.tree.attr(at, side).and_then(|size| size.trim_end_matches("px").parse::<u32>().ok()).is_some_and(|size| size <= 32));
                if is_icon {
                    return String::new();
                }
                if src.is_empty() || src.starts_with("data:") {
                    if alt.is_empty() { String::new() } else { format!("[image: {alt}]") }
                } else {
                    let title = self.tree.attr(at, "title").map(|title| collapse(title).trim().replace('"', "\\\"")).filter(|title| !title.is_empty());
                    let title = title.map_or(String::new(), |title| format!(" \"{title}\""));
                    format!("![{alt}]({}{title})", self.href(src).replace(' ', "%20").replace('(', "%28").replace(')', "%29"))
                }
            }
            "ul" | "menu" => self.list(at, false),
            "ol" => self.list(at, true),
            "li" => block(&inner()),
            "blockquote" => {
                let content = normalize(&inner());
                let quoted: Vec<String> = content.lines().map(|line| if line.is_empty() { ">".to_string() } else { format!(">{PAD}{line}") }).collect();
                block(&quoted.join("\n"))
            }
            "table" => self.table(at),
            "caption" => String::new(),
            "dt" => block(&wrap(&inner(), "**")),
            "dd" => block(&inner()),
            "dl" => block(&inner()),
            "button" | "select" | "input" | "textarea" | "label" | "option" => String::new(),
            _ => inner(),
        }
    }
}

fn is_list_line(line: &str) -> bool {
    let line = line.trim_start_matches([' ', PAD]);
    let digits = line.chars().take_while(char::is_ascii_digit).count();

    line.starts_with("- ") || digits > 0 && line[digits..].starts_with(". ")
}

fn wrap(text: &str, mark: &str) -> String {
    let trimmed = text.trim_matches([' ', '\n']);
    if trimmed.is_empty() {
        return text.to_string();
    }
    // `<b><strong>x</strong></b>` is bold once.
    if trimmed.len() > 2 * mark.len() && trimmed.starts_with(mark) && trimmed.ends_with(mark) && !trimmed[mark.len()..].starts_with('*') {
        return text.to_string();
    }
    let lead = if text.starts_with([' ', '\n']) { " " } else { "" };
    let tail = if text.ends_with([' ', '\n']) { " " } else { "" };

    format!("{lead}{mark}{trimmed}{mark}{tail}")
}

// The last pass, over whole lines: a marked first character is escaped where it does start a line's content and
// left alone elsewhere, adjacent bold runs merge (`**a****b**` is `**ab**`), hard breaks are two trailing spaces, and
// added indentation becomes spaces.
fn finish(markdown: &str) -> String {
    let mut out = Vec::new();
    let mut in_fence = false;
    for line in normalize(markdown).lines() {
        if line.trim_start_matches([' ', PAD]).starts_with("```") {
            in_fence = !in_fence;
            out.push(line.replace(PAD, " "));
            continue;
        }
        if in_fence {
            out.push(line.replace(PAD, " "));
            continue;
        }
        let mut text = String::with_capacity(line.len());
        let mut at_start = true;
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                ESCAPE => {
                    if at_start {
                        let digits: String = chars.clone().take_while(char::is_ascii_digit).collect();
                        text.push_str(&digits);
                        for _ in 0..digits.len() {
                            chars.next();
                        }
                        // A number is escaped at its dot: `1\. not a list`.
                        text.push('\\');
                        at_start = digits.is_empty();
                    }
                }
                BREAK => {}
                PAD | ' ' => text.push(' '),
                '>' if at_start => text.push(c),
                _ => {
                    // A list marker keeps the start of the content after it.
                    let marker = at_start && (c == '-' && chars.peek() == Some(&' ') || c.is_ascii_digit() && {
                        let rest: String = chars.clone().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
                        rest.ends_with('.') && chars.clone().nth(rest.len()) == Some(' ')
                    });
                    text.push(c);
                    if marker {
                        while let Some(&next) = chars.peek() {
                            if next == ' ' || next.is_ascii_digit() || next == '.' {
                                text.push(next);
                                chars.next();
                                if next == ' ' {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                        continue;
                    }
                    at_start = false;
                }
            }
        }
        let text = text.replace("****", "").trim_end().to_string();
        out.push(if line.ends_with(BREAK) && !text.is_empty() { format!("{text}  ") } else { text });
    }
    while out.last().is_some_and(|line: &String| line.trim().is_empty()) {
        out.pop();
    }

    out.join("\n").trim_start_matches('\n').to_string()
}

// `text`, a web page (with the headers `curl -i` prints, or none), as Markdown; None when it is no web page.
pub fn convert(text: &str, base: Option<&str>) -> Option<Page> {
    if !is_html(text) || text.len() > MOST_HTML_BYTES {
        return None;
    }
    let (status, body) = split_headers(text.trim_start_matches('\u{feff}'));
    let tree = parse(body);
    let mut writer = Writer { tree: &tree, base, roots: Vec::new() };
    let body_at = tree.find("body").first().copied().unwrap_or(0);
    let whole_text = tree.text_len(body_at);
    let article = main_content(&tree).filter(|picked| {
        let length: usize = picked.iter().map(|&at| tree.text_len(at)).sum();
        length >= LEAST_ARTICLE_CHARS.min(whole_text / 2)
    });
    let whole = article.is_none();
    writer.roots = article.clone().unwrap_or_default();
    let markdown: String = match &article {
        Some(picked) => picked.iter().map(|&at| writer.node(at, false)).collect(),
        None => writer.node(body_at, false),
    };
    let mut markdown = finish(&markdown);
    if let Some(status) = status.filter(|line| !line.contains(" 200")) {
        markdown = format!("{}\n\n{markdown}", status.trim());
    }

    Some(Page { title: tree.title.clone(), markdown, whole })
}

// The whole of `html` as Markdown, with no choice of main content: the converter alone, as html-to-markdown is.
pub fn convert_all(html: &str, base: Option<&str>) -> String {
    let tree = parse(html);
    let writer = Writer { tree: &tree, base, roots: Vec::new() };

    finish(&writer.node(0, false))
}

// What the model reads: the title and where the page came from, then the page.
pub fn render(page: &Page, base: Option<&str>) -> String {
    let mut head = String::new();
    if !page.title.is_empty() {
        head.push_str(&format!("Title: {}\n", page.title));
    }
    if let Some(base) = base {
        head.push_str(&format!("URL: {base}\n"));
    }
    if !head.is_empty() {
        head.push('\n');
    }

    format!("{head}{}\n", page.markdown)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTICLE: &str = r#"<!DOCTYPE html><html><head><title>The dam  report</title><style>p{}</style><script>var x = 1;</script></head>
<body><header class="site-header"><nav><a href="/">Home</a> <a href="/blog">Blog</a></nav></header>
<div class="sidebar"><ul><li><a href="/a">Related one</a></li><li><a href="/b">Related two</a></li></ul></div>
<div class="post-content">
  <h1>Beavers build dams</h1>
  <p>Beavers cut trees with their teeth, drag the logs to the stream, and pile them up with mud and stones, which slows the water.</p>
  <p>A family keeps one dam in repair for years, and a pond forms behind it, where they build their lodge, store food, and stay safe.</p>
  <h2>How long it <em>takes</em></h2>
  <ul><li>A small dam: a few <strong>nights</strong>.</li><li>A large one: several <a href="seasons.html">seasons</a>.</li></ul>
  <pre><code class="language-rust">fn main() {
    println!("dam");
}</code></pre>
  <table><tr><th>Dam</th><th>Length</th></tr><tr><td>Small</td><td>2 m</td></tr><tr><td>Wood Buffalo</td><td>850 m</td></tr></table>
  <p>Use <code>dam-squeeze</code> to <img src="data:image/png;base64,AAAA" alt="">read logs, and see <img src="/img/beaver.png" alt="a beaver">.</p>
</div>
<footer class="footer"><p>Copyright, all rights, reserved, by nobody at all, ever, anywhere.</p></footer></body></html>"#;

    #[test]
    fn the_main_content_comes_out_as_markdown() {
        let page = convert(ARTICLE, Some("https://example.com/blog/dams/index.html")).expect("a page");
        assert_eq!(page.title, "The dam report");
        assert!(!page.whole);
        let md = &page.markdown;
        for wanted in ["# Beavers build dams", "## How long it *takes*", "- A small dam: a few **nights**.", "[seasons](https://example.com/blog/dams/seasons.html)", "```rust\nfn main() {\n    println!(\"dam\");\n}\n```", "| Dam | Length |\n| --- | --- |\n| Small | 2 m |", "`dam-squeeze`", "![a beaver](https://example.com/img/beaver.png)"] {
            assert!(md.contains(wanted), "missing {wanted:?} in\n{md}");
        }
        for unwanted in ["Home", "Related one", "Copyright", "var x", "p{}", "data:image"] {
            assert!(!md.contains(unwanted), "kept {unwanted:?} in\n{md}");
        }
    }

    #[test]
    fn a_page_with_no_main_content_is_kept_whole_and_other_text_is_left_alone() {
        let page = convert("<html><body><div><a href='/x'>Sign in</a></div><div>Short note.</div><script>evil()</script></body></html>", None).expect("a page");
        assert!(page.whole);
        assert!(page.markdown.contains("[Sign in](/x)") && page.markdown.contains("Short note.") && !page.markdown.contains("evil"), "{}", page.markdown);
        assert_eq!(convert("error: <html> tag expected at line 3", None), None);
        assert_eq!(convert("{\"html\": \"<p>x</p>\"}", None), None);
    }

    #[test]
    fn curl_headers_are_read_past_and_a_failure_status_kept() {
        let text = "HTTP/1.1 404 Not Found\r\nContent-Type: text/html\r\n\r\n<!doctype html><html><body><p>Gone.</p></body></html>";
        assert!(is_html(text));
        let page = convert(text, None).expect("a page");
        assert!(page.markdown.starts_with("HTTP/1.1 404 Not Found"), "{}", page.markdown);
        assert!(page.markdown.contains("Gone."));
    }

    // Rules taken from html-to-markdown (MIT): smart escaping, hard breaks, spans and captions, bold once.
    #[test]
    fn escaping_breaks_and_tables_follow_html_to_markdown() {
        let md = convert_all(r"<p># not a heading</p><p>1. not a list, but 3.1 stays</p><p>fake **bold**, snake_case, _x_ and \ [a]</p><p>Line A<br>Line B</p><p><b><strong>bold</strong></b> <b>a</b><b>b</b></p>", None);
        for wanted in [r"\# not a heading", r"1\. not a list, but 3.1 stays", r"fake \*\*bold\*\*, snake_case, \_x\_ and \\ \[a]", "Line A  \nLine B", "**bold** **ab**"] {
            assert!(md.contains(wanted), "missing {wanted:?} in\n{md}");
        }
        let table = convert_all("<table><caption>Sizes</caption><tr><td colspan=2>wide</td><td>B1</td></tr><tr><td>A2</td><td>B2</td><td>C2</td></tr></table>", None);
        assert_eq!(table, "Sizes\n\n| | | |\n| --- | --- | --- |\n| wide | | B1 |\n| A2 | B2 | C2 |");
        let layout = convert_all("<table><tr><td><p>Hello</p><table><tr><td>A</td><td>B</td></tr></table></td></tr></table>", None);
        assert_eq!(layout, "Hello\n\n| | |\n| --- | --- |\n| A | B |");
    }

    #[test]
    fn nested_lists_and_quotes_keep_their_shape() {
        let page = convert("<html><body><ol start='3'><li>Three<ul><li>inner</li></ul></li><li>Four</li></ol><blockquote><p>Said once.</p><p>Said twice.</p></blockquote></body></html>", None).expect("a page");
        assert!(page.markdown.contains("3. Three\n   - inner\n4. Four"), "{}", page.markdown);
        assert!(page.markdown.contains("> Said once.\n>\n> Said twice."), "{}", page.markdown);
    }
}

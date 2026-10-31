//! Docs ↔ code sync checks.
//!
//! The markdown docs in this repo (`AGENTS.md`, which `CLAUDE.md` and
//! `GEMINI.md` import; `README.md`, `TESTS.md`, `PRESENTATION_FORMAT.md` and
//! the bs-deck skill) are load-bearing: AI agents act on what they say. These
//! tests mechanically enforce the sync rules the docs used to state only as
//! prose, so drift shows up as a `cargo test` failure at the edit that caused
//! it instead of as a misled future agent. Every assertion message says exactly which doc to fix.
//!
//! What is enforced:
//! - every test function is listed in `TESTS.md`, and `TESTS.md` names no
//!   test that does not exist;
//! - the test totals stated in `TESTS.md` and `AGENTS.md` match the code;
//! - repo paths mentioned in the docs exist;
//! - the object-type lists in `AGENTS.md`, `PRESENTATION_FORMAT.md`, and
//!   the bs-deck skill match the `SceneObject` enum;
//! - every complete JSON example in the docs parses (and, for full decks,
//!   compiles) with the current serde structs;
//! - `CLAUDE.md` and `GEMINI.md` are bare `@AGENTS.md` imports, so the agent
//!   guide has exactly one copy;
//! - `AGENTS.md`'s module map covers every core module file;
//! - every `§N` and `#anchor` reference in `PRESENTATION_FORMAT.md` names an
//!   existing heading.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use bs::engine::{
    source::{SceneObject, SourcePresentation},
    Engine,
};
use bs::renderer::Renderer;
use bs::types::TerminalContract;

/// All markdown docs that make claims about the code.
const DOCS: &[&str] = &[
    "AGENTS.md",
    "README.md",
    "TESTS.md",
    "PRESENTATION_FORMAT.md",
    // The packaged Claude skill for authoring decks. It restates the source
    // format, so it rots exactly like the rest of the docs — hold it to the
    // same machine-checked standard.
    ".claude/skills/bs-deck/SKILL.md",
];

/// Tool-specific entry points that must only import `AGENTS.md` (Claude Code
/// and Gemini CLI both expand an `@path` line), never carry their own copy.
const AGENT_DOC_STUBS: &[&str] = &["CLAUDE.md", "GEMINI.md"];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_repo_file(rel: &str) -> String {
    let path = repo_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Recursively collect every `.rs` file under `dir` (paths relative to root).
fn rs_files_under(rel_dir: &str) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}")) {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&repo_root().join(rel_dir), &mut out);
    out.sort();
    out
}

/// Extract the names of all test functions in a source file: a function whose
/// preceding attributes include the test attribute. (Textual scan; keep the
/// raw attribute out of strings/comments in scanned files so it can't
/// over-count — this file builds the pattern at runtime for that reason.)
fn test_fn_names(source: &str) -> Vec<String> {
    let test_attr = format!("#[{}]", "test");
    let mut names = Vec::new();
    let mut in_test_attrs = false;
    for line in source.lines() {
        let t = line.trim();
        if t.starts_with(&test_attr) {
            in_test_attrs = true;
            continue;
        }
        if !in_test_attrs {
            continue;
        }
        if t.starts_with("#[") || t.is_empty() || t.starts_with("//") {
            continue; // further attributes / noise between the attr and the fn
        }
        if let Some(rest) = t.strip_prefix("fn ") {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            assert!(!name.is_empty(), "malformed test fn line: {line}");
            names.push(name);
        }
        in_test_attrs = false;
    }
    names
}

/// (file rel-path, test name) for every integration test under `tests/`.
fn integration_tests() -> Vec<(String, String)> {
    collect_tests("tests")
}

/// (file rel-path, test name) for every inline unit test under `src/`.
fn inline_tests() -> Vec<(String, String)> {
    collect_tests("src")
}

fn collect_tests(rel_dir: &str) -> Vec<(String, String)> {
    let root = repo_root();
    let mut out = Vec::new();
    for path in rs_files_under(rel_dir) {
        let rel = path.strip_prefix(&root).unwrap().to_string_lossy().into_owned();
        let source = fs::read_to_string(&path).unwrap();
        for name in test_fn_names(&source) {
            out.push((rel.clone(), name));
        }
    }
    out
}

/// The test names `TESTS.md` claims to exist: the first backticked cell of
/// every row in a `| Test | Verifies |` table.
fn tests_md_claimed_names(tests_md: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_test_table = false;
    for line in tests_md.lines() {
        let t = line.trim();
        if t.starts_with("| Test |") {
            in_test_table = true;
            continue;
        }
        if !t.starts_with('|') {
            in_test_table = false;
            continue;
        }
        if !in_test_table || t.starts_with("|-") || t.starts_with("| -") {
            continue;
        }
        if let Some(rest) = t.strip_prefix("| `") {
            if let Some(name) = rest.split('`').next() {
                if !name.is_empty()
                    && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                {
                    names.push(name.to_string());
                }
            }
        }
    }
    names
}

/// The `SceneObject` variant names, parsed from the enum in
/// `src/engine/source.rs`.
fn scene_object_variants() -> Vec<String> {
    let source = read_repo_file("src/engine/source.rs");
    let start = source
        .find("pub enum SceneObject {")
        .expect("SceneObject enum not found in src/engine/source.rs");
    let body = &source[start..];
    let mut variants = Vec::new();
    for line in body.lines().skip(1) {
        let t = line.trim();
        if t == "}" {
            break;
        }
        if let Some(name) = t.split('(').next() {
            let name = name.trim();
            if !name.is_empty() && name.chars().next().unwrap().is_uppercase() {
                variants.push(name.to_string());
            }
        }
    }
    assert!(!variants.is_empty(), "no SceneObject variants parsed");
    variants
}

/// serde's `rename_all = "snake_case"`: `HLine` → `h_line`.
fn snake_case(variant: &str) -> String {
    let mut out = String::new();
    for (i, c) in variant.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Every ```json fenced block in `text` (handles blocks inside `>` quotes).
/// Returns (1-based start line, block content).
fn json_blocks(text: &str) -> Vec<(usize, String)> {
    let mut blocks = Vec::new();
    let mut lines = text.lines().enumerate();
    while let Some((i, line)) = lines.next() {
        let quoted = line.trim_start().starts_with('>');
        let fence = strip_quote(line);
        if fence.trim() != "```json" {
            continue;
        }
        let mut content = String::new();
        for (_, line) in lines.by_ref() {
            let l = if quoted { strip_quote(line) } else { line.to_string() };
            if l.trim_start().starts_with("```") {
                break;
            }
            content.push_str(&l);
            content.push('\n');
        }
        blocks.push((i + 1, content));
    }
    blocks
}

fn strip_quote(line: &str) -> String {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix("> ") {
        rest.to_string()
    } else if let Some(rest) = t.strip_prefix('>') {
        rest.to_string()
    } else {
        line.to_string()
    }
}

#[test]
fn every_test_function_is_listed_in_tests_md() {
    let tests_md = read_repo_file("TESTS.md");
    let mut missing = Vec::new();
    for (file, name) in integration_tests().into_iter().chain(inline_tests()) {
        if !tests_md.contains(&format!("`{name}`")) {
            missing.push(format!("{file}::{name}"));
        }
    }
    assert!(
        missing.is_empty(),
        "these tests are not listed in TESTS.md — add a `| \\`name\\` | what it verifies |` \
         row to the matching section (and update the totals at the top):\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn tests_md_names_no_test_that_does_not_exist() {
    let tests_md = read_repo_file("TESTS.md");
    let real: BTreeSet<String> = integration_tests()
        .into_iter()
        .chain(inline_tests())
        .map(|(_, name)| name)
        .collect();
    let phantom: Vec<String> = tests_md_claimed_names(&tests_md)
        .into_iter()
        .filter(|n| !real.contains(n))
        .collect();
    assert!(
        phantom.is_empty(),
        "TESTS.md lists tests that do not exist in the code — remove or rename these rows \
         (and update the totals at the top):\n  {}",
        phantom.join("\n  ")
    );
}

#[test]
fn documented_test_counts_match_the_code() {
    let integration = integration_tests().len();
    let inline = inline_tests().len();
    let total = integration + inline;

    let tests_md = read_repo_file("TESTS.md");
    let expected_tests_md = format!(
        "{total} tests: {integration} integration tests under `tests/` and\n{inline} inline unit tests"
    );
    assert!(
        tests_md.contains(&expected_tests_md),
        "TESTS.md's totals line is stale — the code has {total} tests \
         ({integration} integration + {inline} inline); update the paragraph at the top \
         to contain:\n  {expected_tests_md:?}"
    );

    let agents_md = read_repo_file("AGENTS.md");
    let expected_agents_md =
        format!("totals {total} tests ({integration} integration + {inline} inline");
    assert!(
        agents_md.contains(&expected_agents_md),
        "AGENTS.md's test-count sentence (in the Tests section) is stale — update it to \
         contain:\n  {expected_agents_md:?}"
    );
}

#[test]
fn doc_referenced_repo_paths_exist() {
    let prefixes = ["src/", "tests/", "docs/", "examples/", "scripts/"];
    let root = repo_root();
    let mut checked = 0usize;
    let mut broken = Vec::new();
    for doc in DOCS {
        let text = read_repo_file(doc);
        for (i, line) in text.lines().enumerate() {
            // Odd-indexed chunks after splitting on backticks are code spans.
            for (j, span) in line.split('`').enumerate() {
                if j % 2 == 0 {
                    continue;
                }
                let is_path = prefixes.iter().any(|p| span.starts_with(p));
                let is_literal =
                    !span.contains(['<', '>', '*', ' ', '…', '{']) && !span.contains("...");
                if is_path && is_literal {
                    checked += 1;
                    if !root.join(span).exists() {
                        broken.push(format!("{doc}:{}: `{span}`", i + 1));
                    }
                }
            }
        }
    }
    assert!(
        broken.is_empty(),
        "docs reference repo paths that do not exist — fix the doc (or restore the file):\n  {}",
        broken.join("\n  ")
    );
    // Guard against this check going vacuous if the extraction logic breaks:
    // the docs reference far more than 20 repo paths today.
    assert!(checked >= 20, "only {checked} doc paths were checked — the extractor is broken");
}

#[test]
fn object_type_docs_match_the_scene_object_enum() {
    let variants = scene_object_variants();
    let tags: Vec<String> = variants.iter().map(|v| snake_case(v)).collect();
    let n = variants.len();

    let agents_md = read_repo_file("AGENTS.md");
    let count_claim = format!("The {n} object types");
    assert!(
        agents_md.contains(&count_claim),
        "AGENTS.md's object-type count is stale — the SceneObject enum has {n} variants; \
         update the \"Conventions & landmines\" bullet to say {count_claim:?} and list every tag"
    );
    for tag in &tags {
        assert!(
            agents_md.contains(&format!("`{tag}`")),
            "AGENTS.md's object-type list is missing `{tag}` — update the \
             \"Conventions & landmines\" bullet"
        );
    }

    let format_md = read_repo_file("PRESENTATION_FORMAT.md");
    for tag in &tags {
        assert!(
            format_md.contains(&format!("| `{tag}` |")),
            "PRESENTATION_FORMAT.md's object catalog (§4) has no row for `{tag}` — \
             add it there and document the type in the matching section"
        );
    }

    let count_claim = format!("The {n} `SceneObject` types");
    assert!(
        agents_md.contains(&count_claim),
        "AGENTS.md's module-map row for src/engine/objects/ is stale — update it to say \
         {count_claim:?}"
    );
    for v in &variants {
        assert!(
            agents_md.contains(&format!("`{v}`")),
            "AGENTS.md never mentions the `{v}` object type — update the module map's \
             src/engine/objects/ row"
        );
    }

    // The packaged skill restates the catalog: a §3 row, a field-table section
    // headed by the tag, and the exhaustive tag list in §14 self-check item 3.
    let skill = ".claude/skills/bs-deck/SKILL.md";
    let skill_md = read_repo_file(skill);
    // Only §3 counts as the catalog: field tables elsewhere also have `| `tag` |` rows.
    let catalog = skill_md
        .split_once("## 3. Object catalog")
        .and_then(|(_, rest)| rest.split_once("\n## 4."))
        .map(|(cat, _)| cat)
        .unwrap_or_else(|| panic!("{skill}: §3 object catalog not found"));
    let self_check = skill_md
        .split_once("Every `type` tag spelled exactly:")
        .and_then(|(_, rest)| rest.split_once("\n4. "))
        .map(|(list, _)| list)
        .unwrap_or_else(|| panic!("{skill}: §14 self-check item 3 (the type-tag list) not found"));
    for tag in &tags {
        let code = format!("`{tag}`");
        assert!(
            catalog.contains(&format!("| {code} |")),
            "{skill}'s object catalog (§3) has no row for {code} — add it there"
        );
        assert!(
            skill_md.lines().any(|l| l.starts_with('#') && l.contains(&code)),
            "{skill} has no section headed {code} — add its field table"
        );
        assert!(
            self_check.contains(&code),
            "{skill}'s §14 self-check item 3 does not list {code} — add it to the tag list"
        );
    }
}

#[test]
fn doc_json_examples_parse_and_compile() {
    let mut objects_checked = 0usize;
    let mut decks_checked = 0usize;
    for doc in DOCS {
        let text = read_repo_file(doc);
        for (line, block) in json_blocks(&text) {
            let at = format!("{doc}:{line}");
            let trimmed = block.trim();
            // Fragments (`"width": 20`) and blocks with `/* … */` placeholders
            // are illustrative, not complete documents.
            if !trimmed.starts_with('{') || trimmed.contains("/*") {
                continue;
            }
            let value: serde_json::Value = serde_json::from_str(trimmed)
                .unwrap_or_else(|e| panic!("{at}: JSON example does not parse: {e}"));

            if value.get("type").is_some_and(|t| t.is_string()) {
                serde_json::from_value::<SceneObject>(value).unwrap_or_else(|e| {
                    panic!("{at}: example object no longer matches the serde structs: {e}")
                });
                objects_checked += 1;
            } else if let Some(objects) = value.get("objects").and_then(|o| o.as_array()) {
                if value.get("frame_count").is_some() {
                    decks_checked += 1;
                    let source: SourcePresentation = serde_json::from_value(value.clone())
                        .unwrap_or_else(|e| {
                            panic!("{at}: example deck no longer parses as a SourcePresentation: {e}")
                        });
                    source
                        .validate_loops()
                        .unwrap_or_else(|e| panic!("{at}: example deck fails validation: {e}"));
                    let scenes = Engine::compile(&source);
                    let contract = TerminalContract {
                        width: source.width,
                        height: source.height,
                    };
                    Renderer::render(&scenes, contract); // must not panic
                } else {
                    for obj in objects {
                        serde_json::from_value::<SceneObject>(obj.clone()).unwrap_or_else(|e| {
                            panic!("{at}: example object no longer matches the serde structs: {e}")
                        });
                        objects_checked += 1;
                    }
                }
            }
        }
    }
    // Guard against this check going vacuous if the block extraction breaks:
    // PRESENTATION_FORMAT.md alone has an example per object type, and both it
    // and AGENTS.md carry at least one full deck.
    assert!(
        objects_checked >= 10 && decks_checked >= 2,
        "only {objects_checked} example objects and {decks_checked} example decks were \
         checked — the ```json block extractor is broken"
    );
}

#[test]
fn agent_doc_stubs_only_import_agents_md() {
    for doc in AGENT_DOC_STUBS {
        assert_eq!(
            read_repo_file(doc).trim(),
            "@AGENTS.md",
            "{doc} must stay a bare `@AGENTS.md` import — move its extra content into \
             AGENTS.md, the single agent guide, so the two cannot drift apart"
        );
    }
}

#[test]
fn agents_md_module_map_covers_every_core_module() {
    let agents_md = read_repo_file("AGENTS.md");
    let root = repo_root();
    let mut missing = Vec::new();
    for dir in ["src", "src/editor", "src/engine", "src/renderer", "src/player"] {
        for entry in fs::read_dir(root.join(dir)).unwrap() {
            let path = entry.unwrap().path();
            if !path.extension().is_some_and(|e| e == "rs") {
                continue;
            }
            let rel = path.strip_prefix(&root).unwrap().to_string_lossy().into_owned();
            if !agents_md.contains(&rel) {
                missing.push(rel);
            }
        }
    }
    // src/engine/objects/* is exempt: it is documented as a directory whose
    // per-type contents are covered by the object catalog and the checklist
    // in src/engine/objects/mod.rs.
    missing.sort();
    assert!(
        missing.is_empty(),
        "these modules are absent from AGENTS.md (add a Module Map row or mention the \
         path where the module is described):\n  {}",
        missing.join("\n  ")
    );
}

/// GitHub's heading anchor: lowercase, spaces to `-`, other punctuation dropped.
fn heading_slug(heading: &str) -> String {
    heading
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

#[test]
fn presentation_format_section_references_resolve() {
    let doc = read_repo_file("PRESENTATION_FORMAT.md");
    let mut numbers = BTreeSet::new();
    let mut slugs = BTreeSet::new();
    let mut in_code = false;
    for line in doc.lines() {
        if line.starts_with("```") {
            in_code = !in_code;
        }
        if in_code || !line.starts_with('#') {
            continue; // a `#` comment inside a code block is not a heading
        }
        let heading = line.trim_start_matches('#').trim();
        slugs.insert(heading_slug(heading));
        let first = heading.split(' ').next().unwrap_or("").trim_end_matches('.');
        if !first.is_empty() && first.chars().all(|c| c.is_ascii_digit() || c == '.') {
            numbers.insert(first.to_string());
        }
    }
    let mut broken = Vec::new();
    for (i, line) in doc.lines().enumerate() {
        for part in line.split('§').skip(1) {
            let num: String = part.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            let num = num.trim_end_matches('.');
            if !numbers.contains(num) {
                broken.push(format!("line {}: §{num}", i + 1));
            }
        }
        for part in line.split("](#").skip(1) {
            let anchor = part.split(')').next().unwrap_or("");
            if !slugs.contains(anchor) {
                broken.push(format!("line {}: #{anchor}", i + 1));
            }
        }
    }
    assert!(
        broken.is_empty(),
        "PRESENTATION_FORMAT.md references sections that do not exist (fix the number or anchor):\n  {}",
        broken.join("\n  ")
    );
}

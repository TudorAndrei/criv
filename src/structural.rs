use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[cfg(test)]
use std::{cell::Cell, thread_local};

use ast_grep_config::{DeserializeEnv, SerializableRuleCore};
use ast_grep_core::meta_var::MetaVariable;
use ast_grep_core::{Doc, Matcher, NodeMatch, Pattern};
use ast_grep_language::{Language, LanguageExt, SupportLang};

use crate::source_paths::read_source_to_string;
use crate::util::GlobMatcher;
use crate::vault::{PolicyPattern, Vault};
use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy)]
pub(crate) enum PatternSource<'a> {
    Pattern(&'a str),
    Rule(&'a str),
}

/// Which source files a structural scan may visit.
///
/// An empty `GlobSet` matches nothing, so a caller that means "no filter" must
/// say so explicitly rather than passing an empty glob list. `Globs(&[])` keeps
/// the empty-means-nothing reading that the incremental state rebuild relies on.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PathScope<'a> {
    All,
    Globs(&'a [String]),
}

impl<'a> PathScope<'a> {
    pub(crate) fn from_paths(paths: &'a [String]) -> Self {
        if paths.is_empty() {
            Self::All
        } else {
            Self::Globs(paths)
        }
    }
}

enum CompiledPathScope {
    All,
    Globs(GlobMatcher),
}

impl CompiledPathScope {
    fn compile(scope: PathScope<'_>) -> Result<Self> {
        Ok(match scope {
            PathScope::All => Self::All,
            PathScope::Globs(paths) => Self::Globs(GlobMatcher::new(paths)?),
        })
    }

    fn is_match(&self, path: &str) -> bool {
        match self {
            Self::All => true,
            Self::Globs(matcher) => matcher.is_match(path),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct StructuralMatch {
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) range: String,
    pub(crate) text: String,
    pub(crate) captures: BTreeMap<String, String>,
}

enum CompiledMatcher {
    Pattern(Pattern),
    Rule(ast_grep_config::RuleCore),
}

pub(crate) struct PolicyScanRequest<'a> {
    pub(crate) key: usize,
    pub(crate) policy: &'a PolicyPattern,
    pub(crate) paths: &'a BTreeSet<String>,
}

struct CompiledPolicyRequest<'a> {
    key: usize,
    language: SupportLang,
    matcher: CompiledMatcher,
    paths: &'a BTreeSet<String>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct WorkCounts {
    policy_compilations: usize,
    pub(crate) ast_parses: usize,
}

#[cfg(test)]
thread_local! {
    static WORK_COUNTS: Cell<WorkCounts> = const { Cell::new(WorkCounts {
        policy_compilations: 0,
        ast_parses: 0,
    }) };
}

#[cfg(test)]
fn record_work(update: impl FnOnce(&mut WorkCounts)) {
    WORK_COUNTS.with(|counts| {
        let mut next = counts.get();
        update(&mut next);
        counts.set(next);
    });
}

#[cfg(test)]
pub(crate) fn reset_work_counts() {
    WORK_COUNTS.with(|counts| counts.set(WorkCounts::default()));
}

#[cfg(test)]
pub(crate) fn work_counts() -> WorkCounts {
    WORK_COUNTS.with(Cell::get)
}

#[cfg(test)]
pub(crate) fn reset_batch_parse_count() {
    reset_work_counts();
}

#[cfg(test)]
pub(crate) fn batch_parse_count() -> usize {
    work_counts().ast_parses
}

pub(crate) fn validate_source(source: PatternSource<'_>, language: &str) -> Result<()> {
    let language = parse_language(language)?;
    compile(source, language).map(|_| ())
}

pub(crate) fn find(
    root: &Path,
    vault: &Vault,
    source: PatternSource<'_>,
    scope: PathScope<'_>,
    language: Option<&str>,
) -> Result<Vec<StructuralMatch>> {
    let forced_language = language.map(parse_language).transpose()?;
    let mut rows = Vec::new();
    let mut compiled_any_language = false;
    let mut first_compile_error = None;
    let forced_matcher = forced_language
        .map(|language| compile(source, language))
        .transpose()?;

    let path_matcher = CompiledPathScope::compile(scope)?;
    for source_file in vault.source_files() {
        if !path_matcher.is_match(source_file) {
            continue;
        }
        if forced_language
            .is_some_and(|language| SupportLang::from_path(source_file) != Some(language))
        {
            continue;
        }
        let Some(language) = forced_language.or_else(|| SupportLang::from_path(source_file)) else {
            continue;
        };

        let matcher = if let Some(matcher) = forced_matcher.as_ref() {
            matcher
        } else {
            match compile(source, language) {
                Ok(matcher) => {
                    let contents = read_source_to_string(root, source_file)?;
                    rows.extend(scan_compiled_source(
                        source_file,
                        language,
                        &contents,
                        &matcher,
                    ));
                    compiled_any_language = true;
                    continue;
                }
                Err(err) => {
                    first_compile_error.get_or_insert(err);
                    continue;
                }
            }
        };
        compiled_any_language = true;

        let contents = read_source_to_string(root, source_file)?;
        rows.extend(scan_compiled_source(
            source_file,
            language,
            &contents,
            matcher,
        ));
    }

    if !compiled_any_language && let Some(err) = first_compile_error {
        return Err(err);
    }

    sort_matches(&mut rows);
    Ok(rows)
}

pub(crate) fn find_policies_batch(
    root: &Path,
    vault: &Vault,
    requests: &[PolicyScanRequest<'_>],
) -> Result<BTreeMap<usize, Vec<StructuralMatch>>> {
    let mut compiled = Vec::new();
    for request in requests {
        let (source, language) = policy_source(request.policy)?;
        let language = parse_language(language)?;
        #[cfg(test)]
        record_work(|counts| counts.policy_compilations += 1);
        compiled.push(CompiledPolicyRequest {
            key: request.key,
            language,
            matcher: compile(source, language)?,
            paths: request.paths,
        });
    }

    let mut rows_by_key = BTreeMap::<usize, Vec<StructuralMatch>>::new();
    for request in &compiled {
        rows_by_key.entry(request.key).or_default();
    }

    for source_file in vault.source_files() {
        let Some(language) = SupportLang::from_path(source_file) else {
            continue;
        };
        let requests = compiled
            .iter()
            .filter(|request| request.language == language && request.paths.contains(source_file))
            .collect::<Vec<_>>();
        if requests.is_empty() {
            continue;
        }

        let contents = read_source_to_string(root, source_file)?;
        #[cfg(test)]
        record_work(|counts| counts.ast_parses += 1);
        let ast = language.ast_grep(&contents);
        let root = ast.root();
        for request in requests {
            let rows = rows_by_key.entry(request.key).or_default();
            match &request.matcher {
                CompiledMatcher::Pattern(pattern) => {
                    rows.extend(
                        root.find_all(pattern)
                            .map(|matched| row_from_match(source_file, &matched)),
                    );
                }
                CompiledMatcher::Rule(rule) => {
                    rows.extend(
                        root.find_all(rule)
                            .map(|matched| row_from_match(source_file, &matched)),
                    );
                }
            }
        }
    }

    for rows in rows_by_key.values_mut() {
        sort_matches(rows);
    }
    Ok(rows_by_key)
}

fn sort_matches(rows: &mut Vec<StructuralMatch>) {
    rows.sort_by(|left, right| {
        (&left.path, left.line, &left.range, &left.text).cmp(&(
            &right.path,
            right.line,
            &right.range,
            &right.text,
        ))
    });
    rows.dedup_by(|left, right| {
        left.path == right.path && left.range == right.range && left.text == right.text
    });
}

pub(crate) fn find_policy_pattern_entry(
    root: &Path,
    vault: &Vault,
    policy: &PolicyPattern,
    scope: PathScope<'_>,
) -> Result<Vec<StructuralMatch>> {
    let (source, language) = policy_source(policy)?;
    validate_source(source, language)?;
    find(root, vault, source, scope, Some(language))
}

pub(crate) fn policy_pattern_entry_is_valid(policy: &PolicyPattern) -> bool {
    let Ok((source, language)) = policy_source(policy) else {
        return false;
    };
    validate_source(source, language).is_ok()
}

fn policy_source(policy: &PolicyPattern) -> Result<(PatternSource<'_>, &str)> {
    if !policy.has_inline_definition() {
        return Err(CrivError::new(
            "policy pattern must declare language and pattern or rule",
        ));
    }
    let Some(language) = policy
        .language
        .as_deref()
        .map(str::trim)
        .filter(|language| !language.is_empty())
    else {
        return Err(CrivError::new(
            "inline policy pattern must declare a language",
        ));
    };

    match (policy.pattern.as_deref(), policy.rule.as_deref()) {
        (Some(_), Some(_)) => Err(CrivError::new(
            "inline policy pattern must declare either pattern or rule, not both",
        )),
        (None, None) => Err(CrivError::new(
            "inline policy pattern must declare pattern or rule",
        )),
        (Some(pattern), None) => Ok((PatternSource::Pattern(pattern), language)),
        (None, Some(rule)) => Ok((PatternSource::Rule(rule), language)),
    }
}

pub(crate) fn language_glob(language: &str) -> &'static str {
    match language {
        "rust" | "rs" => "**/*.rs",
        "typescript" | "ts" => "**/*.ts",
        "tsx" => "**/*.tsx",
        "javascript" | "js" => "**/*.js",
        "jsx" => "**/*.jsx",
        "python" | "py" => "**/*.py",
        "go" | "golang" => "**/*.go",
        _ => "**",
    }
}

fn compile(source: PatternSource<'_>, language: SupportLang) -> Result<CompiledMatcher> {
    match source {
        PatternSource::Pattern(pattern) => Pattern::try_new(pattern, language)
            .map(CompiledMatcher::Pattern)
            .map_err(|err| {
                CrivError::new(format!(
                    "failed to compile ast-grep pattern for {language}: {err}"
                ))
            }),
        PatternSource::Rule(rule) => {
            let yaml = normalize_rule(rule);
            let serial: SerializableRuleCore = ast_grep_config::from_str(&yaml).map_err(|err| {
                CrivError::new(format!(
                    "failed to parse ast-grep rule for {language}: {err}"
                ))
            })?;
            serial
                .get_matcher(DeserializeEnv::new(language))
                .map(CompiledMatcher::Rule)
                .map_err(|err| {
                    CrivError::new(format!(
                        "failed to compile ast-grep rule for {language}: {err}"
                    ))
                })
        }
    }
}

fn scan_source<M: Matcher>(
    source_file: &str,
    language: SupportLang,
    contents: &str,
    matcher: &M,
) -> Vec<StructuralMatch> {
    let ast = language.ast_grep(contents);
    ast.root()
        .find_all(matcher)
        .map(|matched| row_from_match(source_file, &matched))
        .collect()
}

fn scan_compiled_source(
    source_file: &str,
    language: SupportLang,
    contents: &str,
    matcher: &CompiledMatcher,
) -> Vec<StructuralMatch> {
    match matcher {
        CompiledMatcher::Pattern(pattern) => scan_source(source_file, language, contents, pattern),
        CompiledMatcher::Rule(rule) => scan_source(source_file, language, contents, rule),
    }
}

fn row_from_match<D: Doc>(source_file: &str, matched: &NodeMatch<'_, D>) -> StructuralMatch {
    let node = matched.get_node();
    let start = node.start_pos();
    let end = node.end_pos();
    let line = start.line() + 1;
    StructuralMatch {
        path: source_file.to_string(),
        line,
        range: format!(
            "L{}:C{}-L{}:C{}",
            line,
            start.column(node) + 1,
            end.line() + 1,
            end.column(node) + 1
        ),
        text: node.text().trim().to_string(),
        captures: capture_map(matched),
    }
}

fn capture_map<D: Doc>(matched: &NodeMatch<'_, D>) -> BTreeMap<String, String> {
    let env = matched.get_env();
    let mut captures = BTreeMap::new();
    for variable in env.get_matched_variables() {
        match variable {
            MetaVariable::Capture(name, _) => {
                if let Some(node) = env.get_match(&name) {
                    captures.insert(name, node.text().to_string());
                }
            }
            MetaVariable::MultiCapture(name) => {
                let values = env
                    .get_multiple_matches(&name)
                    .into_iter()
                    .map(|node| node.text().to_string())
                    .collect::<Vec<_>>();
                captures.insert(name, values.join("\n"));
            }
            MetaVariable::Multiple | MetaVariable::Dropped(_) => {}
        }
    }
    captures
}

fn normalize_rule(rule: &str) -> String {
    let trimmed = rule.trim();
    if trimmed.starts_with("rule:") {
        return trimmed.to_string();
    }
    format!("rule:\n{}", indent_yaml(trimmed))
}

fn indent_yaml(value: &str) -> String {
    value
        .lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_language(language: &str) -> Result<SupportLang> {
    language
        .parse()
        .map_err(|err| CrivError::new(format!("unsupported ast-grep language `{language}`: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsx_language_glob_matches_only_jsx_files() {
        assert_eq!(language_glob("jsx"), "**/*.jsx");
        assert_eq!(language_glob("javascript"), "**/*.js");
    }
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn ast_grep_pattern_returns_ranges_and_captures() {
        let matches = scan_source(
            "src/main.rs",
            SupportLang::Rust,
            "fn main() {\n  println!(\"hi\");\n}\nfn helper() {}\n",
            &Pattern::try_new("fn $NAME() { $$$ }", SupportLang::Rust).unwrap(),
        );

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line, 1);
        assert_eq!(matches[0].range, "L1:C1-L3:C2");
        assert_eq!(
            matches[0].captures.get("NAME").map(String::as_str),
            Some("main")
        );
    }

    #[test]
    fn ast_grep_rule_supports_composite_yaml() {
        let rule = compile(
            PatternSource::Rule(
                r#"
all:
  - pattern: println!($$$ARGS)
  - inside:
      pattern: fn $NAME() { $$$ }
      stopBy: end
"#,
            ),
            SupportLang::Rust,
        )
        .unwrap();
        let CompiledMatcher::Rule(rule) = rule else {
            panic!("expected rule matcher");
        };
        let matches = scan_source(
            "src/main.rs",
            SupportLang::Rust,
            "fn main() {\n  println!(\"hi\");\n}\n",
            &rule,
        );

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].line, 2);
        assert_eq!(
            matches[0].captures.get("ARGS").map(String::as_str),
            Some("\"hi\"")
        );
    }

    #[test]
    fn batch_matches_sequential_find() {
        let (temp, vault) = policy_fixture();
        let function_policy = policy("rust", "fn $NAME() { $$$ }");
        let struct_policy = policy("rust", "struct $NAME;");
        let paths = vec!["src/**".to_string()];
        let policy_paths = BTreeSet::from(["src/left.rs".to_string(), "src/right.rs".to_string()]);
        let requests = vec![
            PolicyScanRequest {
                key: 0,
                policy: &function_policy,
                paths: &policy_paths,
            },
            PolicyScanRequest {
                key: 1,
                policy: &struct_policy,
                paths: &policy_paths,
            },
        ];

        let batch = find_policies_batch(temp.path(), &vault, &requests).unwrap();

        assert_eq!(
            batch.get(&0).unwrap(),
            &find_policy_pattern_entry(
                temp.path(),
                &vault,
                &function_policy,
                PathScope::Globs(&paths),
            )
            .unwrap()
        );
        assert_eq!(
            batch.get(&1).unwrap(),
            &find_policy_pattern_entry(
                temp.path(),
                &vault,
                &struct_policy,
                PathScope::Globs(&paths),
            )
            .unwrap()
        );
    }

    #[test]
    fn batch_respects_per_pattern_scopes() {
        let (temp, vault) = policy_fixture();
        let function_policy = policy("rust", "fn $NAME() { $$$ }");
        let left_paths = BTreeSet::from(["src/left.rs".to_string()]);
        let right_paths = BTreeSet::from(["src/right.rs".to_string()]);
        let requests = vec![
            PolicyScanRequest {
                key: 0,
                policy: &function_policy,
                paths: &left_paths,
            },
            PolicyScanRequest {
                key: 1,
                policy: &function_policy,
                paths: &right_paths,
            },
        ];

        reset_work_counts();
        let batch = find_policies_batch(temp.path(), &vault, &requests).unwrap();

        assert_eq!(
            batch
                .get(&0)
                .unwrap()
                .iter()
                .map(|row| row.path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/left.rs"]
        );
        assert_eq!(
            batch
                .get(&1)
                .unwrap()
                .iter()
                .map(|row| row.path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/right.rs"]
        );
        assert_eq!(
            work_counts(),
            WorkCounts {
                policy_compilations: 2,
                ast_parses: 2,
            },
            "each policy is compiled once and each affected source is parsed once"
        );
    }

    #[test]
    fn batch_skips_non_matching_language() {
        let (temp, vault) = policy_fixture();
        let python_policy = policy("python", "def $NAME($$$): $$$");
        let paths = BTreeSet::from(["src/left.rs".to_string(), "src/right.rs".to_string()]);
        let requests = vec![PolicyScanRequest {
            key: 0,
            policy: &python_policy,
            paths: &paths,
        }];

        let batch = find_policies_batch(temp.path(), &vault, &requests).unwrap();

        assert!(batch.get(&0).unwrap().is_empty());
    }

    #[test]
    fn path_scope_all_visits_every_source_file() {
        let (temp, vault) = policy_fixture();

        let rows = find(
            temp.path(),
            &vault,
            PatternSource::Pattern("fn $NAME() { $$$ }"),
            PathScope::All,
            Some("rust"),
        )
        .unwrap();

        assert_eq!(
            rows.iter().map(|row| row.path.as_str()).collect::<Vec<_>>(),
            vec!["src/left.rs", "src/right.rs"]
        );
    }

    #[test]
    fn path_scope_with_no_globs_visits_nothing() {
        let (temp, vault) = policy_fixture();

        let rows = find(
            temp.path(),
            &vault,
            PatternSource::Pattern("fn $NAME() { $$$ }"),
            PathScope::Globs(&[]),
            Some("rust"),
        )
        .unwrap();

        assert!(rows.is_empty());
    }

    fn policy(language: &str, pattern: &str) -> PolicyPattern {
        PolicyPattern {
            id: Some("test".to_string()),
            line: 1,
            language: Some(language.to_string()),
            pattern: Some(pattern.to_string()),
            rule: None,
            message: None,
        }
    }

    fn policy_fixture() -> (TempDir, Vault) {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]
"#,
        )
        .unwrap();
        fs::write(root.join("src/left.rs"), "fn left() {}\nstruct Left;\n").unwrap();
        fs::write(root.join("src/right.rs"), "fn right() {}\nstruct Right;\n").unwrap();
        let vault = Vault::load(root).unwrap();
        (temp, vault)
    }
}

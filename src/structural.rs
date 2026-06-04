use std::collections::BTreeMap;
use std::path::Path;

use ast_grep_config::{DeserializeEnv, SerializableRuleCore};
use ast_grep_core::meta_var::MetaVariable;
use ast_grep_core::{Doc, Matcher, NodeMatch, Pattern};
use ast_grep_language::{Language, LanguageExt, SupportLang};

use crate::config::PatternConfig;
use crate::util::{glob_matches, read_to_string};
use crate::vault::Vault;
use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy)]
pub(crate) enum PatternSource<'a> {
    Pattern(&'a str),
    Rule(&'a str),
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

pub(crate) fn pattern_source(config: &PatternConfig) -> Option<PatternSource<'_>> {
    config
        .pattern
        .as_deref()
        .map(PatternSource::Pattern)
        .or_else(|| config.rule.as_deref().map(PatternSource::Rule))
}

pub(crate) fn find(
    root: &Path,
    vault: &Vault,
    source: PatternSource<'_>,
    paths: &[String],
    language: Option<&str>,
) -> Result<Vec<StructuralMatch>> {
    let forced_language = language.map(parse_language).transpose()?;
    let mut rows = Vec::new();
    let mut compiled_any_language = false;
    let mut first_compile_error = None;

    for source_file in vault.source_files() {
        if !path_allowed(source_file, paths) {
            continue;
        }
        let Some(language) = forced_language.or_else(|| SupportLang::from_path(source_file)) else {
            continue;
        };

        let matcher = match compile(source, language) {
            Ok(matcher) => matcher,
            Err(err) if forced_language.is_none() => {
                first_compile_error.get_or_insert(err);
                continue;
            }
            Err(err) => return Err(err),
        };
        compiled_any_language = true;

        let path = root.join(source_file);
        let contents = read_to_string(&path)?;
        match matcher {
            CompiledMatcher::Pattern(pattern) => {
                rows.extend(scan_source(source_file, language, &contents, &pattern));
            }
            CompiledMatcher::Rule(rule) => {
                rows.extend(scan_source(source_file, language, &contents, &rule));
            }
        }
    }

    if !compiled_any_language && let Some(err) = first_compile_error {
        return Err(err);
    }

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
    Ok(rows)
}

pub(crate) fn find_pattern_id(
    root: &Path,
    vault: &Vault,
    pattern_id: &str,
    paths: &[String],
) -> Result<Vec<StructuralMatch>> {
    let Some(pattern) = vault.config.pattern_defs.get(pattern_id) else {
        return Err(CrivError::new(format!(
            "registered pattern `{pattern_id}` does not resolve"
        )));
    };
    let Some(source) = pattern_source(pattern) else {
        return Err(CrivError::new(format!(
            "registered pattern `{pattern_id}` has no ast-grep rule or pattern body"
        )));
    };
    let mut scoped_paths = paths.to_vec();
    if let Some(language) = &pattern.language {
        scoped_paths.push(language_glob(language).to_string());
    }
    find(
        root,
        vault,
        source,
        &scoped_paths,
        pattern.language.as_deref(),
    )
}

pub(crate) fn find_policy_pattern(
    root: &Path,
    vault: &Vault,
    pattern_id: &str,
    fallback_pattern: &str,
    paths: &[String],
) -> Result<Vec<StructuralMatch>> {
    if let Some(pattern) = vault.config.pattern_defs.get(pattern_id) {
        let Some(source) = pattern_source(pattern) else {
            return Ok(Vec::new());
        };
        let mut scoped_paths = paths.to_vec();
        if let Some(language) = &pattern.language {
            scoped_paths.push(language_glob(language).to_string());
        }
        return find(
            root,
            vault,
            source,
            &scoped_paths,
            pattern.language.as_deref(),
        );
    }

    find(
        root,
        vault,
        PatternSource::Pattern(fallback_pattern),
        paths,
        None,
    )
}

pub(crate) fn language_glob(language: &str) -> &'static str {
    match language {
        "rust" | "rs" => "**/*.rs",
        "typescript" | "ts" => "**/*.ts",
        "tsx" => "**/*.tsx",
        "javascript" | "js" | "jsx" => "**/*.js",
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

fn path_allowed(path: &str, patterns: &[String]) -> bool {
    patterns.is_empty() || patterns.iter().any(|pattern| glob_matches(pattern, path))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

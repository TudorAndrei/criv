use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use clap::{Args as ClapArgs, Subcommand};

use crate::structural::PatternSource;
use crate::vault::{PolicyPattern, Vault};
use crate::{CrivError, Result};

const START_MARKER: &str = "# criv:generated policy-patterns start";
const END_MARKER: &str = "# criv:generated policy-patterns end";

#[derive(Debug, ClapArgs)]
pub(crate) struct PolicyOptions {
    #[command(subcommand)]
    command: PolicyCommand,
}

#[derive(Debug, Subcommand)]
enum PolicyCommand {
    Generate(GenerateOptions),
}

#[derive(Debug, ClapArgs)]
struct GenerateOptions {
    #[arg(long)]
    check: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct GeneratedPattern {
    language: String,
    pattern: Option<String>,
    rule: Option<String>,
    message: Option<String>,
}

pub(crate) fn run(root: &Path, options: PolicyOptions) -> Result<()> {
    match options.command {
        PolicyCommand::Generate(options) => generate(root, options.check),
    }
}

fn generate(root: &Path, check: bool) -> Result<()> {
    let vault = Vault::load(root)?;
    let config_path = root.join("criv.toml");
    let current = if config_path.exists() {
        crate::util::read_to_string(&config_path)?
    } else {
        String::new()
    };
    let generated = render_generated_block(&vault)?;
    let expected = replace_generated_block(&current, &generated)?;

    if check {
        if normalize_trailing_newline(&current) != expected {
            return Err(CrivError::new(
                "generated policy patterns are stale; run `criv policy generate`",
            ));
        }
        println!("generated policy patterns are up to date");
        return Ok(());
    }

    if current != expected {
        fs::write(config_path, expected)?;
        println!("generated policy patterns updated");
    } else {
        println!("generated policy patterns are up to date");
    }
    Ok(())
}

fn render_generated_block(vault: &Vault) -> Result<String> {
    let patterns = generated_patterns(vault)?;
    if patterns.is_empty() {
        return Ok(String::new());
    }

    let mut output = String::new();
    output.push_str(START_MARKER);
    output.push('\n');
    output.push_str("# Generated from ADR policy.patterns. Do not edit this block by hand.");
    output.push('\n');
    for (id, pattern) in patterns {
        output.push('\n');
        output.push_str("[patterns.");
        output.push_str(&toml_string(&id));
        output.push_str("]\n");
        output.push_str("language = ");
        output.push_str(&toml_string(&pattern.language));
        output.push('\n');
        if let Some(body) = pattern.pattern {
            output.push_str("pattern = ");
            output.push_str(&toml_string(&body));
            output.push('\n');
        }
        if let Some(body) = pattern.rule {
            output.push_str("rule = ");
            output.push_str(&toml_string(&body));
            output.push('\n');
        }
        if let Some(message) = pattern.message {
            output.push_str("message = ");
            output.push_str(&toml_string(&message));
            output.push('\n');
        }
    }
    output.push('\n');
    output.push_str(END_MARKER);
    output.push('\n');
    Ok(output)
}

fn generated_patterns(vault: &Vault) -> Result<BTreeMap<String, GeneratedPattern>> {
    let mut generated = BTreeMap::new();
    for note in &vault.notes {
        let Some(adr_id) = &note.id else {
            continue;
        };
        for policy in &note.policy_patterns {
            if !policy.has_inline_definition() {
                continue;
            }
            let Some(local_id) = policy
                .id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
            else {
                return Err(CrivError::new(format!(
                    "{}:{}: inline policy pattern must declare an id",
                    note.rel_path, policy.line
                )));
            };
            let pattern = generated_pattern(&note.rel_path, policy, local_id)?;
            let id = format!("{adr_id}/{local_id}");
            if generated.insert(id.clone(), pattern).is_some() {
                return Err(CrivError::new(format!(
                    "{}:{}: generated policy pattern `{id}` is declared more than once",
                    note.rel_path, policy.line
                )));
            }
        }
    }
    Ok(generated)
}

fn generated_pattern(
    rel_path: &str,
    policy: &PolicyPattern,
    local_id: &str,
) -> Result<GeneratedPattern> {
    let Some(language) = policy
        .language
        .as_deref()
        .map(str::trim)
        .filter(|language| !language.is_empty())
    else {
        return Err(CrivError::new(format!(
            "{rel_path}:{}: inline policy pattern `{local_id}` must declare a language",
            policy.line
        )));
    };

    match (policy.pattern.as_deref(), policy.rule.as_deref()) {
        (Some(_), Some(_)) => Err(CrivError::new(format!(
            "{rel_path}:{}: inline policy pattern `{local_id}` must declare either pattern or rule, not both",
            policy.line
        ))),
        (None, None) => Err(CrivError::new(format!(
            "{rel_path}:{}: inline policy pattern `{local_id}` must declare pattern or rule",
            policy.line
        ))),
        (Some(pattern), None) => {
            crate::structural::validate_source(PatternSource::Pattern(pattern), language)?;
            Ok(GeneratedPattern {
                language: language.to_string(),
                pattern: Some(pattern.to_string()),
                rule: None,
                message: policy.message.clone(),
            })
        }
        (None, Some(rule)) => {
            crate::structural::validate_source(PatternSource::Rule(rule), language)?;
            Ok(GeneratedPattern {
                language: language.to_string(),
                pattern: None,
                rule: Some(rule.to_string()),
                message: policy.message.clone(),
            })
        }
    }
}

fn replace_generated_block(current: &str, generated: &str) -> Result<String> {
    let current = normalize_trailing_newline(current);
    match (current.find(START_MARKER), current.find(END_MARKER)) {
        (Some(start), Some(end)) if start <= end => {
            let after_start = end + END_MARKER.len();
            Ok(join_sections(
                &current[..start],
                generated,
                &current[after_start..],
            ))
        }
        (None, None) => Ok(join_sections(&current, generated, "")),
        _ => Err(CrivError::new(
            "generated policy pattern block markers are unbalanced in criv.toml",
        )),
    }
}

fn join_sections(before: &str, generated: &str, after: &str) -> String {
    let sections = [
        before.trim_end_matches('\n'),
        generated.trim_matches('\n'),
        after.trim_start_matches('\n').trim_end_matches('\n'),
    ]
    .into_iter()
    .filter(|section| !section.is_empty())
    .collect::<Vec<_>>();

    if sections.is_empty() {
        String::new()
    } else {
        format!("{}\n", sections.join("\n\n"))
    }
}

fn normalize_trailing_newline(value: &str) -> String {
    if value.is_empty() || value.ends_with('\n') {
        value.to_string()
    } else {
        format!("{value}\n")
    }
}

fn toml_string(value: &str) -> String {
    let mut output = String::from("\"");
    for value in value.chars() {
        match value {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            value if value < ' ' => {
                output.push_str(&format!("\\u{:04X}", value as u32));
            }
            value => output.push(value),
        }
    }
    output.push('"');
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_generated_block_without_touching_manual_sections() {
        let current = r#"[source]
roots = ["src"]

# criv:generated policy-patterns start
old = true
# criv:generated policy-patterns end

[enforce]
stages = ["commit"]
"#;
        let generated = "# criv:generated policy-patterns start\nnew = true\n# criv:generated policy-patterns end\n";

        assert_eq!(
            replace_generated_block(current, generated).unwrap(),
            r#"[source]
roots = ["src"]

# criv:generated policy-patterns start
new = true
# criv:generated policy-patterns end

[enforce]
stages = ["commit"]
"#
        );
    }

    #[test]
    fn toml_string_escapes_multiline_rule_body() {
        assert_eq!(
            toml_string("all:\n  - pattern: \"println!($$$ARGS)\""),
            "\"all:\\n  - pattern: \\\"println!($$$ARGS)\\\"\""
        );
    }
}

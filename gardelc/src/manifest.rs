// Copyright 2026 The Gardel Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file.

//! Manifest (`Gardel.toml`) parser and project model for Phase E (Task E.1).
//!
//! Implements the strict TOML 1.0 subset specified in Roadmap Decision 10:
//! - Tables, quoted and bare keys, basic strings, inline tables of strings, and comments.
//! - Validates package names according to Decision 11 (lowercase, digits, `-`, `.`,
//!   segments starting with a lowercase letter, no reserved `gardel` / `GARDEL` identity).
//! - Dependency-free, deterministic (`BTreeMap`) implementation reporting 1-based line
//!   numbers for any syntax error, duplicate key/table, bad escape, or construct outside
//!   the subset.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Parsed `Gardel.toml` manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The `[project]` section.
    pub project: ProjectSection,
}

/// The `[project]` table in `Gardel.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSection {
    /// Package name (e.g. `"echo-server"`, `"acme.json"`).
    pub name: String,
    /// Package version string (e.g. `"0.1.0"`).
    pub version: String,
}

/// An error encountered while parsing or validating a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError {
    /// 1-based line number in `Gardel.toml`.
    pub line: usize,
    /// Human-readable diagnostic description.
    pub message: String,
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line.max(1), self.message)
    }
}

impl std::error::Error for ManifestError {}

/// Derives the default alias from a package name (Decision 11).
///
/// Aliases are uppercase, defaulting to the last `.`-separated segment of the
/// package name with hyphens replaced by underscores.
/// Examples:
/// - `acme.json` -> `JSON`
/// - `acme.http-client` -> `HTTP_CLIENT`
/// - `echo-server` -> `ECHO_SERVER`
pub fn default_alias_for_package(pkg_name: &str) -> String {
    let last_seg = pkg_name.rsplit('.').next().unwrap_or(pkg_name);
    last_seg.to_ascii_uppercase().replace('-', "_")
}

/// Validates a package name against Decision 8 & Decision 11 rules:
/// - Only lowercase ASCII letters, digits, `-`, and `.`.
/// - Cannot be empty.
/// - Segments separated by `.` cannot be empty.
/// - Each segment must start with a lowercase ASCII letter (`a..=z`) and cannot end with `-`.
/// - Package name cannot start with `gardel.` or derive the alias `GARDEL` (reserved for Standard Prelude).
///
/// # Errors
///
/// Returns a descriptive `Err(String)` if `name` violates any package naming rule.
pub fn validate_package_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("package name cannot be empty".to_string());
    }
    for c in name.chars() {
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-' && c != '.' {
            return Err(format!(
                "invalid package name '{name}': package names may only contain lowercase letters, digits, '-' and '.'"
            ));
        }
    }
    let segments: Vec<&str> = name.split('.').collect();
    for seg in &segments {
        let Some(first) = seg.chars().next() else {
            return Err(format!(
                "invalid package name '{name}': segments separated by '.' cannot be empty"
            ));
        };
        if !first.is_ascii_lowercase() {
            return Err(format!(
                "invalid package name '{name}': segment '{seg}' must start with a lowercase letter"
            ));
        }
        if seg.ends_with('-') {
            return Err(format!(
                "invalid package name '{name}': segment '{seg}' cannot end with '-'"
            ));
        }
    }
    let alias = default_alias_for_package(name);
    if alias == "GARDEL" || segments[0] == "gardel" {
        return Err(format!(
            "invalid package name '{name}': 'gardel' and alias 'GARDEL' are reserved for the Standard Prelude"
        ));
    }
    Ok(())
}

/// A value in the TOML subset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TomlValue {
    /// A basic string literal `"..."`.
    String(String),
    /// An inline table of strings `{ key = "val", ... }`.
    InlineTable(BTreeMap<String, String>),
}

/// A parsed TOML table and the 1-based line number of its `[header]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTable {
    /// 1-based line number of the table header.
    pub line: usize,
    /// Map from key name to `(line_number, TomlValue)`.
    pub entries: BTreeMap<String, (usize, TomlValue)>,
}

/// Error returned when resolving a manifest project directory (`Gardel.toml` + `src/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectResolutionError {
    pub rule: &'static str,
    pub tag: &'static str,
    pub path: String,
    pub line: usize,
    pub message: String,
}

/// Resolves a project directory (`Gardel.toml` + `src/` directory).
///
/// # Errors
///
/// Returns [`ProjectResolutionError`] if `Gardel.toml` is missing or invalid,
/// or if `src/` is missing.
pub fn resolve_manifest_project_dir(
    dir: &Path,
) -> Result<(Manifest, PathBuf), ProjectResolutionError> {
    let manifest_path = dir.join("Gardel.toml");
    let manifest_path_str = manifest_path.to_string_lossy().replace('\\', "/");
    if !manifest_path.is_file() {
        return Err(ProjectResolutionError {
            rule: "IO",
            tag: "io_error",
            path: manifest_path_str,
            line: 1,
            message: format!("Could not find manifest 'Gardel.toml' in '{}'", dir.display()),
        });
    }
    let manifest_src =
        std::fs::read_to_string(&manifest_path).map_err(|e| ProjectResolutionError {
            rule: "IO",
            tag: "io_error",
            path: manifest_path_str.clone(),
            line: 1,
            message: format!("Failed to read manifest '{manifest_path_str}': {e}"),
        })?;
    let manifest = parse_manifest(&manifest_src).map_err(|err| ProjectResolutionError {
        rule: "Manifest",
        tag: "manifest_error",
        path: manifest_path_str.clone(),
        line: err.line,
        message: format!("{manifest_path_str}:{}: {}", err.line, err.message),
    })?;
    let src_dir = dir.join("src");
    let src_dir_str = src_dir.to_string_lossy().replace('\\', "/");
    if !src_dir.is_dir() {
        return Err(ProjectResolutionError {
            rule: "IO",
            tag: "io_error",
            path: src_dir_str,
            line: 1,
            message: format!(
                "Project '{}' in '{}' is missing its 'src/' directory",
                manifest.project.name,
                dir.display()
            ),
        });
    }
    Ok((manifest, src_dir))
}

/// Parses a `Gardel.toml` source string into a `Manifest`.
///
/// # Errors
///
/// Returns a [`ManifestError`] with a 1-based line number if the TOML syntax is
/// outside the supported subset, a duplicate key or table is present, an unknown
/// table or key is used, or package metadata is invalid.
pub fn parse_manifest(src: &str) -> Result<Manifest, ManifestError> {
    let raw_tables = parse_toml_subset(src)?;

    // Sort tables by source line number so unknown-table diagnostics are deterministic.
    let mut tables_by_line: Vec<(&String, &RawTable)> = raw_tables.iter().collect();
    tables_by_line.sort_by_key(|(_, tbl)| tbl.line);

    for (tbl_name, tbl) in tables_by_line {
        if tbl_name != "project" {
            return Err(ManifestError {
                line: tbl.line,
                message: format!("unknown table '[{tbl_name}]' in manifest"),
            });
        }
    }

    let Some(project_table) = raw_tables.get("project") else {
        return Err(ManifestError {
            line: 1,
            message: "missing required `[project]` table in manifest".to_string(),
        });
    };

    // Sort entries by line number for deterministic error reporting.
    let mut entries_by_line: Vec<(&String, &(usize, TomlValue))> =
        project_table.entries.iter().collect();
    entries_by_line.sort_by_key(|(_, (line, _))| *line);

    let mut name = None;
    let mut version = None;

    for (k, (line, val)) in entries_by_line {
        match k.as_str() {
            "name" => match val {
                TomlValue::String(s) => {
                    if let Err(e) = validate_package_name(s) {
                        return Err(ManifestError {
                            line: *line,
                            message: e,
                        });
                    }
                    name = Some(s.clone());
                }
                TomlValue::InlineTable(_) => {
                    return Err(ManifestError {
                        line: *line,
                        message: "key 'name' in `[project]` must be a string".to_string(),
                    });
                }
            },
            "version" => match val {
                TomlValue::String(s) => {
                    if s.trim().is_empty() {
                        return Err(ManifestError {
                            line: *line,
                            message: "key 'version' in `[project]` cannot be empty".to_string(),
                        });
                    }
                    version = Some(s.clone());
                }
                TomlValue::InlineTable(_) => {
                    return Err(ManifestError {
                        line: *line,
                        message: "key 'version' in `[project]` must be a string".to_string(),
                    });
                }
            },
            other => {
                return Err(ManifestError {
                    line: *line,
                    message: format!("unknown key '{other}' in `[project]` table"),
                });
            }
        }
    }

    let Some(name) = name else {
        return Err(ManifestError {
            line: project_table.line,
            message: "missing required key 'name' in `[project]`".to_string(),
        });
    };

    let Some(version) = version else {
        return Err(ManifestError {
            line: project_table.line,
            message: "missing required key 'version' in `[project]`".to_string(),
        });
    };

    Ok(Manifest {
        project: ProjectSection { name, version },
    })
}

/// Parses the strict TOML subset into a map of table names to [`RawTable`]s.
///
/// # Errors
///
/// Returns a [`ManifestError`] with the 1-based line number on any syntax error or
/// construct outside the subset.
pub fn parse_toml_subset(src: &str) -> Result<BTreeMap<String, RawTable>, ManifestError> {
    let mut tables: BTreeMap<String, RawTable> = BTreeMap::new();
    let mut current_table: Option<String> = None;

    for (zero_line, line_raw) in src.lines().enumerate() {
        let line_num = zero_line + 1;
        check_comment_control_chars(line_raw, line_num)?;
        let line = toml_trim(line_raw);

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') {
            let table_name = parse_table_header(line, line_num)?;
            if tables.contains_key(&table_name) {
                return Err(ManifestError {
                    line: line_num,
                    message: format!("duplicate table '[{table_name}]'"),
                });
            }
            tables.insert(
                table_name.clone(),
                RawTable {
                    line: line_num,
                    entries: BTreeMap::new(),
                },
            );
            current_table = Some(table_name);
            continue;
        }

        let Some(table) = &current_table else {
            return Err(ManifestError {
                line: line_num,
                message: "key-value pair found before any table header".to_string(),
            });
        };

        let (key, val) = parse_key_value(line, line_num)?;

        let Some(table_map) = tables.get_mut(table) else {
            return Err(ManifestError {
                line: line_num,
                message: format!("internal error: missing table '[{table}]'"),
            });
        };
        if table_map.entries.contains_key(&key) {
            return Err(ManifestError {
                line: line_num,
                message: format!("duplicate key '{key}' in table '[{table}]'"),
            });
        }

        table_map.entries.insert(key, (line_num, val));
    }

    Ok(tables)
}

/// Rejects control characters (`U+0000..=U+0008`, `U+000A..=U+001F`, `U+007F`) inside comments.
fn check_comment_control_chars(line: &str, line_num: usize) -> Result<(), ManifestError> {
    let mut in_str = false;
    let mut escape = false;
    let mut in_comment = false;

    for c in line.chars() {
        if in_comment {
            if is_disallowed_toml_control(c) {
                return Err(ManifestError {
                    line: line_num,
                    message: format!(
                        "control character U+{:04X} is not permitted in TOML comments",
                        u32::from(c)
                    ),
                });
            }
        } else if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
        } else if c == '#' {
            in_comment = true;
        }
    }
    Ok(())
}

/// Strips what TOML calls whitespace, space and tab. `str::trim` also strips Unicode spaces and
/// control characters such as a form feed, which TOML refuses outside a string.
fn toml_trim(s: &str) -> &str {
    s.trim_matches([' ', '\t'])
}

/// Parses a `[table]` header line.
fn parse_table_header(line: &str, line_num: usize) -> Result<String, ManifestError> {
    if line.starts_with("[[") {
        return Err(ManifestError {
            line: line_num,
            message: "array of tables '[[...]]' is not supported in manifest subset".to_string(),
        });
    }
    let Some(end_idx) = find_unquoted_char(line, ']') else {
        return Err(ManifestError {
            line: line_num,
            message: "unclosed table header (missing ']')".to_string(),
        });
    };

    let raw_name = toml_trim(&line[1..end_idx]);
    if raw_name.is_empty() {
        return Err(ManifestError {
            line: line_num,
            message: "table header name cannot be empty".to_string(),
        });
    }

    let trailing = toml_trim(&line[end_idx + 1..]);
    if !trailing.is_empty() && !trailing.starts_with('#') {
        return Err(ManifestError {
            line: line_num,
            message: format!("unexpected trailing content '{trailing}' after table header"),
        });
    }

    parse_key(raw_name, line_num)
}

/// Parses a single `key = value` line.
fn parse_key_value(line: &str, line_num: usize) -> Result<(String, TomlValue), ManifestError> {
    let Some(eq_pos) = find_unquoted_char(line, '=') else {
        return Err(ManifestError {
            line: line_num,
            message: "expected '=' in key-value pair".to_string(),
        });
    };

    let key_part = toml_trim(&line[..eq_pos]);
    let val_part = toml_trim(&line[eq_pos + 1..]);

    if key_part.is_empty() {
        return Err(ManifestError {
            line: line_num,
            message: "key cannot be empty".to_string(),
        });
    }

    let key = parse_key(key_part, line_num)?;
    let val = parse_value(val_part, line_num)?;

    Ok((key, val))
}

/// Parses a bare or quoted key (rejecting dotted keys and trailing characters).
fn parse_key(key_str: &str, line_num: usize) -> Result<String, ManifestError> {
    if key_str.starts_with('"') {
        let (parsed, trailing) = extract_string_literal(key_str, line_num)?;
        if !toml_trim(trailing).is_empty() {
            return Err(ManifestError {
                line: line_num,
                message: format!("unexpected trailing characters '{trailing}' in quoted key"),
            });
        }
        Ok(parsed)
    } else {
        if key_str.is_empty() {
            return Err(ManifestError {
                line: line_num,
                message: "key cannot be empty".to_string(),
            });
        }
        for c in key_str.chars() {
            if !c.is_ascii_alphanumeric() && c != '_' && c != '-' {
                return Err(ManifestError {
                    line: line_num,
                    message: format!("invalid character '{c}' in bare key '{key_str}'"),
                });
            }
        }
        Ok(key_str.to_string())
    }
}

/// Parses a TOML value in our strict subset (basic string or inline table of strings).
fn parse_value(val_str: &str, line_num: usize) -> Result<TomlValue, ManifestError> {
    if val_str.starts_with("\"\"\"") {
        return Err(ManifestError {
            line: line_num,
            message: "multi-line strings are not supported in manifest subset".to_string(),
        });
    }
    if val_str.starts_with('"') {
        let (s, trailing) = extract_string_literal(val_str, line_num)?;
        let trailing = toml_trim(trailing);
        if !trailing.is_empty() && !trailing.starts_with('#') {
            return Err(ManifestError {
                line: line_num,
                message: format!("unexpected trailing characters '{trailing}' after string value"),
            });
        }
        Ok(TomlValue::String(s))
    } else if val_str.starts_with('{') {
        let (tbl, trailing) = parse_inline_table(val_str, line_num)?;
        let trailing = toml_trim(trailing);
        if !trailing.is_empty() && !trailing.starts_with('#') {
            return Err(ManifestError {
                line: line_num,
                message: format!("unexpected trailing characters '{trailing}' after inline table"),
            });
        }
        Ok(TomlValue::InlineTable(tbl))
    } else {
        let clean = match find_unquoted_char(val_str, '#') {
            Some(idx) => toml_trim(&val_str[..idx]),
            None => val_str,
        };
        Err(ManifestError {
            line: line_num,
            message: format!(
                "unsupported value '{clean}' (only basic strings and inline tables of strings are supported in manifest subset)"
            ),
        })
    }
}

/// Extracts a `"..."` basic string literal and returns `(parsed_string, remaining_slice)`.
fn extract_string_literal(src: &str, line_num: usize) -> Result<(String, &str), ManifestError> {
    if !src.starts_with('"') {
        return Err(ManifestError {
            line: line_num,
            message: "expected '\"'".to_string(),
        });
    }

    let mut escape = false;
    let mut end_idx = None;

    for (idx, c) in src.char_indices().skip(1) {
        if escape {
            escape = false;
        } else if c == '\\' {
            escape = true;
        } else if c == '"' {
            end_idx = Some(idx);
            break;
        }
    }

    let Some(end_idx) = end_idx else {
        return Err(ManifestError {
            line: line_num,
            message: "unterminated string literal".to_string(),
        });
    };

    let inner = &src[1..end_idx];
    let remaining = &src[end_idx + 1..];
    let parsed = parse_string_literal(inner, line_num)?;
    Ok((parsed, remaining))
}

/// Whether `c` is a control character forbidden unescaped in TOML 1.0 basic strings and comments.
fn is_disallowed_toml_control(c: char) -> bool {
    let code = u32::from(c);
    (code < 0x20 && c != '\t') || code == 0x7F
}

/// Parses an N-digit hex unicode scalar escape (`\uXXXX` or `\UXXXXXXXX`).
fn parse_unicode_escape<I: Iterator<Item = char>>(
    chars: &mut I,
    digits: usize,
    prefix: char,
    line_num: usize,
) -> Result<char, ManifestError> {
    let mut hex = String::with_capacity(digits);
    for _ in 0..digits {
        match chars.next() {
            Some(h) if h.is_ascii_hexdigit() => hex.push(h),
            Some(bad) => {
                return Err(ManifestError {
                    line: line_num,
                    message: format!("invalid unicode escape char '{bad}'"),
                });
            }
            None => {
                return Err(ManifestError {
                    line: line_num,
                    message: format!("incomplete unicode escape \\{prefix}"),
                });
            }
        }
    }
    let Ok(code) = u32::from_str_radix(&hex, 16) else {
        return Err(ManifestError {
            line: line_num,
            message: format!("invalid unicode code point \\{prefix}{hex}"),
        });
    };
    let Some(ch) = char::from_u32(code) else {
        return Err(ManifestError {
            line: line_num,
            message: format!("invalid unicode code point \\{prefix}{hex}"),
        });
    };
    Ok(ch)
}

/// Parses TOML 1.0 escape sequences in a basic string body.
fn parse_string_literal(inner: &str, line_num: usize) -> Result<String, ManifestError> {
    let mut result = String::with_capacity(inner.len());
    let mut chars = inner.chars();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('b') => result.push('\x08'),
                Some('t') => result.push('\t'),
                Some('n') => result.push('\n'),
                Some('f') => result.push('\x0c'),
                Some('r') => result.push('\r'),
                Some('"') => result.push('"'),
                Some('\\') => result.push('\\'),
                Some('u') => result.push(parse_unicode_escape(&mut chars, 4, 'u', line_num)?),
                Some('U') => result.push(parse_unicode_escape(&mut chars, 8, 'U', line_num)?),
                Some(bad) => {
                    return Err(ManifestError {
                        line: line_num,
                        message: format!("bad escape '\\{bad}' in string literal"),
                    });
                }
                None => {
                    return Err(ManifestError {
                        line: line_num,
                        message: "trailing backslash in string literal".to_string(),
                    });
                }
            }
        } else if c == '"' {
            return Err(ManifestError {
                line: line_num,
                message: "unescaped '\"' in string literal".to_string(),
            });
        } else if is_disallowed_toml_control(c) {
            return Err(ManifestError {
                line: line_num,
                message: format!(
                    "unescaped control character U+{:04X} in string literal",
                    u32::from(c)
                ),
            });
        } else {
            result.push(c);
        }
    }

    Ok(result)
}

/// Parses a TOML 1.0 inline table `{ key1 = "val1", key2 = "val2" }`.
///
/// Trailing commas, leading commas, and empty entries are rejected per TOML 1.0.
fn parse_inline_table(
    src: &str,
    line_num: usize,
) -> Result<(BTreeMap<String, String>, &str), ManifestError> {
    if !src.starts_with('{') {
        return Err(ManifestError {
            line: line_num,
            message: "expected '{' for inline table".to_string(),
        });
    }

    let Some(end_idx) = find_unquoted_char(src, '}') else {
        return Err(ManifestError {
            line: line_num,
            message: "unclosed inline table (missing '}')".to_string(),
        });
    };

    let inner = toml_trim(&src[1..end_idx]);
    let remaining = &src[end_idx + 1..];

    let mut table = BTreeMap::new();
    if inner.is_empty() {
        return Ok((table, remaining));
    }

    let items = split_unquoted(inner, ',');
    for item in items {
        let item = toml_trim(item);
        if item.is_empty() {
            return Err(ManifestError {
                line: line_num,
                message: "trailing or empty comma is not permitted in TOML inline tables"
                    .to_string(),
            });
        }

        let Some(eq_idx) = find_unquoted_char(item, '=') else {
            return Err(ManifestError {
                line: line_num,
                message: "expected '=' in inline table entry".to_string(),
            });
        };

        let k_str = toml_trim(&item[..eq_idx]);
        let v_str = toml_trim(&item[eq_idx + 1..]);

        let key = parse_key(k_str, line_num)?;
        if !v_str.starts_with('"') || v_str.starts_with("\"\"\"") {
            return Err(ManifestError {
                line: line_num,
                message: format!("inline table entry '{key}' must be a basic string (found '{v_str}')"),
            });
        }

        let (val, trailing) = extract_string_literal(v_str, line_num)?;
        if !toml_trim(trailing).is_empty() {
            return Err(ManifestError {
                line: line_num,
                message: format!("unexpected trailing content '{trailing}' in inline table entry"),
            });
        }

        if table.contains_key(&key) {
            return Err(ManifestError {
                line: line_num,
                message: format!("duplicate key '{key}' in inline table"),
            });
        }

        table.insert(key, val);
    }

    Ok((table, remaining))
}

/// Finds the byte index of `target` in `s` outside double-quoted strings, stopping at `#` comments.
fn find_unquoted_char(s: &str, target: char) -> Option<usize> {
    let mut in_str = false;
    let mut escape = false;

    for (idx, c) in s.char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
        } else if c == '#' {
            return None;
        } else if c == target {
            return Some(idx);
        }
    }
    None
}

/// Splits `s` by `delimiter` where `delimiter` is outside double-quoted strings.
fn split_unquoted(s: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_str = false;
    let mut escape = false;

    for (idx, c) in s.char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
        } else if c == delimiter {
            parts.push(&s[start..idx]);
            start = idx + c.len_utf8();
        }
    }

    if start <= s.len() {
        parts.push(&s[start..]);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_manifest() {
        let src = r#"
            # Project manifest
            [project]
            name = "echo-server"
            version = "0.1.0"
        "#;
        let m = parse_manifest(src).expect("manifest should parse");
        assert_eq!(m.project.name, "echo-server");
        assert_eq!(m.project.version, "0.1.0");
        assert_eq!(default_alias_for_package(&m.project.name), "ECHO_SERVER");
    }

    #[test]
    fn test_package_name_rules() {
        assert!(validate_package_name("my-package").is_ok());
        assert!(validate_package_name("acme.json").is_ok());
        assert_eq!(default_alias_for_package("acme.json"), "JSON");
        assert!(validate_package_name("acme.http-client").is_ok());
        assert_eq!(default_alias_for_package("acme.http-client"), "HTTP_CLIENT");

        // Reject uppercase
        assert!(validate_package_name("EchoServer").is_err());
        // Reject underscore
        assert!(validate_package_name("echo_server").is_err());
        // Reject empty
        assert!(validate_package_name("").is_err());
        // Reject empty segment
        assert!(validate_package_name("foo..bar").is_err());
        // Reject hyphen at start/end of segment
        assert!(validate_package_name("-foo").is_err());
        assert!(validate_package_name("foo-").is_err());
        assert!(validate_package_name("foo.-bar").is_err());
        // Reject digit-leading package or segment
        assert!(validate_package_name("123").is_err());
        assert!(validate_package_name("9gag").is_err());
        assert!(validate_package_name("acme.9gag").is_err());
        // Reject GARDEL alias and gardel.* prefix
        assert!(validate_package_name("gardel").is_err());
        assert!(validate_package_name("foo.gardel").is_err());
        assert!(validate_package_name("gardel.core").is_err());
    }

    #[test]
    fn test_toml_1_0_strictness() {
        // \0 is not a valid TOML 1.0 escape (\u0000 must be used)
        let null_escape = "[project]\nname = \"echo-server\"\nversion = \"0.1.\\0\"\n";
        let err = parse_manifest(null_escape).expect_err("\\0 must be rejected");
        assert_eq!(err.line, 3);
        assert!(err.message.contains("bad escape '\\0'"));

        // \u0000 is valid TOML 1.0
        let unicode_null = "[project]\nname = \"echo-server\"\nversion = \"0.1.\\u0030\"\n";
        let m = parse_manifest(unicode_null).expect("\\u0030 must be accepted");
        assert_eq!(m.project.version, "0.1.0");

        // Trailing comma in inline table is invalid in TOML 1.0
        let trailing_comma = "a = { path = \"../foo\", }";
        let err = parse_key_value(trailing_comma, 5).expect_err("trailing comma must fail");
        assert_eq!(err.line, 5);
        assert!(err.message.contains("trailing or empty comma"));

        // Leading comma and double comma in inline table must fail
        assert!(parse_key_value("a = { , path = \"../foo\" }", 1).is_err());
        assert!(parse_key_value("a = { path = \"../foo\", , as = \"FOO\" }", 1).is_err());

        // Quoted key with trailing garbage or unescaped quote must fail
        let bad_quoted_key = "[project]\n\"name\" \"extra\" = \"echo-server\"\nversion = \"0.1.0\"\n";
        let err = parse_manifest(bad_quoted_key).expect_err("split quoted key must fail");
        assert_eq!(err.line, 2);

        let dotted_quoted_key = "[project]\n\"a\".\"b\" = \"echo-server\"\nversion = \"0.1.0\"\n";
        let err = parse_manifest(dotted_quoted_key).expect_err("dotted quoted key must fail");
        assert_eq!(err.line, 2);

        // Valid quoted key and quoted table header work
        let quoted_manifest = "[\"project\"]\n\"name\" = \"echo-server\"\n\"version\" = \"0.1.0\"\n";
        let m = parse_manifest(quoted_manifest).expect("quoted table and keys should parse");
        assert_eq!(m.project.name, "echo-server");

        // DEL (U+007F) in string or comment must fail
        let del_in_str = "[project]\nname = \"echo-server\"\nversion = \"0.1.\x7f\"\n";
        let err = parse_manifest(del_in_str).expect_err("DEL in string must fail");
        assert_eq!(err.line, 3);

        let del_in_comment = "# bad \x7f comment\n[project]\nname = \"echo-server\"\nversion = \"0.1.0\"\n";
        let err = parse_manifest(del_in_comment).expect_err("DEL in comment must fail");
        assert_eq!(err.line, 1);
    }

    #[test]
    fn test_bad_escape_and_duplicate_key() {
        let bad_escape = "[project]\nname = \"echo-server\"\nversion = \"0.1.\\x\"\n";
        let err = parse_manifest(bad_escape).expect_err("should fail");
        assert_eq!(err.line, 3);
        assert!(err.message.contains("bad escape '\\x'"));

        let dup_key = "[project]\nname = \"echo-server\"\nname = \"other\"\nversion = \"0.1.0\"\n";
        let err = parse_manifest(dup_key).expect_err("should fail");
        assert_eq!(err.line, 3);
        assert!(err.message.contains("duplicate key 'name'"));
    }

    #[test]
    fn test_duplicate_table_and_missing_keys_have_line_numbers() {
        let dup_table = "[project]\nname = \"a\"\nversion = \"1\"\n[project]\nname = \"b\"\n";
        let err = parse_manifest(dup_table).expect_err("should fail");
        assert_eq!(err.line, 4);
        assert!(err.message.contains("duplicate table '[project]'"));

        let missing_name = "# comment\n[project]\nversion = \"0.1.0\"\n";
        let err = parse_manifest(missing_name).expect_err("missing name should fail");
        assert_eq!(err.line, 2);
        assert!(err.message.contains("missing required key 'name'"));

        let missing_version = "\n\n[project]\nname = \"echo-server\"\n";
        let err = parse_manifest(missing_version).expect_err("missing version should fail");
        assert_eq!(err.line, 3);
        assert!(err.message.contains("missing required key 'version'"));

        let empty_manifest = "# empty\n";
        let err = parse_manifest(empty_manifest).expect_err("missing [project] should fail");
        assert_eq!(err.line, 1);
    }

    #[test]
    fn test_only_space_and_tab_are_toml_whitespace() {
        let name = "name = \"echo-server\"";
        let version = "version = \"0.1.0\"";
        let good = format!("\t[project] \t\r\n \t{name}\t# note\r\n \t\r\n{version} \r\n");
        parse_manifest(&good).expect("space, tab and CRLF are TOML");

        for (bad, line) in [
            (format!("\u{a0}[project]\n{name}\n{version}\n"), 1),
            (format!("[project]\u{a0}\n{name}\n{version}\n"), 1),
            (format!("[project]\n{name}\u{a0}\n{version}\n"), 2),
            (format!("[project]\n\u{3000}{name}\n{version}\n"), 2),
            (format!("[project]\nname\u{c} = \"echo-server\"\n{version}\n"), 2),
            (format!("[project]\nname =\u{b} \"echo-server\"\n{version}\n"), 2),
            (format!("[project]\n\u{a0}\n{name}\n{version}\n"), 2),
            (format!("[project]\n\u{c}\n{name}\n{version}\n"), 2),
            (format!("[project]\n{name}\r\r\n{version}\n"), 2),
        ] {
            let err = parse_manifest(&bad).expect_err(&format!("{bad:?} must be refused"));
            assert_eq!(err.line, line, "{bad:?}");
        }
        assert!(parse_key_value("a = {\u{a0}path = \"x\" }", 1).is_err());
        assert!(parse_key_value("a = { path\u{c} = \"x\" }", 1).is_err());
    }

    #[test]
    fn test_unsupported_value() {
        let non_string = "[project]\nname = \"a\"\nversion = 1\n";
        let err = parse_manifest(non_string).expect_err("should fail");
        assert_eq!(err.line, 3);
        assert!(err.message.contains("unsupported value"));
    }

    #[test]
    fn test_inline_table_subset() {
        let line = "a = { path = \"../foo\", as = \"FOO\" }";
        let (k, v) = parse_key_value(line, 1).expect("inline table should parse");
        assert_eq!(k, "a");
        match v {
            TomlValue::InlineTable(map) => {
                assert_eq!(map.get("path").expect("path key"), "../foo");
                assert_eq!(map.get("as").expect("as key"), "FOO");
            }
            TomlValue::String(_) => panic!("expected inline table"),
        }
    }
}

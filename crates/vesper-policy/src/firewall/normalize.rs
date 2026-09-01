//! Shell-text normalization for the hard-denial firewall.
//!
//! The firewall matches rules against what the shell would *actually
//! execute*, not what the model typed. This module strips data-heredoc
//! bodies, removes quoting, decodes ANSI-C escapes, splits pipeline and
//! list operators onto separate lines, and recursively extracts command
//! substitutions (`$(…)` and `` `…` ``) so a payload hidden three
//! substitutions deep is still visible to the rules.
//!
//! Honest limitation (PRD §1.5): normalization is lexical, not semantic.
//! Encoded payloads (`base64 … | sh`), two-step write-then-exec, and
//! interpreter `-c` indirection are documented residual risks contained
//! only by sandboxing (PRD Feature 2), never by this module.

/// Recursion ceiling for `$(…)` / `` `…` `` payloads; guards against
/// substitution bombs.
pub(super) const MAX_SUBSTITUTION_DEPTH: usize = 8;

/// Maximum normalized-text size before pruning (64 KiB). Adversarial
/// expansion is truncated, not failed: deny rules still match prefixes,
/// and truncation is visible in `scan_text`.
pub(super) const MAX_SCAN_TEXT_BYTES: usize = 64 * 1024;

/// Normalizes shell text for rule matching.
///
/// Returns operator-separated segments joined by newlines with any
/// trailing newline trimmed. Empty input yields empty output.
#[must_use]
pub fn normalize(command: &str) -> String {
    let mut raw = normalize_depth(command, 0);
    if raw.len() > MAX_SCAN_TEXT_BYTES {
        raw.truncate(MAX_SCAN_TEXT_BYTES);
        raw.push('…');
    }
    while raw.ends_with('\n') {
        raw.pop();
    }
    raw
}

/// One normalization pass: heredocs → vars → unquote → segment → substitutions.
fn normalize_depth(command: &str, depth: usize) -> String {
    let without_heredocs = strip_heredoc_bodies(command);
    let unquoted = unquote(&without_heredocs);
    let mut segmented = resolve_variables(&segment(&unquoted));
    if depth >= MAX_SUBSTITUTION_DEPTH {
        return prune_substitution_syntax(&segmented);
    }
    let substitutions = extract_substitutions(&without_heredocs);
    for payload in substitutions {
        segmented.push('\n');
        segmented.push_str(&normalize_depth(&payload, depth + 1));
    }
    segmented
}

/// Removes residual `$(`, backticks, and doubled parens left after the
/// depth ceiling is hit, so payload words anchor cleanly against rules.
fn prune_substitution_syntax(text: &str) -> String {
    text.replace("$(", " ")
        .replace('`', " ")
        .replace("((", " ( ( ")
        .replace("))", " ) ) ")
}

/// Interpreters whose stdin heredoc bodies are executable and must stay
/// visible to the rules.
const HEREDOC_INTERPRETERS: &[&str] = &[
    "psql", "mysql", "mariadb", "dropdb", "sqlcmd", "sqlite", "sqlite3", "python", "python2",
    "python3", "perl", "ruby", "node", "php", "lua", "rscript",
];

/// Whether the text before a `<<` operator feeds an interpreter (or a
/// shell reading its script from stdin), meaning the body will execute.
fn heredoc_feeds_interpreter(before: &str) -> bool {
    let Some(last_line) = before.lines().last() else {
        return false;
    };
    let words =
        last_line.split(|c: char| c.is_whitespace() || matches!(c, ';' | '|' | '&' | '(' | ')'));
    for word in words {
        if word.is_empty() || word.contains('=') || word.starts_with('-') {
            continue;
        }
        if matches!(word, "sudo" | "env" | "command" | "exec") {
            continue;
        }
        let name = word.rsplit('/').next().unwrap_or(word);
        if HEREDOC_INTERPRETERS.contains(&name) {
            return true;
        }
        if matches!(name, "sh" | "bash" | "dash" | "zsh" | "ksh") {
            let has_dash_c = last_line
                .split_whitespace()
                .skip(1)
                .any(|arg| arg.starts_with('-') && !arg.starts_with("--") && arg.contains('c'));
            return !has_dash_c;
        }
        return false;
    }
    false
}

/// Removes heredoc bodies that are data, keeping interpreter-fed ones.
///
/// Handles `<<DELIM`, `<<-DELIM` (tab-indented terminator), and quoted
/// delimiters (`<<'DELIM'`, `<<"DELIM"`). Unterminated bodies are treated
/// as extending to end-of-input: interpreter-fed bodies stay visible
/// (fail-safe), data bodies are dropped.
fn strip_heredoc_bodies(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(pos) = rest.find("<<") {
        let after_op = &rest[pos + 2..];
        let dash_len = usize::from(after_op.starts_with('-'));
        let after_dash = &after_op[dash_len..];
        let quote_len = after_dash
            .chars()
            .next()
            .filter(|c| *c == '\'' || *c == '"')
            .map_or(0, char::len_utf8);
        let delim: String = after_dash[quote_len..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if delim.is_empty() {
            // `<<` here is a shift/comparison/here-string, not a heredoc.
            output.push_str(&rest[..pos + 2]);
            rest = &rest[pos + 2..];
            continue;
        }
        let header_len = pos + 2 + dash_len + quote_len + delim.len();
        let Some(newline) = after_op.find('\n') else {
            // Header with no body start: keep everything.
            output.push_str(rest);
            return output;
        };
        let body_start = newline + 1;
        let Some(body_end) =
            find_heredoc_terminator(&after_op[body_start..], &delim).map(|end| body_start + end)
        else {
            // Unterminated body: remainder is the body.
            output.push_str(&rest[..header_len]);
            if heredoc_feeds_interpreter(&rest[..pos]) {
                output.push_str(&after_op[body_start..]);
            }
            return output;
        };
        output.push_str(&rest[..header_len]);
        if heredoc_feeds_interpreter(&rest[..pos]) {
            // Emit a hard line break: the kept body must not glue onto the
            // heredoc header, or rule anchors (`\brm\b`) misread the fused
            // token. The header itself is already scannable above.
            output.push('\n');
            output.push_str(&after_op[body_start..body_end]);
        }
        rest = &after_op[body_end..];
    }
    output.push_str(rest);
    output
}

/// Byte offset of the end of a heredoc body: the terminator is a line
/// whose entire content is exactly `delim` (leading tabs tolerated for
/// `<<-`, trailing whitespace tolerated).
fn find_heredoc_terminator(body: &str, delim: &str) -> Option<usize> {
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        if line.trim_start_matches('\t').trim_end() == delim {
            return Some(offset + line.len());
        }
        offset += line.len();
    }
    None
}

/// Unquotes `"…"`, `'…'`, and ANSI-C `$'…'`, and collapses backslash
/// escapes, so `"rm"`, `r\m`, and `$'rm'` all become scannable `rm`.
/// Unterminated quoted spans are kept verbatim (fail-safe: their content
/// stays visible to the rules).
fn unquote(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(ch) = rest.chars().next() {
        match ch {
            '"' => match closing_quote(rest, '"') {
                Some(end) => {
                    output.push_str(&unescape(&rest[1..end]));
                    rest = &rest[end + 1..];
                }
                None => {
                    output.push_str(rest);
                    return output;
                }
            },
            '\'' => match closing_quote(rest, '\'') {
                Some(end) => {
                    output.push_str(&rest[1..end]);
                    rest = &rest[end + 1..];
                }
                None => {
                    output.push_str(rest);
                    return output;
                }
            },
            '$' if rest.starts_with("$'") => {
                let inner = &rest[1..];
                match closing_quote(inner, '\'') {
                    Some(end) => {
                        output.push_str(&decode_ansi_c(&inner[1..end]));
                        rest = &inner[end + 1..];
                    }
                    None => {
                        output.push_str(rest);
                        return output;
                    }
                }
            }
            '\\' => {
                let mut tail = rest[1..].chars();
                match tail.next() {
                    Some(next) => {
                        output.push(next);
                        rest = &rest[1 + next.len_utf8()..];
                    }
                    None => return output,
                }
            }
            _ => {
                output.push(ch);
                rest = &rest[ch.len_utf8()..];
            }
        }
    }
    output
}

/// Byte index of the closing `quote` in `input`, where `input[0]` is the
/// opening quote. Backslash escapes count only inside double quotes,
/// matching shell semantics.
fn closing_quote(input: &str, quote: char) -> Option<usize> {
    let mut escaped = false;
    for (offset, ch) in input[1..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quote == '"' {
            escaped = true;
            continue;
        }
        if ch == quote {
            return Some(offset + 1);
        }
    }
    None
}

/// Collapses backslash escapes inside double-quoted spans.
fn unescape(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some(next) => output.push(next),
                None => break,
            }
        } else {
            output.push(ch);
        }
    }
    output
}

/// Decodes ANSI-C escapes per POSIX `$'…'` (`\t`, `\n`, `\162`, …).
fn decode_ansi_c(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => output.push('\n'),
            Some('t') => output.push('\t'),
            Some('r') => output.push('\r'),
            Some('a') => output.push('\u{7}'),
            Some('b') => output.push('\u{8}'),
            Some('e') => output.push('\u{1b}'),
            Some('f') => output.push('\u{c}'),
            Some('v') => output.push('\u{b}'),
            Some(digit @ '0'..='7') => {
                let mut value = octal_digit(digit);
                for _ in 0..2 {
                    match chars.peek().copied() {
                        Some(next @ '0'..='7') => {
                            chars.next();
                            value = value * 8 + octal_digit(next);
                        }
                        _ => break,
                    }
                }
                output.push(char::from_u32(u32::from(value)).unwrap_or('\u{fffd}'));
            }
            Some(other) => output.push(other),
            None => {}
        }
    }
    output
}

/// Digit value of an octal character.
fn octal_digit(ch: char) -> u8 {
    (ch as u8).wrapping_sub(b'0')
}

/// Splits pipeline and list operators onto separate lines: `|`, `||`,
/// `|&`, `&`, `&&`, `;`, and newlines all become `\n`.
fn segment(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while !rest.is_empty() {
        let ch = rest.chars().next().unwrap_or_default();
        let two_char = matches!(ch, '|' | '&') && rest.as_bytes().get(1) == Some(&(ch as u8));
        if matches!(ch, '|' | '&' | ';' | '\n') {
            output.push('\n');
            rest = if two_char { &rest[2..] } else { &rest[1..] };
        } else {
            output.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    output
}

/// Extracts command-substitution payloads (`$(…)` and `` `…` ``) as
/// separate strings for recursive normalization. Escaped forms (`\$(`,
/// `` \` ``) are skipped. `$((…))` arithmetic is treated as a nested
/// substitution payload (pruned at the depth ceiling).
pub(super) fn extract_substitutions(input: &str) -> Vec<String> {
    let mut payloads = Vec::new();
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let ch = bytes[index];
        if ch == b'\\' {
            index += 2; // skip escaped pair
            continue;
        }
        if ch == b'$' && bytes.get(index + 1) == Some(&b'(') {
            if let Some(close) = find_substitution_close(&input[index + 2..]) {
                payloads.push(input[index + 2..index + 2 + close].to_string());
                index = index + 2 + close + 1;
                continue;
            }
            index += 2;
            continue;
        }
        if ch == b'`' {
            if let Some(close) = input[index + 1..].find('`') {
                payloads.push(input[index + 1..index + 1 + close].to_string());
                index = index + 1 + close + 1;
                continue;
            }
            index += 1;
            continue;
        }
        index += 1;
    }
    payloads
}

/// Byte offset of the `)` that closes a substitution whose text starts
/// right after `$('`. Depth starts at 1 (the consumed `(`); nested
/// parens, quotes, and backslash escapes are honored.
fn find_substitution_close(input: &str) -> Option<usize> {
    let mut depth = 1usize;
    let mut quote: Option<char> = None;
    let mut chars = input.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            } else if ch == '\\' && q == '"' {
                chars.next();
            }
            continue;
        }
        match ch {
            '\'' | '"' | '`' => quote = Some(ch),
            '\\' => {
                chars.next();
            }
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Resolves `NAME=value` assignments that appear in the same command so
/// `X=/; rm -rf $X` becomes scannable `rm -rf /`. Assignments whose value is
/// quoted or contains shell metacharacters are left untouched (fail-safe:
/// unresolved text stays visible to rules).
fn resolve_variables(input: &str) -> String {
    let assignments: Vec<(String, String)> = input
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let eq = trimmed.find('=')?;
            let name = &trimmed[..eq];
            let value = trimmed[eq + 1..].trim_matches(['"', '\'']);
            if name.is_empty()
                || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                || name.chars().next().is_some_and(|c| c.is_ascii_digit())
            {
                return None;
            }
            if value.contains('$') || value.contains(';') || value.contains('|') {
                return None;
            }
            Some((name.to_string(), value.to_string()))
        })
        .collect();
    if assignments.is_empty() {
        return input.to_string();
    }
    let mut output = input.to_string();
    for (name, value) in assignments {
        let bare = format!("${name}");
        let braced = format!("${{{name}}}");
        if output.contains(&bare) {
            output = output.replace(&bare, &value);
        }
        if output.contains(&braced) {
            output = output.replace(&braced, &value);
        }
    }
    output
}

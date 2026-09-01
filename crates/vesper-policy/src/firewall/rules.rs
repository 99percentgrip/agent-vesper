//! Compiled rules and the scan algorithm for the hard-denial firewall.
//!
//! Rules are ordered triples `(pattern, decision, reason)`. Each pattern is
//! compiled once against lowercase input: the workspace pins `regex`
//! without `unicode-case`, so case-insensitivity is achieved by lowercasing
//! the pattern at compile time and the command at scan time — never `(?i)`.
//!
//! A scan evaluates every rule against the raw (lowercased) command and its
//! [`normalize`](crate::firewall::normalize)d form. The most severe match
//! wins; the earliest matching rule index breaks ties. `Deny` is absolute
//! and cannot be overridden by an `allow` rule or by Bypass (PRD §1.3).

use std::sync::OnceLock;

use regex::RegexBuilder;

use super::normalize;
use serde::{Deserialize, Serialize};

/// Verdict severity. Ordered so comparisons rank outcomes directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RuleDecision {
    /// No rule objected.
    Allow,
    /// Permit only after human approval through the existing broker.
    RequireApproval,
    /// Absolute refusal; Bypass cannot override (PRD §1.3).
    Deny,
}

/// One compiled rule.
#[derive(Debug, Clone)]
pub struct CompiledRule {
    /// Original pattern text (pre-lowercasing), for display and config.
    pub pattern: String,
    /// Compiled regex, matching lowercase text.
    pub regex: regex::Regex,
    /// Verdict when this rule matches.
    pub decision: RuleDecision,
    /// Stable human reason surfaced in deny output.
    pub reason: &'static str,
}

/// One verdict for a scanned command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirewallVerdict {
    /// Winning decision; `Allow` when no rule matched.
    pub decision: RuleDecision,
    /// Index of the winning rule in the compiled list, if any.
    pub matched_rule: Option<usize>,
    /// Stable reason of the winning rule, if any.
    pub matched_reason: Option<&'static str>,
    /// The normalized text rules actually evaluated. Surfaced for
    /// diagnostics only — never re-matched.
    pub scan_text: String,
}

/// A compiled, ordered command ruleset.
#[derive(Clone, Debug)]
pub struct CommandFirewall {
    rules: Vec<CompiledRule>,
}

impl CommandFirewall {
    /// Compiles rules from `(pattern, decision, reason)` triples. An
    /// invalid pattern fails closed with an error naming the rule index.
    pub fn compile(rules: &[(&str, RuleDecision, &'static str)]) -> Result<Self, String> {
        let mut compiled = Vec::with_capacity(rules.len());
        for (index, (pattern, decision, reason)) in rules.iter().enumerate() {
            // Compile the pattern VERBATIM. Never lowercase it here: `\S`,
            // `\W`, and `\D` classes would be silently corrupted into
            // `\s`/`\w`/`\d`. Case-insensitivity comes from lowercasing
            // the SCAN TEXT instead, so patterns are authored lowercase.
            let regex = RegexBuilder::new(pattern)
                .size_limit(64 * 1024)
                .build()
                .map_err(|error| format!("rule {index} pattern `{pattern}` is invalid: {error}"))?;
            compiled.push(CompiledRule {
                pattern: (*pattern).to_string(),
                regex,
                decision: *decision,
                reason,
            });
        }
        Ok(Self { rules: compiled })
    }

    /// Compiles from owned triples; used by [`Self::compose`], which
    /// reconstructs triples from an existing firewall plus custom rules.
    fn compile_owned(rules: Vec<(String, RuleDecision, String)>) -> Result<Self, String> {
        let refs: Vec<(&str, RuleDecision, &str)> = rules
            .iter()
            .map(|(p, d, r)| (p.as_str(), *d, r.as_str()))
            .collect();
        let borrowed: Vec<(&str, RuleDecision, &str)> = refs;
        let mut compiled = Vec::with_capacity(borrowed.len());
        for (index, (pattern, decision, reason)) in borrowed.iter().enumerate() {
            let regex = RegexBuilder::new(pattern)
                .size_limit(64 * 1024)
                .build()
                .map_err(|error| format!("rule {index} pattern `{pattern}` is invalid: {error}"))?;
            // Leak-safe: reasons must live for the lifetime of the compiled
            // firewall, so owned reasons are leaked once here.
            let leaked_reason: &'static str = Box::leak(reason.to_string().into_boxed_str());
            let leaked_pattern: &'static str = Box::leak(pattern.to_string().into_boxed_str());
            compiled.push(CompiledRule {
                pattern: leaked_pattern.to_string(),
                regex,
                decision: *decision,
                reason: leaked_reason,
            });
        }
        Ok(Self { rules: compiled })
    }

    /// A firewall with no rules: every scan allows. Hosts hold this when
    /// the firewall is disabled, so a scan costs one `is_empty` check.
    #[must_use]
    pub fn empty() -> Self {
        Self { rules: Vec::new() }
    }

    /// The default single-user ruleset (PRD §1.2), compiled once per process.
    #[must_use]
    pub fn default_ruleset() -> &'static Self {
        static DEFAULT: OnceLock<CommandFirewall> = OnceLock::new();
        DEFAULT.get_or_init(|| Self::compile(DEFAULT_RULES).expect("default ruleset compiles"))
    }

    /// Number of compiled rules.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Composes custom rules over the given default ruleset (PRD §1.4).
    /// Custom rules are appended after the defaults; because the most
    /// severe decision wins regardless of index, appended rules can only
    /// *tighten* (deny/approval) or *annotate* — never loosen a default
    /// deny. Use [`CommandFirewall::compile`] for exactly-custom rules.
    pub fn compose(&self, custom: &[(&str, RuleDecision, &'static str)]) -> Result<Self, String> {
        // Compiled patterns are stored verbatim (author-lowercase), so
        // re-compiling from them is lossless.
        let mut triples: Vec<(String, RuleDecision, String)> = self
            .rules
            .iter()
            .map(|r| (r.pattern.clone(), r.decision, r.reason.to_string()))
            .collect();
        let owned: Vec<(String, RuleDecision, String)> = custom
            .iter()
            .map(|(p, d, r)| ((*p).to_string(), *d, (*r).to_string()))
            .collect();
        triples.extend(owned);
        Self::compile_owned(triples)
    }

    /// Whether no rules are compiled.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Compiled rules in declaration order.
    #[must_use]
    pub fn rules(&self) -> &[CompiledRule] {
        &self.rules
    }

    /// Scans one command. Rules are matched against the normalized,
    /// lowercased text only — never the raw command — so that normalization
    /// is security-meaningful: quoted/escaped/ANSI-C forms decode, pipeline
    /// and list operators split commands apart, and data-heredoc bodies are
    /// stripped before any rule sees the text. The most severe matched
    /// decision wins; the earliest matching index breaks ties.
    #[must_use]
    pub fn scan(&self, command: &str) -> FirewallVerdict {
        // Zero-overhead contract: a disabled firewall must never pay for
        // normalization. `is_empty` is one length check; only an enabled
        // firewall normalizes.
        if self.rules.is_empty() {
            return FirewallVerdict {
                decision: RuleDecision::Allow,
                matched_rule: None,
                matched_reason: None,
                scan_text: String::new(),
            };
        }
        let normalized = normalize(command).to_lowercase();
        let mut best: Option<(usize, RuleDecision, &'static str)> = None;
        for (index, rule) in self.rules.iter().enumerate() {
            if !rule.regex.is_match(&normalized) {
                continue;
            }
            let better = match best {
                None => true,
                Some((_, decision, _)) => rule.decision > decision,
            };
            if better {
                best = Some((index, rule.decision, rule.reason));
            }
        }
        match best {
            Some((index, decision, reason)) => FirewallVerdict {
                decision,
                matched_rule: Some(index),
                matched_reason: Some(reason),
                scan_text: normalized,
            },
            None => FirewallVerdict {
                decision: RuleDecision::Allow,
                matched_rule: None,
                matched_reason: None,
                scan_text: normalized,
            },
        }
    }
}

/// Default deny/approval rules (PRD §1.2). Declared deny-first; because the
/// most severe decision wins regardless of order, this is documentation of
/// intent rather than a functional requirement. Patterns are lowercase and
/// are matched ONLY against the normalized, lowercased scan text — never
/// the raw command — so normalization decisions (e.g. stripping data-heredoc
/// bodies) are authoritative. Pipeline-context rules therefore match the
/// segmented form (`|`/`;`/`&` became newlines), not the raw operator.
pub(crate) static DEFAULT_RULES: &[(&str, RuleDecision, &str)] = &[
    // ---- hard denials: filesystem-destroying commands -----------------
    (
        r"\bmkfs(\.\w+)?\b",
        RuleDecision::Deny,
        "mkfs on a filesystem",
    ),
    // Fork-bomb signature: a `:` function definition whose body invokes `:`.
    // Written against segmented text (operators become newlines).
    (r":\(\)\s*\{[^}]*:", RuleDecision::Deny, "fork bomb"),
    (
        r"\bdd\b[^|\n]*\bof=/dev/(sd|nvme|hd|vd)",
        RuleDecision::Deny,
        "dd to raw block device",
    ),
    (
        r"\bdd\b[^|\n]*\bof=/dev/mem",
        RuleDecision::Deny,
        "dd to /dev/mem",
    ),
    // Recursive delete of root, home, glob-of-root, or system directories.
    // Path scoping for ordinary targets stays with the approval broker.
    (
        r"\brm\b[^|\n]*\s-{1,2}[a-z]*r[a-z]*\b[^|\n]*\s(?:/(?:[^a-z0-9_./~-]|$)|//|/\*|~(?:\s|$)|\$\{?home\}?(?:\s|$)|/(?:etc|usr|bin|sbin|var|home|root|boot|lib|lib64|opt|srv)(?:[/\s]|$))",
        RuleDecision::Deny,
        "recursive delete of system root or home",
    ),
    // Recursive world-writable chmod on system root. Anchor on the mode
    // value; any 777/0777 chmod whose subsequent text contains a bare root
    // or system-directory path is denied.
    (
        r"\bchmod\b[^|\n]*\s-{1,2}[a-z]*r[a-z]*\b[^|\n]*\b0?777\b[^|\n]*\s/(?:etc|usr|bin|sbin|var|home|root|boot|lib|lib64|opt|srv)?(?:[/\s]|$)",
        RuleDecision::Deny,
        "recursive world-writable on system root",
    ),
    (
        r"\bchown\b[^|\n]*\s-[a-z]*r[a-z]*\s+\S+\s+/\s*$",
        RuleDecision::Deny,
        "recursive chown on /",
    ),
    // ---- approval-gated ----------------------------------------------
    (
        r"\bgit\s+push\b[^|\n]*(-f\b|--force\b)",
        RuleDecision::RequireApproval,
        "force push",
    ),
    (
        r"\bgit\s+reset\b[^|\n]*--hard\b",
        RuleDecision::RequireApproval,
        "git reset --hard",
    ),
    (
        r"\b(drop|truncate)\s+table\b",
        RuleDecision::RequireApproval,
        "destructive SQL",
    ),
    // `curl|wget` piped into a shell. Segmented form: the shell is the
    // first word of the line following the download.
    (
        r"\b(curl|wget)\b[^\n]*\n\s*(sudo\s+)?(ba|z|da|k)?sh\b",
        RuleDecision::RequireApproval,
        "download piped to shell",
    ),
];

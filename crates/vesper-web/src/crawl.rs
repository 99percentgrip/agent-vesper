//! Crawl discipline types (VRO-14 PR-2).
//!
//! Pure admission policy for a bounded crawl: no transport, no clock, no
//! network. A host supplies fetched pages; this module decides which URLs
//! may enter the frontier and records a typed denial reason for every
//! exclusion — the discipline the PRD §1.6 ports from web oracle alpha's
//! `DenialReason` surface so the model can debug its own crawl.

use crate::links::normalize_url;

/// Bounds for one crawl run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrawlPolicy {
    /// Maximum URLs admitted across the whole run.
    pub max_urls: usize,
    /// Maximum path-segment depth (alpha's `maxDepth` semantics: the
    /// number of non-empty path segments).
    pub max_depth: usize,
    /// Maximum concurrent fetches the transport may run (a hint for the
    /// composition boundary's executor, not enforced here).
    pub concurrency: usize,
    /// Admit only URLs on the seed's origin (scheme + host + port).
    pub same_origin: bool,
    /// Respect robots.txt rules supplied by the caller.
    pub respect_robots: bool,
}

impl Default for CrawlPolicy {
    fn default() -> Self {
        Self {
            max_urls: 20,
            max_depth: 2,
            concurrency: 2,
            same_origin: true,
            respect_robots: true,
        }
    }
}

impl CrawlPolicy {
    /// The hard ceiling this PRD allows for `max_urls`.
    pub const MAX_URLS_CEILING: usize = 200;
    /// The hard ceiling this PRD allows for `max_depth`.
    pub const MAX_DEPTH_CEILING: usize = 10;
    /// The hard ceiling this PRD allows for `concurrency`.
    pub const CONCURRENCY_CEILING: usize = 4;

    /// Clamp every bound to its PRD ceiling (fail-safe, never widen).
    pub fn clamped(&self) -> Self {
        Self {
            max_urls: self.max_urls.min(Self::MAX_URLS_CEILING),
            max_depth: self.max_depth.min(Self::MAX_DEPTH_CEILING),
            concurrency: self.concurrency.clamp(1, Self::CONCURRENCY_CEILING),
            same_origin: self.same_origin,
            respect_robots: self.respect_robots,
        }
    }
}

/// Typed denial for a URL excluded from the frontier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrawlDenial {
    /// Path-segment count exceeds `max_depth`.
    DepthLimit { depth: usize, max_depth: usize },
    /// robots.txt disallows this path for the configured user agent.
    RobotsDisallowed,
    /// Different origin while `same_origin` is set.
    OffOrigin,
    /// The run already admitted `max_urls` URLs.
    BudgetExhausted,
    /// Normalized form already admitted (duplicate).
    DuplicateNormalized,
    /// Non-web scheme (`mailto:`, `tel:`, `ftp:`, …).
    NonWebProtocol,
    /// The URL could not be parsed.
    UrlParseError,
}

impl CrawlDenial {
    /// Stable machine-readable name (fixture/log friendly).
    pub fn name(&self) -> &'static str {
        match self {
            Self::DepthLimit { .. } => "depth_limit",
            Self::RobotsDisallowed => "robots_disallowed",
            Self::OffOrigin => "off_origin",
            Self::BudgetExhausted => "budget_exhausted",
            Self::DuplicateNormalized => "duplicate_normalized",
            Self::NonWebProtocol => "non_web_protocol",
            Self::UrlParseError => "url_parse_error",
        }
    }
}

/// The outcome of admitting one URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// The URL may enter the frontier (with its normalized form).
    Admitted { normalized: String },
    /// The URL is excluded, with the typed reason.
    Denied(CrawlDenial),
}

/// Minimal robots.txt rule set (pure, caller-fetched).
///
/// Supports `User-agent`-scoped `Allow`/`Disallow` prefixes and a global
/// `*` section — the subset alpha's robots-parser supplies in practice.
#[derive(Debug, Clone, Default)]
pub struct RobotsRules {
    /// `(prefix, allowed)` pairs, most-specific (longest) prefix wins.
    rules: Vec<(String, bool)>,
}

impl RobotsRules {
    /// Parse robots.txt content for one user agent (plus `*`).
    pub fn parse(body: &str, user_agent: &str) -> Self {
        let agent = user_agent.to_ascii_lowercase();
        type RuleGroup = (Vec<String>, Vec<(String, bool)>);
        let mut groups: Vec<RuleGroup> = Vec::new();
        let mut agents = Vec::new();
        let mut rules = Vec::new();
        let mut saw_rule = false;
        for line in body.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (key, value) = match line.split_once(':') {
                Some((key, value)) => (key.trim().to_ascii_lowercase(), value.trim()),
                None => continue,
            };
            match key.as_str() {
                "user-agent" => {
                    if saw_rule {
                        groups.push((std::mem::take(&mut agents), std::mem::take(&mut rules)));
                        saw_rule = false;
                    }
                    agents.push(value.to_ascii_lowercase());
                }
                "allow" | "disallow" => {
                    saw_rule = true;
                    if !agents.is_empty() && !value.is_empty() {
                        rules.push((robots_octets(value), key == "allow"));
                    }
                }
                _ => {}
            }
        }
        groups.push((agents, rules));
        let specific = groups
            .iter()
            .any(|(agents, _)| agents.iter().any(|name| name == &agent));
        let rules = groups
            .into_iter()
            .filter(|(agents, _)| {
                agents.iter().any(|name| {
                    if specific {
                        name == &agent
                    } else {
                        name == "*"
                    }
                })
            })
            .flat_map(|(_, rules)| rules)
            .collect();
        Self { rules }
    }

    /// Longest-prefix match decides; empty rules allow everything.
    pub fn is_allowed(&self, path: &str) -> bool {
        let path = robots_octets(path);
        self.rules
            .iter()
            .filter(|(prefix, _)| robots_matches(prefix.as_bytes(), path.as_bytes()))
            .max_by_key(|(prefix, allowed)| {
                (
                    prefix
                        .bytes()
                        .filter(|byte| !matches!(byte, b'*' | b'$'))
                        .count(),
                    *allowed,
                )
            })
            .map(|(_, allowed)| *allowed)
            .unwrap_or(true)
    }
}

// RFC 9309 matching: decode percent-encoded unreserved octets; preserve
// reserved escapes and encode non-ASCII UTF-8 octets before comparison.
fn robots_octets(value: &str) -> String {
    fn hex(byte: u8) -> Option<u8> {
        (byte as char).to_digit(16).map(|v| v as u8)
    }
    let bytes = value.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b'%'
            && i + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            let decoded = high * 16 + low;
            if decoded.is_ascii_alphanumeric() || b"-._~".contains(&decoded) {
                out.push(decoded as char);
            } else {
                out.push_str(&format!("%{decoded:02X}"));
            }
            i += 3;
        } else {
            if byte.is_ascii() {
                out.push(byte as char);
            } else {
                out.push_str(&format!("%{byte:02X}"));
            }
            i += 1;
        }
    }
    out
}

fn robots_matches(pattern: &[u8], path: &[u8]) -> bool {
    let anchored = pattern.last() == Some(&b'$');
    let pattern = if anchored {
        &pattern[..pattern.len() - 1]
    } else {
        pattern
    };
    let (mut p, mut t) = (0, 0);
    let mut star = None;
    let mut retry = 0;
    while t < path.len() {
        if p == pattern.len() && !anchored {
            return true;
        }
        if pattern.get(p) == Some(&b'*') {
            star = Some(p);
            p += 1;
            retry = t;
        } else if pattern.get(p) == path.get(t) {
            p += 1;
            t += 1;
        } else if let Some(index) = star {
            retry += 1;
            t = retry;
            p = index + 1;
        } else {
            return false;
        }
    }
    while pattern.get(p) == Some(&b'*') {
        p += 1;
    }
    p == pattern.len()
}

/// Depth = number of non-empty path segments (alpha's `getURLDepth`).
pub fn url_depth(path: &str) -> usize {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .count()
}

/// The crawl-frontier admission check (pure).
pub struct Frontier<'a> {
    policy: CrawlPolicy,
    seed_origin: Option<String>,
    robots: Option<&'a RobotsRules>,
    robots_path: fn(&str) -> String,
    admitted: Vec<String>,
}

impl<'a> Frontier<'a> {
    /// Create a frontier over a seed URL's origin.
    pub fn new(seed: &str, policy: CrawlPolicy, robots: Option<&'a RobotsRules>) -> Self {
        let seed_origin = url::Url::parse(seed)
            .ok()
            .map(|u| u.origin().ascii_serialization());
        Self {
            policy: policy.clamped(),
            seed_origin,
            robots,
            robots_path: default_path,
            admitted: Vec::new(),
        }
    }

    /// Replace the robots path extractor (used when the transport layer
    /// supplies a pre-normalized path).
    pub fn with_path_extractor(mut self, f: fn(&str) -> String) -> Self {
        self.robots_path = f;
        self
    }

    /// Admit one candidate URL (already canonicalized).
    pub fn admit(&mut self, candidate: &str) -> Admission {
        let Ok(parsed) = url::Url::parse(candidate) else {
            return Admission::Denied(CrawlDenial::UrlParseError);
        };
        if !matches!(parsed.scheme(), "http" | "https") {
            return Admission::Denied(CrawlDenial::NonWebProtocol);
        }
        if self.admitted.len() >= self.policy.max_urls {
            return Admission::Denied(CrawlDenial::BudgetExhausted);
        }
        let normalized = normalize_url(&parsed);
        if self.admitted.iter().any(|u| u == &normalized) {
            return Admission::Denied(CrawlDenial::DuplicateNormalized);
        }
        if self.policy.same_origin {
            let origin = parsed.origin().ascii_serialization();
            if Some(&origin) != self.seed_origin.as_ref() {
                return Admission::Denied(CrawlDenial::OffOrigin);
            }
        }
        let depth = url_depth(parsed.path());
        if depth > self.policy.max_depth {
            return Admission::Denied(CrawlDenial::DepthLimit {
                depth,
                max_depth: self.policy.max_depth,
            });
        }
        if self.policy.respect_robots
            && let Some(robots) = self.robots
        {
            let path = (self.robots_path)(parsed.as_str());
            if !robots.is_allowed(&path) {
                return Admission::Denied(CrawlDenial::RobotsDisallowed);
            }
        }
        self.admitted.push(normalized.clone());
        Admission::Admitted { normalized }
    }

    /// URLs admitted so far.
    pub fn admitted(&self) -> &[String] {
        &self.admitted
    }
}

fn default_path(url: &str) -> String {
    url::Url::parse(url)
        .map(|u| {
            if u.query().is_some() {
                format!("{}?{}", u.path(), u.query().unwrap_or_default())
            } else {
                u.path().to_string()
            }
        })
        .unwrap_or_else(|_| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robots_normalizes_octets_without_decoding_reserved_slashes() {
        let rules = RobotsRules::parse(
            "User-agent: *\nDisallow: /caf%C3%A9\nDisallow: /~private\nDisallow: /a%2fb",
            "agent-vesper",
        );
        assert!(!rules.is_allowed("/café"));
        assert!(!rules.is_allowed("/%7eprivate"));
        assert!(!rules.is_allowed("/a%2Fb"));
        assert!(rules.is_allowed("/a/b"));
    }

    #[test]
    fn robots_combines_agent_groups_and_prefers_specific_rules() {
        let rules = RobotsRules::parse(
            "User-agent: *\nDisallow: /\nUser-agent: agent-vesper\nUser-agent: other\nDisallow: /private\nUser-agent: agent-vesper\nAllow: /private/public\n",
            "agent-vesper",
        );
        assert!(rules.is_allowed("/public"));
        assert!(!rules.is_allowed("/private/secret"));
        assert!(rules.is_allowed("/private/public"));
    }

    #[test]
    fn robots_wildcards_end_anchor_and_allow_ties() {
        let rules = RobotsRules::parse(
            "User-agent: *\nDisallow: /*.pdf$\nAllow: /same\nDisallow: /same\n",
            "agent-vesper",
        );
        assert!(!rules.is_allowed("/nested/file.pdf"));
        assert!(rules.is_allowed("/nested/file.pdf?download=1"));
        assert!(rules.is_allowed("/same"));
    }

    fn policy() -> CrawlPolicy {
        CrawlPolicy {
            max_urls: 3,
            max_depth: 2,
            concurrency: 2,
            same_origin: true,
            respect_robots: true,
        }
    }

    #[test]
    fn depth_counts_non_empty_segments() {
        assert_eq!(url_depth("/"), 0);
        assert_eq!(url_depth("/a"), 1);
        assert_eq!(url_depth("/a/b/c"), 3);
    }

    #[test]
    fn policy_clamps_to_ceilings() {
        let p = CrawlPolicy {
            max_urls: 10_000,
            max_depth: 99,
            concurrency: 0,
            ..CrawlPolicy::default()
        }
        .clamped();
        assert_eq!(p.max_urls, CrawlPolicy::MAX_URLS_CEILING);
        assert_eq!(p.max_depth, CrawlPolicy::MAX_DEPTH_CEILING);
        assert_eq!(p.concurrency, 1);
    }

    #[test]
    fn admits_within_bounds_and_denies_duplicates() {
        let mut f = Frontier::new("https://example.com/a", policy(), None);
        assert!(matches!(
            f.admit("https://example.com/a/b"),
            Admission::Admitted { .. }
        ));
        assert!(matches!(
            f.admit("https://example.com/a/b#frag"),
            Admission::Denied(CrawlDenial::DuplicateNormalized)
        ));
    }

    #[test]
    fn denies_off_origin_when_same_origin_set() {
        let mut f = Frontier::new("https://example.com/", policy(), None);
        assert!(matches!(
            f.admit("https://other.example.org/x"),
            Admission::Denied(CrawlDenial::OffOrigin)
        ));
    }

    #[test]
    fn denies_depth_over_limit_with_typed_reason() {
        let mut f = Frontier::new("https://example.com/", policy(), None);
        match f.admit("https://example.com/a/b/c") {
            Admission::Denied(CrawlDenial::DepthLimit { depth, max_depth }) => {
                assert_eq!((depth, max_depth), (3, 2));
            }
            other => panic!("expected depth denial, got {other:?}"),
        }
    }

    #[test]
    fn denies_budget_exhaustion_after_max_urls() {
        let mut f = Frontier::new("https://example.com/", policy(), None);
        for i in 0..3 {
            let url = format!("https://example.com/p{i}");
            assert!(matches!(f.admit(&url), Admission::Admitted { .. }));
        }
        assert!(matches!(
            f.admit("https://example.com/p9"),
            Admission::Denied(CrawlDenial::BudgetExhausted)
        ));
    }

    #[test]
    fn denies_non_web_protocols() {
        let mut f = Frontier::new("https://example.com/", policy(), None);
        assert!(matches!(
            f.admit("mailto:someone@example.com"),
            Admission::Denied(CrawlDenial::NonWebProtocol)
        ));
        assert!(matches!(
            f.admit("javascript:void(0)"),
            Admission::Denied(CrawlDenial::NonWebProtocol)
        ));
    }

    #[test]
    fn robots_disallow_prefix_blocks_path() {
        let robots = RobotsRules::parse(
            "User-agent: *\nDisallow: /private\nAllow: /private/public\n",
            "vesper",
        );
        assert!(!robots.is_allowed("/private/secret"));
        assert!(robots.is_allowed("/private/public/x"));
        assert!(robots.is_allowed("/open"));
    }

    #[test]
    fn robots_empty_disallow_allows_everything() {
        let robots = RobotsRules::parse("User-agent: *\nDisallow:\n", "vesper");
        assert!(robots.is_allowed("/anything"));
    }

    #[test]
    fn frontier_respects_robots_when_enabled() {
        let robots = RobotsRules::parse("User-agent: *\nDisallow: /private\n", "vesper");
        let mut f = Frontier::new("https://example.com/", policy(), Some(&robots));
        assert!(matches!(
            f.admit("https://example.com/private/x"),
            Admission::Denied(CrawlDenial::RobotsDisallowed)
        ));
        // Same policy without robots admits it.
        let mut g = Frontier::new(
            "https://example.com/",
            CrawlPolicy {
                respect_robots: false,
                ..policy()
            },
            Some(&robots),
        );
        assert!(matches!(
            g.admit("https://example.com/private/x"),
            Admission::Admitted { .. }
        ));
        // The path-extractor seam stays public for the transport-wiring
        // PR; prove it round-trips here.
        let mut g2 = f.with_path_extractor(default_path);
        assert!(matches!(
            g2.admit("https://example.com/"),
            Admission::Admitted { .. }
        ));
    }

    #[test]
    fn denial_names_are_stable() {
        assert_eq!(
            CrawlDenial::DepthLimit {
                depth: 3,
                max_depth: 2
            }
            .name(),
            "depth_limit"
        );
        assert_eq!(CrawlDenial::RobotsDisallowed.name(), "robots_disallowed");
        assert_eq!(CrawlDenial::OffOrigin.name(), "off_origin");
        assert_eq!(CrawlDenial::BudgetExhausted.name(), "budget_exhausted");
        assert_eq!(
            CrawlDenial::DuplicateNormalized.name(),
            "duplicate_normalized"
        );
        assert_eq!(CrawlDenial::NonWebProtocol.name(), "non_web_protocol");
        assert_eq!(CrawlDenial::UrlParseError.name(), "url_parse_error");
    }
}

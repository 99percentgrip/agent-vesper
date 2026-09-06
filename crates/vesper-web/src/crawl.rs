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
        let mut rules = Vec::new();
        let mut current_agents: Vec<String> = Vec::new();
        let mut in_scope = false;
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
                    if !current_agents.is_empty() {
                        // A rule block ended; reset scope.
                        current_agents.clear();
                    }
                    let value = value.to_ascii_lowercase();
                    in_scope = value == agent || value == "*";
                    if in_scope {
                        current_agents.push(value);
                    } else {
                        // Track group membership even when out of scope so
                        // the next agent line can close the block.
                        current_agents.push(value);
                        in_scope = false;
                    }
                }
                "allow" | "disallow" => {
                    if in_scope && !value.is_empty() {
                        rules.push((value.to_string(), key == "allow"));
                    } else if in_scope && value.is_empty() && key == "disallow" {
                        // `Disallow:` with an empty value allows everything.
                        rules.push((String::new(), true));
                    }
                }
                _ => {}
            }
        }
        Self { rules }
    }

    /// Longest-prefix match decides; empty rules allow everything.
    pub fn is_allowed(&self, path: &str) -> bool {
        self.rules
            .iter()
            .filter(|(prefix, _)| prefix.is_empty() || path.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len())
            .map(|(_, allowed)| *allowed)
            .unwrap_or(true)
    }
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

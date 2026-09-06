//! VRO-14 PR-3: the pure egress gate — every check that must clear
//! **before** a sandbox is provisioned for a fetch.
//!
//! No I/O: classification only. The gate answers "is this URL even
//! eligible for sandboxed fetching?" — private/loopback/link-local
//! addresses, non-web schemes, and (via the caller-supplied robots
//! verdict) disallowed paths are denied with typed reasons. DNS
//! resolution is deliberately NOT performed here: hostnames resolve
//! inside the sandbox at fetch time, and the helper enforces the same
//! address class against the *resolved* address (the gate catches the
//! literal-address case; the helper catches the DNS-rebinding case).

use std::net::IpAddr;

/// Egress policy knobs (all defaults deny-by-exception).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressPolicy {
    /// Permit `http:` in addition to `https:` (default false — TLS first).
    pub allow_plain_http: bool,
    /// Optional host allowlist; when non-empty, only listed hosts pass.
    pub allowed_hosts: Vec<String>,
    /// Respect robots.txt (default true; the caller supplies the verdict).
    pub respect_robots: bool,
}

impl Default for EgressPolicy {
    fn default() -> Self {
        Self {
            allow_plain_http: false,
            allowed_hosts: Vec::new(),
            respect_robots: true,
        }
    }
}

/// Typed egress verdicts (mirrors `CrawlDenial`'s discipline).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgressDenial {
    NonWebScheme,
    PlainHttpDisallowed,
    LoopbackAddress,
    PrivateAddress,
    LinkLocalAddress,
    UnspecifiedAddress,
    HostNotAllowlisted,
    RobotsDisallowed,
    UnparsableUrl,
}

impl EgressDenial {
    /// Stable machine name (test contract).
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::NonWebScheme => "non_web_scheme",
            Self::PlainHttpDisallowed => "plain_http_disallowed",
            Self::LoopbackAddress => "loopback_address",
            Self::PrivateAddress => "private_address",
            Self::LinkLocalAddress => "link_local_address",
            Self::UnspecifiedAddress => "unspecified_address",
            Self::HostNotAllowlisted => "host_not_allowlisted",
            Self::RobotsDisallowed => "robots_disallowed",
            Self::UnparsableUrl => "unparsable_url",
        }
    }
}

/// The verdict of one gate evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgressVerdict {
    Allow,
    Deny(EgressDenial),
}

/// Classify one IP address by egress safety.
#[must_use]
pub fn classify_ip(addr: &IpAddr) -> EgressVerdict {
    use std::net::IpAddr::*;
    match addr {
        V4(v4) => {
            let o = v4.octets();
            if o[0] == 127 {
                EgressVerdict::Deny(EgressDenial::LoopbackAddress)
            } else if o[0] == 10
                || (o[0] == 192 && o[1] == 168)
                || (o[0] == 172 && (16..=31).contains(&o[1]))
            {
                EgressVerdict::Deny(EgressDenial::PrivateAddress)
            } else if o[0] == 169 && o[1] == 254 {
                EgressVerdict::Deny(EgressDenial::LinkLocalAddress)
            } else if o[0] == 0 {
                EgressVerdict::Deny(EgressDenial::UnspecifiedAddress)
            } else {
                EgressVerdict::Allow
            }
        }
        V6(v6) => {
            // IPv4-mapped (::ffff:a.b.c.d) checks the embedded v4.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return classify_ip(&IpAddr::V4(v4));
            }
            if v6.is_loopback() {
                EgressVerdict::Deny(EgressDenial::LoopbackAddress)
            } else if v6.is_unspecified() {
                EgressVerdict::Deny(EgressDenial::UnspecifiedAddress)
            } else {
                EgressVerdict::Allow
            }
        }
    }
}

/// Evaluate the gate for one URL plus the caller's robots verdict.
///
/// Hostnames are lowercased and matched against the allowlist; literal IP
/// hosts are classified by [`classify_ip`]. DNS resolution happens inside
/// the sandbox; the fetch helper re-checks the resolved address.
#[must_use]
pub fn evaluate(url: &str, policy: &EgressPolicy, robots_allowed: Option<bool>) -> EgressVerdict {
    let Ok(parsed) = url::Url::parse(url) else {
        return EgressVerdict::Deny(EgressDenial::UnparsableUrl);
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return EgressVerdict::Deny(EgressDenial::NonWebScheme);
    }
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    if host.is_empty() {
        return EgressVerdict::Deny(EgressDenial::UnparsableUrl);
    }
    // Address-class denials outrank scheme policy: a loopback/private
    // probe over plain http reports as the address class it is, so the
    // caller sees the security-relevant denial, not the cosmetic one.
    // `url::Url::host_str` keeps v6 brackets ("[::1]"), so strip them
    // before the literal-IP classification.
    let bare_host = host.strip_prefix('[').unwrap_or(&host);
    let bare_host = bare_host.strip_suffix(']').unwrap_or(bare_host);
    if let Ok(addr) = bare_host.parse::<IpAddr>() {
        if let deny @ EgressVerdict::Deny(_) = classify_ip(&addr) {
            return deny;
        }
    } else if host == "localhost" {
        return EgressVerdict::Deny(EgressDenial::LoopbackAddress);
    }
    if parsed.scheme() == "http" && !policy.allow_plain_http {
        return EgressVerdict::Deny(EgressDenial::PlainHttpDisallowed);
    }
    if !policy.allowed_hosts.is_empty()
        && !policy
            .allowed_hosts
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(&host))
    {
        return EgressVerdict::Deny(EgressDenial::HostNotAllowlisted);
    }
    if policy.respect_robots && robots_allowed == Some(false) {
        return EgressVerdict::Deny(EgressDenial::RobotsDisallowed);
    }
    EgressVerdict::Allow
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> EgressPolicy {
        EgressPolicy {
            allow_plain_http: false,
            allowed_hosts: Vec::new(),
            respect_robots: true,
        }
    }

    #[test]
    fn loopback_v4_is_denied() {
        for url in [
            "http://127.0.0.1/x",
            "https://127.0.0.1:8080/",
            "http://127.8.8.8/", // whole 127/8
        ] {
            assert!(
                matches!(
                    evaluate(url, &policy(), None),
                    EgressVerdict::Deny(EgressDenial::LoopbackAddress)
                ),
                "{url} must be denied as loopback"
            );
        }
    }

    #[test]
    fn localhost_name_is_denied() {
        assert!(matches!(
            evaluate("http://localhost/admin", &policy(), None),
            EgressVerdict::Deny(EgressDenial::LoopbackAddress)
        ));
    }

    #[test]
    fn private_ranges_are_denied() {
        for url in [
            "http://10.0.0.1/",
            "http://10.255.255.255/",
            "http://192.168.1.1/router",
            "http://172.16.0.1/",
            "http://172.31.255.254/",
        ] {
            assert!(
                matches!(
                    evaluate(url, &policy(), None),
                    EgressVerdict::Deny(EgressDenial::PrivateAddress)
                ),
                "{url} must be denied as private"
            );
        }
    }

    #[test]
    fn link_local_and_unspecified_are_denied() {
        assert!(matches!(
            evaluate("http://169.254.169.254/latest/meta-data", &policy(), None),
            EgressVerdict::Deny(EgressDenial::LinkLocalAddress)
        ));
        assert!(matches!(
            evaluate("http://0.0.0.0/", &policy(), None),
            EgressVerdict::Deny(EgressDenial::UnspecifiedAddress)
        ));
    }

    #[test]
    fn v6_loopback_and_mapped_v4_are_denied() {
        assert!(matches!(
            evaluate("http://[::1]/x", &policy(), None),
            EgressVerdict::Deny(EgressDenial::LoopbackAddress)
        ));
        assert!(matches!(
            evaluate("http://[::ffff:127.0.0.1]/x", &policy(), None),
            EgressVerdict::Deny(EgressDenial::LoopbackAddress)
        ));
        assert!(matches!(
            evaluate("http://[::ffff:192.168.0.1]/x", &policy(), None),
            EgressVerdict::Deny(EgressDenial::PrivateAddress)
        ));
    }

    #[test]
    fn public_https_is_allowed() {
        assert_eq!(
            evaluate("https://example.com/page", &policy(), None),
            EgressVerdict::Allow
        );
        assert_eq!(
            evaluate("https://93.184.216.34/", &policy(), None),
            EgressVerdict::Allow
        );
    }

    #[test]
    fn plain_http_denied_unless_enabled() {
        assert!(matches!(
            evaluate("http://example.com/", &policy(), None),
            EgressVerdict::Deny(EgressDenial::PlainHttpDisallowed)
        ));
        let mut http_ok = policy();
        http_ok.allow_plain_http = true;
        assert_eq!(
            evaluate("http://example.com/", &http_ok, None),
            EgressVerdict::Allow
        );
    }

    #[test]
    fn non_web_schemes_are_denied() {
        for url in ["file:///etc/passwd", "ftp://x/y", "mailto:a@b.c"] {
            assert!(
                matches!(
                    evaluate(url, &policy(), None),
                    EgressVerdict::Deny(EgressDenial::NonWebScheme)
                ),
                "{url} must be denied as non-web"
            );
        }
    }

    #[test]
    fn robots_verdict_denies_when_disallowed() {
        assert!(matches!(
            evaluate("https://example.com/private", &policy(), Some(false)),
            EgressVerdict::Deny(EgressDenial::RobotsDisallowed)
        ));
        // Unknown robots state does not block (the caller fetches robots
        // separately; a missing robots.txt means allowed).
        assert_eq!(
            evaluate("https://example.com/private", &policy(), None),
            EgressVerdict::Allow
        );
        assert_eq!(
            evaluate("https://example.com/private", &policy(), Some(true)),
            EgressVerdict::Allow
        );
    }

    #[test]
    fn allowlist_gates_hosts() {
        let mut scoped = policy();
        scoped.allowed_hosts = vec!["docs.example.com".to_string()];
        assert_eq!(
            evaluate("https://docs.example.com/x", &scoped, None),
            EgressVerdict::Allow
        );
        assert!(matches!(
            evaluate("https://other.example.org/x", &scoped, None),
            EgressVerdict::Deny(EgressDenial::HostNotAllowlisted)
        ));
    }

    #[test]
    fn denial_names_are_stable() {
        assert_eq!(EgressDenial::LoopbackAddress.name(), "loopback_address");
        assert_eq!(EgressDenial::PrivateAddress.name(), "private_address");
        assert_eq!(EgressDenial::RobotsDisallowed.name(), "robots_disallowed");
        assert_eq!(EgressDenial::LinkLocalAddress.name(), "link_local_address");
    }
}

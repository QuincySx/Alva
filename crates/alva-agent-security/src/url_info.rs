// INPUT:  std::net (IpAddr / Ipv4Addr / Ipv6Addr)
// OUTPUT: IpClass, UrlRisk, classify_ip, ip_class_to_risk
// POS:    Pure IP/URL classification primitives for the SSRF defense
//         (T6 fix, 3C path: SecurityGuard → HITL approval).
//
//! url_info — pure URL/IP classification primitives
//!
//! Inputs: `std::net::IpAddr`.
//! Outputs: `IpClass` (what kind of network role the address has) and
//! `UrlRisk` (whether to ask the user before fetching).
//!
//! No I/O, no DNS, no async. DNS lookup + URL parsing happens in a later
//! Loop (B); this file is the pure classifier used by both.
//!
//! Risk mapping is **hard-coded developer judgment**: the user controls
//! the ask-threshold elsewhere (`UrlRules::ask_threshold`, Loop B), not
//! this map. If you find yourself wanting per-class user configurability,
//! push back — that's a footgun for security defaults.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::Url;

/// Classification of an IP address by its network role.
///
/// Do not rely on enum discriminant values for persistence — the set may
/// grow as we add more specific categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpClass {
    /// 127.0.0.0/8 (IPv4) or `::1` (IPv6). Local machine.
    Loopback,
    /// 169.254.0.0/16 (IPv4, contains AWS IMDS at 169.254.169.254) or
    /// fe80::/10 (IPv6). Auto-configured / metadata-service range.
    LinkLocal,
    /// RFC1918 IPv4: 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16.
    /// Private intranet ranges.
    Private,
    /// RFC4193: fc00::/7. IPv6 unique local addresses (private intranet).
    UniqueLocal,
    /// 224.0.0.0/4 (IPv4) or ff00::/8 (IPv6).
    Multicast,
    /// 255.255.255.255 (IPv4 limited broadcast).
    Broadcast,
    /// 0.0.0.0 (IPv4) or `::` (IPv6).
    Unspecified,
    /// Anything not matched above — globally routable internet address.
    Public,
}

/// Risk level for an outbound URL fetch.
///
/// `PartialOrd` + `Ord` are derived so callers can write
/// `if info.risk >= user_threshold`. The enum declaration order IS the
/// risk ordering — do not reorder variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UrlRisk {
    /// Routable public internet — normal web access.
    Low,
    /// Private intranet (RFC1918 / UniqueLocal). Agent may legitimately
    /// access this (debugging local services) or be coerced into internal
    /// network lateral movement. User decides per request.
    Medium,
    /// Loopback (localhost service abuse) / LinkLocal (AWS IMDS credential
    /// theft) / multicast / broadcast / unspecified / DNS resolution
    /// failure. Strong default-ask.
    High,
}

/// Classify an IP address into an [`IpClass`]. Pure function.
pub fn classify_ip(ip: IpAddr) -> IpClass {
    match ip {
        IpAddr::V4(v4) => classify_ipv4(v4),
        IpAddr::V6(v6) => classify_ipv6(v6),
    }
}

fn classify_ipv4(v4: Ipv4Addr) -> IpClass {
    // Order: most-specific first. The std `is_*` predicates are
    // disjoint for our cases (link-local is NOT private, etc.) so any
    // order would also work, but this reads as "check the dangerous
    // ones early."
    if v4.is_unspecified() {
        return IpClass::Unspecified;
    }
    if v4.is_broadcast() {
        return IpClass::Broadcast;
    }
    if v4.is_loopback() {
        return IpClass::Loopback;
    }
    if v4.is_link_local() {
        // 169.254.0.0/16 — includes AWS/GCP/Azure IMDS at 169.254.169.254
        return IpClass::LinkLocal;
    }
    if v4.is_private() {
        // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
        return IpClass::Private;
    }
    if v4.is_multicast() {
        return IpClass::Multicast;
    }
    IpClass::Public
}

fn classify_ipv6(v6: Ipv6Addr) -> IpClass {
    if v6.is_unspecified() {
        return IpClass::Unspecified;
    }
    if v6.is_loopback() {
        return IpClass::Loopback;
    }
    if v6.is_multicast() {
        return IpClass::Multicast;
    }
    // fe80::/10 link-local. `Ipv6Addr::is_unicast_link_local` is unstable,
    // so we check the prefix manually.
    let segments = v6.segments();
    if (segments[0] & 0xffc0) == 0xfe80 {
        return IpClass::LinkLocal;
    }
    // fc00::/7 unique local. `Ipv6Addr::is_unique_local` is also unstable.
    if (segments[0] & 0xfe00) == 0xfc00 {
        return IpClass::UniqueLocal;
    }
    IpClass::Public
}

/// Map an [`IpClass`] to a default [`UrlRisk`].
///
/// This mapping is the developer's judgment call — see the module doc.
pub const fn ip_class_to_risk(class: IpClass) -> UrlRisk {
    match class {
        IpClass::Public => UrlRisk::Low,
        IpClass::Private | IpClass::UniqueLocal => UrlRisk::Medium,
        IpClass::Loopback
        | IpClass::LinkLocal
        | IpClass::Multicast
        | IpClass::Broadcast
        | IpClass::Unspecified => UrlRisk::High,
    }
}

// ─────────────────────────────────────────────────────────────────────
// Loop B: URL inspection + user-tunable threshold
// ─────────────────────────────────────────────────────────────────────

/// AWS / GCP / Azure all use this address for instance metadata.
/// Worth calling out explicitly in the user-facing risk summary because
/// it's the canonical cloud-credential-leak target.
const IMDS_ADDR: Ipv4Addr = Ipv4Addr::new(169, 254, 169, 254);

/// Result of inspecting a URL before fetching.
///
/// Built by [`inspect_url`]. Tools use this to decide whether to request
/// HITL approval — they compare `risk` against `UrlRules::ask_threshold`.
#[derive(Debug, Clone)]
pub struct UrlInfo {
    /// The URL as the caller provided it.
    pub url: String,
    /// Hostname or IP literal extracted from the URL. Empty if parsing
    /// failed and we couldn't recover anything.
    pub host: String,
    /// Explicit port if the URL specified one (not the scheme default).
    pub port: Option<u16>,
    /// `http` / `https` / etc. Empty on parse failure.
    pub scheme: String,
    /// IPs the host resolved to. Empty if:
    ///   - URL failed to parse, or
    ///   - host had no resolvable A/AAAA records, or
    ///   - DNS lookup errored.
    pub resolved_ips: Vec<IpAddr>,
    /// Worst-case (highest-risk) classification among `resolved_ips`,
    /// or `None` if `resolved_ips` is empty.
    pub ip_class: Option<IpClass>,
    /// Final risk verdict — what callers compare against the threshold.
    /// If `ip_class` is `None`, this is `UrlRisk::High` (can't verify is
    /// risky-by-default).
    pub risk: UrlRisk,
}

impl UrlInfo {
    /// Human-readable one-line summary of why this URL got its risk
    /// rating. Shown in the HITL approval dialog so the user has context
    /// for the Allow/Deny decision.
    pub fn risk_summary(&self) -> String {
        // Treat the first resolved IP as representative for the display
        // string. `resolved_ips` is sorted-by-arrival from the DNS
        // resolver; for risk we already took the worst-case, but for
        // *display* the first one is usually what the user expects to see.
        let primary_ip = self.resolved_ips.first().copied();

        match (self.risk, self.ip_class, primary_ip) {
            // Loopback / IMDS are the two cases worth distinct messaging
            (UrlRisk::High, Some(IpClass::LinkLocal), Some(IpAddr::V4(v4))) if v4 == IMDS_ADDR => {
                format!(
                    "Cloud Instance Metadata Service ({IMDS_ADDR}) — \
                     can leak IAM/cloud credentials"
                )
            }
            (UrlRisk::High, Some(IpClass::LinkLocal), Some(ip)) => {
                format!("Link-local address ({ip})")
            }
            (UrlRisk::High, Some(IpClass::Loopback), Some(ip)) => {
                format!("Localhost ({ip})")
            }
            (UrlRisk::High, Some(IpClass::Multicast), Some(ip)) => {
                format!("Multicast address ({ip})")
            }
            (UrlRisk::High, Some(IpClass::Broadcast), Some(ip)) => {
                format!("Broadcast address ({ip})")
            }
            (UrlRisk::High, Some(IpClass::Unspecified), Some(ip)) => {
                format!("Unspecified address ({ip})")
            }
            (UrlRisk::High, None, _) => {
                format!("Host did not resolve via DNS ({})", self.host)
            }

            (UrlRisk::Medium, Some(IpClass::Private), Some(ip)) => {
                format!("Private intranet IP ({ip}) — RFC1918")
            }
            (UrlRisk::Medium, Some(IpClass::UniqueLocal), Some(ip)) => {
                format!("Private IPv6 ({ip}) — Unique Local")
            }

            (UrlRisk::Low, _, _) => {
                format!("Public website ({})", self.host)
            }

            // Catch-all — shouldn't be reached for known IpClass/risk
            // combos, but lets us add new IpClass variants without
            // having to revisit this match.
            _ => format!("Address requires review ({})", self.host),
        }
    }
}

/// User-tunable URL policy.
///
/// Single knob: `ask_threshold`.
/// - `Some(level)` — ask for HITL when `risk >= level`. Default
///   `Some(Medium)` asks for Private/Loopback/LinkLocal/DNS-fail and
///   lets Public through.
/// - `None` — never ask (trust everything). Use only when SSRF is not
///   a concern for the deployment, or for tests.
#[derive(Debug, Clone)]
pub struct UrlRules {
    pub ask_threshold: Option<UrlRisk>,
}

impl Default for UrlRules {
    fn default() -> Self {
        Self {
            ask_threshold: Some(UrlRisk::Medium),
        }
    }
}

impl UrlRules {
    /// Should the given risk level trigger an HITL approval prompt
    /// under these rules?
    pub fn should_ask(&self, risk: UrlRisk) -> bool {
        match self.ask_threshold {
            Some(threshold) => risk >= threshold,
            None => false,
        }
    }
}

/// Inspect a URL: parse it, resolve its host to IP(s), classify the
/// worst-case IP, and bundle everything into a [`UrlInfo`].
///
/// On any error (parse fail, no host, DNS error, no records), returns
/// a `UrlInfo` with `ip_class = None` and `risk = High` — the caller
/// then asks the user.
///
/// No DNS rebinding defense — see the SECURITY note in [`UrlInfo`].
/// We accept the TOCTOU window because this layer is information-
/// gathering for HITL, not a hard block.
pub async fn inspect_url(url: &str) -> UrlInfo {
    // Default "we couldn't tell" result — used on every error path.
    let unresolved = |scheme: String, host: String, port: Option<u16>| UrlInfo {
        url: url.to_string(),
        host,
        port,
        scheme,
        resolved_ips: Vec::new(),
        ip_class: None,
        risk: UrlRisk::High,
    };

    let parsed = match Url::parse(url) {
        Ok(p) => p,
        Err(_) => return unresolved(String::new(), String::new(), None),
    };

    let scheme = parsed.scheme().to_string();
    let host_str = match parsed.host_str() {
        Some(h) => h.to_string(),
        None => return unresolved(scheme, String::new(), None),
    };
    let port = parsed.port();

    // If the host is itself an IP literal, skip DNS — `url::Url` lower-
    // cases hostnames but leaves IPv6 in `[...]` brackets; `host_str`
    // returns it without brackets, so std parsing works directly.
    if let Ok(ip) = host_str.parse::<IpAddr>() {
        let class = classify_ip(ip);
        return UrlInfo {
            url: url.to_string(),
            host: host_str,
            port,
            scheme,
            resolved_ips: vec![ip],
            ip_class: Some(class),
            risk: ip_class_to_risk(class),
        };
    }

    // Need DNS. lookup_host requires a host:port string; pick a sane
    // port if the URL didn't specify one (the actual port doesn't
    // matter for A/AAAA records, but lookup_host insists).
    let lookup_target = format!("{}:{}", host_str, port.unwrap_or(80));
    #[cfg(target_family = "wasm")]
    {
        let _ = lookup_target;
        unresolved(scheme, host_str, port)
    }

    #[cfg(not(target_family = "wasm"))]
    {
        let resolved_ips: Vec<IpAddr> = match tokio::net::lookup_host(&lookup_target).await {
            Ok(addrs) => addrs.map(|sa| sa.ip()).collect(),
            Err(_) => return unresolved(scheme, host_str, port),
        };

        if resolved_ips.is_empty() {
            return unresolved(scheme, host_str, port);
        }

        // Take the WORST-CASE class across resolved IPs. Rationale: an
        // attacker can return multiple A records mixing public + private;
        // we ask if ANY of them is risky. (DNS rebinding still wins this
        // race at fetch time — that's the documented TOCTOU limit.)
        let worst_class = resolved_ips
            .iter()
            .map(|ip| classify_ip(*ip))
            .max_by_key(|c| ip_class_to_risk(*c))
            .expect("non-empty checked above");

        UrlInfo {
            url: url.to_string(),
            host: host_str,
            port,
            scheme,
            resolved_ips,
            ip_class: Some(worst_class),
            risk: ip_class_to_risk(worst_class),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn ip(s: &str) -> IpAddr {
        IpAddr::from_str(s).expect("test IP must parse")
    }

    // ─── IPv4 classification ──────────────────────────────────────────

    #[test]
    fn loopback_ipv4_covers_127_0_0_0_8() {
        for s in ["127.0.0.1", "127.1.2.3", "127.255.255.254"] {
            assert_eq!(classify_ip(ip(s)), IpClass::Loopback, "{s}");
        }
    }

    #[test]
    fn link_local_ipv4_covers_imds_address() {
        // AWS / GCP / Azure all use 169.254.169.254 for instance metadata —
        // the canonical SSRF target. Must classify as LinkLocal (→ High risk).
        assert_eq!(
            classify_ip(ip("169.254.169.254")),
            IpClass::LinkLocal,
            "AWS IMDS must classify as LinkLocal"
        );
        // Edges of 169.254.0.0/16
        for s in ["169.254.0.1", "169.254.255.254"] {
            assert_eq!(classify_ip(ip(s)), IpClass::LinkLocal, "{s}");
        }
    }

    #[test]
    fn rfc1918_private_ipv4_all_three_ranges() {
        // 10.0.0.0/8
        assert_eq!(classify_ip(ip("10.0.0.1")), IpClass::Private);
        assert_eq!(classify_ip(ip("10.255.255.255")), IpClass::Private);
        // 172.16.0.0/12 — only 172.16 through 172.31
        assert_eq!(classify_ip(ip("172.16.0.1")), IpClass::Private);
        assert_eq!(classify_ip(ip("172.31.255.255")), IpClass::Private);
        // 172.32.0.0 is OUTSIDE RFC1918 — must classify as Public
        assert_eq!(
            classify_ip(ip("172.32.0.1")),
            IpClass::Public,
            "172.32 is outside RFC1918 — should be Public"
        );
        // 192.168.0.0/16
        assert_eq!(classify_ip(ip("192.168.0.1")), IpClass::Private);
        assert_eq!(classify_ip(ip("192.168.255.255")), IpClass::Private);
    }

    #[test]
    fn multicast_ipv4_covers_224_0_0_0_4() {
        for s in ["224.0.0.1", "239.255.255.255"] {
            assert_eq!(classify_ip(ip(s)), IpClass::Multicast, "{s}");
        }
    }

    #[test]
    fn broadcast_ipv4() {
        assert_eq!(classify_ip(ip("255.255.255.255")), IpClass::Broadcast);
    }

    #[test]
    fn unspecified_ipv4() {
        assert_eq!(classify_ip(ip("0.0.0.0")), IpClass::Unspecified);
    }

    #[test]
    fn public_ipv4_examples() {
        // 8.8.8.8 = Google DNS, 1.1.1.1 = Cloudflare DNS, 140.82.114.4 = GitHub
        for s in ["8.8.8.8", "1.1.1.1", "140.82.114.4"] {
            assert_eq!(
                classify_ip(ip(s)),
                IpClass::Public,
                "{s} should classify as Public"
            );
        }
    }

    // ─── IPv6 classification ──────────────────────────────────────────

    #[test]
    fn loopback_ipv6() {
        assert_eq!(classify_ip(ip("::1")), IpClass::Loopback);
    }

    #[test]
    fn unspecified_ipv6() {
        assert_eq!(classify_ip(ip("::")), IpClass::Unspecified);
    }

    #[test]
    fn link_local_ipv6_fe80_slash_10() {
        // fe80::/10 — covers fe80:: through febf::
        assert_eq!(classify_ip(ip("fe80::1")), IpClass::LinkLocal);
        assert_eq!(classify_ip(ip("fe80::dead:beef")), IpClass::LinkLocal);
        assert_eq!(classify_ip(ip("febf::1")), IpClass::LinkLocal);
        // fec0:: is OUTSIDE fe80::/10 (historically site-local, now
        // deprecated). Must NOT classify as LinkLocal.
        assert_ne!(
            classify_ip(ip("fec0::1")),
            IpClass::LinkLocal,
            "fec0:: is outside fe80::/10 — should not be LinkLocal"
        );
    }

    #[test]
    fn unique_local_ipv6_fc00_slash_7() {
        // fc00::/7 — covers fc00:: and fd00::
        assert_eq!(classify_ip(ip("fc00::1")), IpClass::UniqueLocal);
        assert_eq!(classify_ip(ip("fd00::1")), IpClass::UniqueLocal);
        // fe00:: is OUTSIDE fc00::/7 — must NOT classify as UniqueLocal
        assert_ne!(
            classify_ip(ip("fe00::1")),
            IpClass::UniqueLocal,
            "fe00:: is outside fc00::/7 — should not be UniqueLocal"
        );
    }

    #[test]
    fn multicast_ipv6_ff00_slash_8() {
        assert_eq!(classify_ip(ip("ff02::1")), IpClass::Multicast);
        assert_eq!(classify_ip(ip("ff0e::1")), IpClass::Multicast);
    }

    #[test]
    fn public_ipv6_example() {
        // 2001:4860:4860::8888 = Google Public DNS over IPv6
        assert_eq!(classify_ip(ip("2001:4860:4860::8888")), IpClass::Public,);
    }

    // ─── ip_class_to_risk mapping ─────────────────────────────────────

    #[test]
    fn risk_mapping_public_is_low() {
        assert_eq!(ip_class_to_risk(IpClass::Public), UrlRisk::Low);
    }

    #[test]
    fn risk_mapping_private_and_unique_local_are_medium() {
        assert_eq!(ip_class_to_risk(IpClass::Private), UrlRisk::Medium);
        assert_eq!(ip_class_to_risk(IpClass::UniqueLocal), UrlRisk::Medium);
    }

    #[test]
    fn risk_mapping_dangerous_classes_are_high() {
        for class in [
            IpClass::Loopback,
            IpClass::LinkLocal,
            IpClass::Multicast,
            IpClass::Broadcast,
            IpClass::Unspecified,
        ] {
            assert_eq!(
                ip_class_to_risk(class),
                UrlRisk::High,
                "{class:?} must be High risk"
            );
        }
    }

    #[test]
    fn risk_is_orderable_for_threshold_comparison() {
        // Callers will write `if info.risk >= ask_threshold` (Loop B/C/D).
        // This test pins the ordering against accidental reordering of
        // the enum variants, which would silently flip security defaults.
        assert!(UrlRisk::High > UrlRisk::Medium);
        assert!(UrlRisk::Medium > UrlRisk::Low);
        assert!(UrlRisk::Low < UrlRisk::High);
        assert!(UrlRisk::Medium >= UrlRisk::Low);
        assert!(UrlRisk::High >= UrlRisk::High);
    }

    // ─── UrlRules.should_ask ──────────────────────────────────────────

    #[test]
    fn url_rules_default_threshold_is_some_medium() {
        let r = UrlRules::default();
        assert_eq!(r.ask_threshold, Some(UrlRisk::Medium));
    }

    #[test]
    fn url_rules_should_ask_respects_threshold() {
        // Default Some(Medium): ask for Medium + High, allow Low
        let r = UrlRules::default();
        assert!(
            !r.should_ask(UrlRisk::Low),
            "Low must NOT ask under default"
        );
        assert!(
            r.should_ask(UrlRisk::Medium),
            "Medium must ask under default"
        );
        assert!(r.should_ask(UrlRisk::High), "High must ask under default");

        // Some(High): only ask for High (loopback / IMDS / DNS-fail);
        // Private intranet auto-passes. "Dev-friendly" setting.
        let lenient = UrlRules {
            ask_threshold: Some(UrlRisk::High),
        };
        assert!(!lenient.should_ask(UrlRisk::Low));
        assert!(
            !lenient.should_ask(UrlRisk::Medium),
            "Medium passes when threshold=High"
        );
        assert!(lenient.should_ask(UrlRisk::High));

        // Some(Low): ask for everything (paranoid mode)
        let paranoid = UrlRules {
            ask_threshold: Some(UrlRisk::Low),
        };
        assert!(
            paranoid.should_ask(UrlRisk::Low),
            "Low asks when threshold=Low"
        );
        assert!(paranoid.should_ask(UrlRisk::Medium));
        assert!(paranoid.should_ask(UrlRisk::High));

        // None: trust everything (never ask)
        let trusting = UrlRules {
            ask_threshold: None,
        };
        assert!(!trusting.should_ask(UrlRisk::Low));
        assert!(!trusting.should_ask(UrlRisk::Medium));
        assert!(
            !trusting.should_ask(UrlRisk::High),
            "None must never ask even for High"
        );
    }

    // ─── inspect_url: IP-literal paths (no DNS needed) ────────────────

    #[tokio::test]
    async fn inspect_url_imds_address_is_high_with_credential_warning() {
        let info = inspect_url("http://169.254.169.254/latest/meta-data/").await;
        assert_eq!(info.scheme, "http");
        assert_eq!(info.host, "169.254.169.254");
        assert_eq!(info.ip_class, Some(IpClass::LinkLocal));
        assert_eq!(info.risk, UrlRisk::High);
        // The summary must call out the cloud credential risk so the
        // user-facing approval dialog has enough context.
        let summary = info.risk_summary();
        assert!(
            summary.contains("Metadata") && summary.contains("credential"),
            "IMDS summary must call out credential risk: {summary}"
        );
    }

    #[tokio::test]
    async fn inspect_url_private_ip_is_medium() {
        let info = inspect_url("http://192.168.1.1/admin").await;
        assert_eq!(info.ip_class, Some(IpClass::Private));
        assert_eq!(info.risk, UrlRisk::Medium);
        let s = info.risk_summary();
        assert!(
            s.contains("Private") && s.contains("RFC1918"),
            "summary missing context: {s}"
        );
    }

    #[tokio::test]
    async fn inspect_url_loopback_ipv4_is_high() {
        let info = inspect_url("http://127.0.0.1:8080/").await;
        assert_eq!(info.ip_class, Some(IpClass::Loopback));
        assert_eq!(info.risk, UrlRisk::High);
        assert_eq!(info.port, Some(8080), "explicit port must round-trip");
        assert!(info.risk_summary().contains("Localhost"));
    }

    #[tokio::test]
    async fn inspect_url_public_ipv4_is_low() {
        // 8.8.8.8 = Google DNS, a literal public IP — no DNS needed
        let info = inspect_url("https://8.8.8.8/").await;
        assert_eq!(info.scheme, "https");
        assert_eq!(info.ip_class, Some(IpClass::Public));
        assert_eq!(info.risk, UrlRisk::Low);
        assert!(info.risk_summary().contains("Public"));
    }

    #[tokio::test]
    async fn inspect_url_ipv6_loopback_is_high() {
        // url crate accepts bracketed IPv6 literals
        let info = inspect_url("http://[::1]/").await;
        assert_eq!(info.ip_class, Some(IpClass::Loopback));
        assert_eq!(info.risk, UrlRisk::High);
    }

    // ─── inspect_url: error paths ─────────────────────────────────────

    #[tokio::test]
    async fn inspect_url_parse_failure_returns_high_unresolved() {
        // Not a valid URL — must NOT panic, must return High
        let info = inspect_url("this is not a url").await;
        assert!(info.resolved_ips.is_empty());
        assert_eq!(info.ip_class, None);
        assert_eq!(
            info.risk,
            UrlRisk::High,
            "unparseable URL must be treated as High"
        );
        assert!(info.risk_summary().contains("did not resolve"));
    }

    #[tokio::test]
    async fn inspect_url_dns_failure_returns_high_unresolved() {
        // `.invalid` is reserved by RFC 2606 — guaranteed not to resolve
        let info = inspect_url("http://does-not-exist.invalid/").await;
        assert!(info.resolved_ips.is_empty());
        assert_eq!(info.ip_class, None);
        assert_eq!(info.risk, UrlRisk::High);
        assert_eq!(info.host, "does-not-exist.invalid");
        let s = info.risk_summary();
        assert!(
            s.contains("did not resolve"),
            "DNS-fail summary should mention resolution: {s}"
        );
    }

    // ─── inspect_url: DNS resolution path (uses localhost) ────────────

    #[tokio::test]
    async fn inspect_url_localhost_resolves_and_classifies_as_loopback() {
        // localhost is universally guaranteed to resolve to 127.0.0.1
        // and/or ::1 — both are Loopback, so the worst-case is still
        // Loopback. This test verifies the DNS branch is reached and
        // wired correctly (any test failure here means lookup_host /
        // tokio net feature is broken in this build).
        let info = inspect_url("http://localhost:9999/").await;
        assert!(!info.resolved_ips.is_empty(), "localhost MUST resolve");
        for ip in &info.resolved_ips {
            assert!(
                ip.is_loopback(),
                "every resolved IP of localhost must be loopback, got {ip}"
            );
        }
        assert_eq!(info.ip_class, Some(IpClass::Loopback));
        assert_eq!(info.risk, UrlRisk::High);
        assert_eq!(info.host, "localhost");
        assert_eq!(info.port, Some(9999));
    }
}

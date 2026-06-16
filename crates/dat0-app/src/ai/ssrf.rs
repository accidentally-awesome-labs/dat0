//! SSRF validation for the Custom provider's base URL (R17). Applies only to
//! Custom; the other three providers use fixed hardcoded https URLs.

use std::net::{IpAddr, Ipv6Addr};
use url::{Host, Url};

#[derive(Debug, thiserror::Error)]
pub enum SsrfError {
    #[error("only https is allowed (enable advanced override for http/local)")]
    NotHttps,
    #[error("blocked host: {0}")]
    BlockedHost(String),
    #[error("invalid url: {0}")]
    InvalidUrl(String),
}

pub struct ValidatedUrl(pub Url);

pub fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
        }
        IpAddr::V6(v6) => is_blocked_ipv6(v6),
    }
}

fn is_blocked_ipv6(v6: Ipv6Addr) -> bool {
    if v6.is_loopback() || v6.is_unspecified() {
        return true;
    }
    // IPv4-mapped (::ffff:a.b.c.d) AND IPv4-compatible (::a.b.c.d) — re-check the embedded v4.
    if let Some(v4) = v6.to_ipv4() {
        return is_blocked_ip(IpAddr::V4(v4));
    }
    let s0 = v6.segments()[0];
    (s0 & 0xfe00) == 0xfc00 // ULA fc00::/7
        || (s0 & 0xffc0) == 0xfe80 // link-local fe80::/10
}

pub fn validate_url(raw: &str, advanced_override: bool) -> Result<ValidatedUrl, SsrfError> {
    let url = Url::parse(raw).map_err(|e| SsrfError::InvalidUrl(e.to_string()))?;
    if !advanced_override && url.scheme() != "https" {
        return Err(SsrfError::NotHttps);
    }
    match url.host() {
        Some(Host::Ipv4(ip)) => {
            if !advanced_override && is_blocked_ip(IpAddr::V4(ip)) {
                return Err(SsrfError::BlockedHost(ip.to_string()));
            }
        }
        Some(Host::Ipv6(ip)) => {
            if !advanced_override && is_blocked_ip(IpAddr::V6(ip)) {
                return Err(SsrfError::BlockedHost(ip.to_string()));
            }
        }
        Some(Host::Domain(d)) => {
            let d = d.strip_suffix('.').unwrap_or(d);
            if !advanced_override && (d == "localhost" || d.ends_with(".localhost")) {
                return Err(SsrfError::BlockedHost(d.to_string()));
            }
        }
        None => return Err(SsrfError::InvalidUrl(raw.to_string())),
    }
    Ok(ValidatedUrl(url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn blocks_private_and_local_ranges() {
        for ip in [
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)),
            IpAddr::V4(Ipv4Addr::new(172, 16, 3, 4)),
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)), // cloud metadata
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 1)), // ULA
            IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)), // link-local
        ] {
            assert!(is_blocked_ip(ip), "should block {ip}");
        }
        assert!(!is_blocked_ip(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
    }

    #[test]
    fn rejects_http_and_local_hosts_without_override() {
        assert!(matches!(validate_url("http://api.example.com", false), Err(SsrfError::NotHttps)));
        assert!(matches!(validate_url("https://localhost", false), Err(SsrfError::BlockedHost(_))));
        assert!(matches!(validate_url("https://127.0.0.1", false), Err(SsrfError::BlockedHost(_))));
        assert!(matches!(validate_url("https://10.0.0.1", false), Err(SsrfError::BlockedHost(_))));
        assert!(validate_url("https://api.example.com", false).is_ok());
    }

    #[test]
    fn override_allows_http_and_private() {
        assert!(validate_url("http://localhost:11434", true).is_ok()); // Ollama
        assert!(validate_url("http://192.168.1.50:1234", true).is_ok());
    }

    #[test]
    fn blocks_ipv4_compatible_ipv6_loopback() {
        // ::127.0.0.1 (IPv4-compatible form, top 96 bits zero) must re-check the embedded v4.
        assert!(is_blocked_ip(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0x7f00, 0x0001))));
        // ::ffff:127.0.0.1 (mapped form) still blocked.
        assert!(is_blocked_ip(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0x7f00, 0x0001))));
        // ::10.0.0.1 (compatible, private v4) blocked too.
        assert!(is_blocked_ip(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0x0a00, 0x0001))));
        // A genuine public IPv6 is NOT blocked (regression guard).
        assert!(!is_blocked_ip(IpAddr::V6(Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0, 1))));
    }

    #[test]
    fn rejects_trailing_dot_localhost() {
        assert!(matches!(validate_url("https://localhost./", false), Err(SsrfError::BlockedHost(_))));
        // also via a bracketed IPv4-compatible IPv6 loopback URL:
        assert!(matches!(validate_url("https://[::127.0.0.1]/", false), Err(SsrfError::BlockedHost(_))));
    }
}

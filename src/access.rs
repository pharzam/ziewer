//! Client IP resolution and role classification.

use crate::config::{Config, parse_ip_rule};
use std::net::{IpAddr, SocketAddr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Admin,
    Allowed,
    Denied,
}

/// IPv4-mapped IPv6 (`::ffff:a.b.c.d`) → IPv4.
pub fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        v4 => v4,
    }
}

pub fn client_ip(peer: SocketAddr, headers: &http::HeaderMap, trust_proxy: bool) -> IpAddr {
    if trust_proxy {
        let forwarded = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .and_then(|first| first.trim().parse::<IpAddr>().ok());
        if let Some(ip) = forwarded {
            return normalize(ip);
        }
    }
    normalize(peer.ip())
}

pub fn classify(ip: IpAddr, cfg: &Config) -> Role {
    let ip = normalize(ip);
    if cfg.admin_ip.trim().parse::<IpAddr>().ok().map(normalize) == Some(ip) {
        return Role::Admin;
    }
    if cfg.allowed_ips.iter().filter_map(|r| parse_ip_rule(r)).any(|n| n.contains(&ip)) {
        Role::Allowed
    } else {
        Role::Denied
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use http::HeaderMap;

    fn cfg() -> Config {
        Config {
            listen: "x".into(),
            admin_ip: "192.168.1.10".into(),
            trust_proxy: false,
            allowed_ips: vec!["192.168.1.20".into(), "10.0.0.0/24".into(), "192.168.1.10".into()],
            folders: vec![],
        }
    }
    fn ip(s: &str) -> IpAddr { s.parse().unwrap() }
    fn peer(s: &str) -> SocketAddr { SocketAddr::new(ip(s), 1234) }
    fn xff(v: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", v.parse().unwrap());
        h
    }

    #[test] fn admin_wins() { assert_eq!(classify(ip("192.168.1.10"), &cfg()), Role::Admin); }
    #[test] fn allowed_single_and_cidr() {
        assert_eq!(classify(ip("192.168.1.20"), &cfg()), Role::Allowed);
        assert_eq!(classify(ip("10.0.0.77"), &cfg()), Role::Allowed);
    }
    #[test] fn denied_otherwise() { assert_eq!(classify(ip("8.8.8.8"), &cfg()), Role::Denied); }
    #[test] fn mapped_v6_matches_v4_rule() {
        assert_eq!(classify(normalize(ip("::ffff:192.168.1.20")), &cfg()), Role::Allowed);
    }
    #[test] fn xff_ignored_without_trust() {
        assert_eq!(client_ip(peer("1.2.3.4"), &xff("5.6.7.8"), false), ip("1.2.3.4"));
    }
    #[test] fn xff_first_entry_with_trust() {
        assert_eq!(client_ip(peer("1.2.3.4"), &xff("5.6.7.8, 10.0.0.1"), true), ip("5.6.7.8"));
    }
    #[test] fn xff_garbage_falls_back_to_peer() {
        assert_eq!(client_ip(peer("1.2.3.4"), &xff("unknown"), true), ip("1.2.3.4"));
    }
    #[test] fn peer_mapped_is_normalized() {
        assert_eq!(client_ip(peer("::ffff:1.2.3.4"), &HeaderMap::new(), false), ip("1.2.3.4"));
    }
}

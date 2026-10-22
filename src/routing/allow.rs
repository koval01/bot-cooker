//! The opt-in allowlist (empty by default = trap everyone).

use std::net::IpAddr;

use crate::config::Allowlist;

/// Compiled allowlist.
pub struct AllowList {
    uas: Vec<String>,
    nets: Vec<Cidr>,
}

impl AllowList {
    pub fn new(allow: &Allowlist) -> Self {
        Self {
            uas: allow.user_agents.iter().map(|s| s.to_ascii_lowercase()).collect(),
            nets: allow.cidrs.iter().filter_map(|c| Cidr::parse(c)).collect(),
        }
    }

    /// Is this client exempt from the tarpit? Only true when the operator has
    /// explicitly populated the allowlist.
    pub fn contains(&self, ip: IpAddr, user_agent: Option<&str>) -> bool {
        if let Some(ua) = user_agent {
            let ua = ua.to_ascii_lowercase();
            if self.uas.iter().any(|a| ua.contains(a.as_str())) {
                return true;
            }
        }
        self.nets.iter().any(|n| n.contains(ip))
    }
}

/// A tiny CIDR type supporting both IPv4 and IPv6.
struct Cidr {
    base: u128,
    mask: u128,
    is_v6: bool,
}

impl Cidr {
    fn parse(s: &str) -> Option<Self> {
        let (addr, bits) = match s.split_once('/') {
            Some((a, b)) => (a, b.parse::<u32>().ok()?),
            None => (s, 0),
        };
        let ip: IpAddr = addr.parse().ok()?;
        match ip {
            IpAddr::V4(v4) => {
                let bits = if s.contains('/') { bits } else { 32 };
                if bits > 32 {
                    return None;
                }
                let base = u32::from(v4) as u128;
                let mask = if bits == 0 { 0 } else { (!0u32 << (32 - bits)) as u128 };
                Some(Cidr { base: base & mask, mask, is_v6: false })
            }
            IpAddr::V6(v6) => {
                let bits = if s.contains('/') { bits } else { 128 };
                if bits > 128 {
                    return None;
                }
                let base = u128::from(v6);
                let mask = if bits == 0 { 0 } else { !0u128 << (128 - bits) };
                Some(Cidr { base: base & mask, mask, is_v6: true })
            }
        }
    }

    fn contains(&self, ip: IpAddr) -> bool {
        match ip {
            IpAddr::V4(v4) if !self.is_v6 => {
                let a = u32::from(v4) as u128;
                (a & self.mask) == self.base
            }
            IpAddr::V6(v6) if self.is_v6 => {
                let a = u128::from(v6);
                (a & self.mask) == self.base
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidrs() {
        let c = Cidr::parse("66.249.64.0/19").unwrap();
        assert!(c.contains("66.249.70.1".parse().unwrap()));
        assert!(!c.contains("8.8.8.8".parse().unwrap()));
        let v6 = Cidr::parse("2001:db8::/32").unwrap();
        assert!(v6.contains("2001:db8::1".parse().unwrap()));
        assert!(!v6.contains("66.249.70.1".parse().unwrap()));
        assert!(Cidr::parse("10.0.0.0/33").is_none());
    }

    #[test]
    fn empty_allowlist_spares_nobody() {
        let a = AllowList::new(&Allowlist::default());
        assert!(!a.contains("127.0.0.1".parse().unwrap(), Some("Googlebot")));
    }

    #[test]
    fn matches_by_cidr_or_agent() {
        let a = AllowList::new(&Allowlist { user_agents: vec!["MyMonitor".into()], cidrs: vec!["10.0.0.0/8".into()] });
        assert!(a.contains("10.1.2.3".parse().unwrap(), None));
        assert!(a.contains("8.8.8.8".parse().unwrap(), Some("mymonitor/1.0")));
        assert!(!a.contains("8.8.8.8".parse().unwrap(), Some("curl")));
    }
}

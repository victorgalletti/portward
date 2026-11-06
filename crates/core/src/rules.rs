//! Endpoint rules in Portmaster's syntax.
//!
//! One rule per line: `<+|-> <entity>` where `+` permits and `-` blocks.
//! Entities:
//! * `example.com`      – exactly this domain
//! * `.example.com`     – this domain and all subdomains
//! * `*.example.com`    – only subdomains
//! * `*ads*`            – glob on the whole domain
//! * `1.2.3.4`, `10.0.0.0/8`, `fd00::/8` – IPs and networks
//! * `*`                – everything
//!
//! Lines starting with `#` are comments. The first matching rule wins.

use crate::normalize_domain;
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Permit,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entity {
    Any,
    Domain(String),
    DomainAndSubs(String),
    SubsOnly(String),
    Glob(String),
    Net(IpNet),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub action: Action,
    pub entity: Entity,
    pub source: String,
}

/// Why something was permitted or blocked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub action: Action,
    /// Human readable reason, e.g. `rule "- ads.com"` or `filter list HaGeZi Pro`.
    pub reason: String,
}

impl Decision {
    pub fn permit(reason: impl Into<String>) -> Self {
        Self { action: Action::Permit, reason: reason.into() }
    }
    pub fn block(reason: impl Into<String>) -> Self {
        Self { action: Action::Block, reason: reason.into() }
    }
    pub fn blocked(&self) -> bool {
        self.action == Action::Block
    }
}

pub fn parse_rule(line: &str) -> Result<Option<Rule>, String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let (action, rest) = match line.chars().next() {
        Some('+') => (Action::Permit, &line[1..]),
        Some('-') => (Action::Block, &line[1..]),
        _ => return Err(format!("a regra precisa começar com + ou -: {line}")),
    };
    let ent = rest.trim();
    if ent.is_empty() {
        return Err(format!("regra sem domínio/IP: {line}"));
    }
    let entity = if ent == "*" {
        Entity::Any
    } else if let Ok(net) = ent.parse::<IpNet>() {
        Entity::Net(net)
    } else if let Ok(ip) = ent.parse::<IpAddr>() {
        Entity::Net(IpNet::from(ip))
    } else if let Some(d) = ent.strip_prefix("*.") {
        if d.contains('*') {
            Entity::Glob(normalize_domain(ent))
        } else {
            Entity::SubsOnly(normalize_domain(d))
        }
    } else if ent.contains('*') {
        Entity::Glob(normalize_domain(ent))
    } else if let Some(d) = ent.strip_prefix('.') {
        Entity::DomainAndSubs(normalize_domain(d))
    } else {
        if !ent.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
            return Err(format!("domínio inválido na regra: {line}"));
        }
        Entity::Domain(normalize_domain(ent))
    };
    Ok(Some(Rule { action, entity, source: line.to_string() }))
}

fn glob_match(pat: &str, s: &str) -> bool {
    // Simple `*` glob, iterative with backtracking.
    let (p, s) = (pat.as_bytes(), s.as_bytes());
    let (mut pi, mut si, mut star, mut mark) = (0, 0, None, 0);
    while si < s.len() {
        if pi < p.len() && p[pi] == s[si] {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(st) = star {
            pi = st + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

impl Rule {
    pub fn matches_domain(&self, d: &str) -> bool {
        match &self.entity {
            Entity::Any => true,
            Entity::Domain(x) => d == x,
            Entity::DomainAndSubs(x) => d == x || d.ends_with(&format!(".{x}")),
            Entity::SubsOnly(x) => d.ends_with(&format!(".{x}")),
            Entity::Glob(g) => glob_match(g, d),
            Entity::Net(_) => false,
        }
    }
    pub fn matches_ip(&self, ip: IpAddr) -> bool {
        match &self.entity {
            Entity::Any => true,
            Entity::Net(n) => n.contains(&ip),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    pub rules: Vec<Rule>,
}

impl RuleSet {
    /// Parses all lines; returns the rule set and the errors of invalid lines.
    pub fn parse<S: AsRef<str>>(lines: &[S]) -> (Self, Vec<String>) {
        let mut rules = vec![];
        let mut errors = vec![];
        for l in lines {
            match parse_rule(l.as_ref()) {
                Ok(Some(r)) => rules.push(r),
                Ok(None) => {}
                Err(e) => errors.push(e),
            }
        }
        (Self { rules }, errors)
    }

    pub fn check_domain(&self, domain: &str) -> Option<Decision> {
        self.rules.iter().find(|r| r.matches_domain(domain)).map(|r| Decision {
            action: r.action,
            reason: format!("regra \"{}\"", r.source),
        })
    }

    pub fn check_ip(&self, ip: IpAddr) -> Option<Decision> {
        self.rules.iter().find(|r| r.matches_ip(ip)).map(|r| Decision {
            action: r.action,
            reason: format!("regra \"{}\"", r.source),
        })
    }

    /// IP networks that should be blocked at the packet filter. Networks that
    /// are shadowed by an earlier permit rule for the same network are skipped.
    pub fn blocked_networks(&self) -> Vec<IpNet> {
        let mut out = vec![];
        for (i, r) in self.rules.iter().enumerate() {
            if let (Action::Block, Entity::Net(n)) = (r.action, &r.entity) {
                let shadowed = self.rules[..i].iter().any(|p| {
                    p.action == Action::Permit
                        && matches!(&p.entity, Entity::Net(pn) if pn.contains(n))
                });
                if !shadowed {
                    out.push(*n);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules() {
        let (rs, errs) = RuleSet::parse(&[
            "# comment",
            "+ good.ads.com",
            "- .ads.com",
            "- *.tracker.net",
            "- *metrics*",
            "- 10.0.0.0/8",
            "bad rule",
        ]);
        assert_eq!(errs.len(), 1);
        assert!(!rs.check_domain("good.ads.com").unwrap().blocked());
        assert!(rs.check_domain("ads.com").unwrap().blocked());
        assert!(rs.check_domain("x.ads.com").unwrap().blocked());
        assert!(rs.check_domain("tracker.net").is_none());
        assert!(rs.check_domain("a.tracker.net").unwrap().blocked());
        assert!(rs.check_domain("app-metrics.example.org").unwrap().blocked());
        assert!(rs.check_ip("10.1.2.3".parse().unwrap()).unwrap().blocked());
        assert!(rs.check_ip("8.8.8.8".parse().unwrap()).is_none());
        assert_eq!(rs.blocked_networks().len(), 1);
    }

    #[test]
    fn glob() {
        assert!(glob_match("*ads*", "myadserver.com"));
        assert!(glob_match("a*c", "abc"));
        assert!(!glob_match("a*c", "abd"));
    }
}

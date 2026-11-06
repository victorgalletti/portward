//! Domain filter lists.
//!
//! Accepts the common formats in one parser:
//! * hosts files: `0.0.0.0 ads.example.com`
//! * plain domain lists: `ads.example.com`
//! * wildcard lists: `*.ads.example.com`
//! * adblock domain rules: `||ads.example.com^`

use crate::{domain_and_parents, normalize_domain};
use std::collections::HashMap;

/// Parses a filter list body into normalised domains.
pub fn parse_list(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in body.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('!') || line.starts_with('[') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let first = parts.next().unwrap_or("");
        let candidate = if first.parse::<std::net::IpAddr>().is_ok() {
            match parts.next() {
                Some(d) => d,
                None => continue,
            }
        } else {
            first
        };
        let mut d = candidate;
        if let Some(x) = d.strip_prefix("||") {
            // adblock: only plain `||domain^` rules, skip ones with paths/options.
            match x.strip_suffix('^') {
                Some(x) if !x.contains('/') => d = x,
                _ => continue,
            }
        }
        let d = d.trim_start_matches("*.").trim_start_matches('.');
        let d = normalize_domain(d);
        if d.is_empty()
            || !d.contains('.')
            || matches!(d.as_str(), "localhost" | "localhost.localdomain" | "local" | "broadcasthost")
            || !d.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        {
            continue;
        }
        out.push(d);
    }
    out
}

/// Fast lookup over all enabled lists. Each domain maps to the index of the
/// first list that contains it.
#[derive(Debug, Default, Clone)]
pub struct FilterSet {
    domains: HashMap<Box<str>, u16>,
    list_names: Vec<String>,
}

impl FilterSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_list(&mut self, name: &str, domains: impl IntoIterator<Item = String>) -> usize {
        let idx = self.list_names.len() as u16;
        self.list_names.push(name.to_string());
        let mut n = 0;
        for d in domains {
            self.domains.entry(d.into_boxed_str()).or_insert(idx);
            n += 1;
        }
        n
    }

    pub fn len(&self) -> usize {
        self.domains.len()
    }

    pub fn is_empty(&self) -> bool {
        self.domains.is_empty()
    }

    /// Returns the name of the list that blocks `domain`, if any.
    pub fn lookup(&self, domain: &str, include_parents: bool) -> Option<&str> {
        let hit = if include_parents {
            domain_and_parents(domain).find_map(|d| self.domains.get(d))
        } else {
            self.domains.get(domain)
        };
        hit.map(|&i| self.list_names[i as usize].as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_formats() {
        let body = "# header\n127.0.0.1 localhost\n0.0.0.0 ads.example.com # c\nplain.org\n||ab.net^\n||path.net/x^\n*.wild.io\n! comment\n::1 ip6-localhost\n";
        assert_eq!(parse_list(body), ["ads.example.com", "plain.org", "ab.net", "wild.io"]);
    }

    #[test]
    fn lookup() {
        let mut fs = FilterSet::new();
        fs.add_list("A", vec!["ads.com".to_string()]);
        assert_eq!(fs.lookup("x.ads.com", true), Some("A"));
        assert_eq!(fs.lookup("x.ads.com", false), None);
        assert_eq!(fs.lookup("ads.com", false), Some("A"));
        assert_eq!(fs.lookup("notads.com", true), None);
    }
}

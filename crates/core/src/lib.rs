//! Portward core: shared types, configuration, rule engine and filter lists.
//!
//! Everything here is platform independent and has no I/O besides reading and
//! writing the config file, so it can be unit tested and shared between the
//! daemon and the desktop UI.

pub mod config;
pub mod filterlist;
pub mod model;
pub mod rules;

pub use config::Config;
pub use filterlist::FilterSet;
pub use rules::{Decision, RuleSet};

/// Normalises a domain name: lower case, no trailing dot, no surrounding spaces.
pub fn normalize_domain(d: &str) -> String {
    d.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// Iterates over a domain and all its parent domains:
/// `a.b.example.com` → `a.b.example.com`, `b.example.com`, `example.com`, `com`.
pub fn domain_and_parents(domain: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(domain);
    std::iter::from_fn(move || {
        let cur = rest?;
        rest = cur.find('.').map(|i| &cur[i + 1..]).filter(|s| !s.is_empty());
        Some(cur)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parents() {
        let v: Vec<_> = domain_and_parents("a.b.example.com").collect();
        assert_eq!(v, ["a.b.example.com", "b.example.com", "example.com", "com"]);
        assert_eq!(normalize_domain(" Foo.COM. "), "foo.com");
    }
}

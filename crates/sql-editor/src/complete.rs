use crate::EXTRA_KEYWORDS;
use sqlparser::keywords::ALL_KEYWORDS;
use std::cmp::Ordering;

const MAX_COMPLETIONS: usize = 50;

fn cmp_ignore_case(a: &str, b: &str) -> Ordering {
    // byte-wise ASCII fold; avoids allocating an uppercase copy per compare
    let (x, y) = (a.as_bytes(), b.as_bytes());
    for (p, q) in x.iter().zip(y.iter()) {
        match p.to_ascii_uppercase().cmp(&q.to_ascii_uppercase()) {
            Ordering::Equal => {}
            ord => return ord,
        }
    }
    x.len().cmp(&y.len())
}

/// Case-insensitive prefix completions over the SQL keyword set plus the
/// session catalog (table/column names). Sorted, deduplicated, ≤ 50 items.
pub fn completions(prefix: &str, catalog: &[String]) -> Vec<String> {
    let matches = |c: &str| {
        c.len() >= prefix.len()
            && c.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
    };
    let mut out: Vec<String> = ALL_KEYWORDS
        .iter()
        .copied()
        .chain(EXTRA_KEYWORDS.iter().copied())
        .chain(catalog.iter().map(String::as_str))
        .filter(|c| matches(c))
        .map(str::to_owned)
        .collect();
    out.sort_by(|a, b| cmp_ignore_case(a, b));
    out.dedup_by(|a, b| cmp_ignore_case(a, b) == Ordering::Equal);
    out.truncate(MAX_COMPLETIONS);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case::keyword("SEL", &[], &["SELECT"])]
    #[case::lowercase_prefix("sel", &[], &["SELECT"])]
    #[case::catalog("use", &["users"], &["users", "USE"])]
    #[case::mixed("us", &["UserSettings"], &["USAGE", "USE", "USER", "UserSettings"])]
    #[case::no_match("zzz", &["users"], &[])]
    #[case::catalog_case_prefix("USER", &["users"], &["USER", "users"])]
    fn completions_match(
        #[case] prefix: &str,
        #[case] catalog: &[&str],
        #[case] expected_contains: &[&str],
    ) {
        let catalog: Vec<String> = catalog.iter().map(|s| s.to_string()).collect();
        let out = completions(prefix, &catalog);
        for want in expected_contains {
            assert!(
                out.iter().any(|c| c.eq_ignore_ascii_case(want)),
                "missing {want} in {out:?}"
            );
        }
        assert!(out.len() <= MAX_COMPLETIONS);
    }

    #[test]
    fn empty_prefix_returns_keywords_capped_at_50() {
        let out = completions("", &[]);
        assert_eq!(out.len(), MAX_COMPLETIONS);
    }

    #[test]
    fn dedup_across_keyword_and_catalog() {
        let out = completions("sel", &["select".to_string()]);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn sorted_output() {
        let catalog = vec!["UserSettings".to_string(), "accounts".to_string()];
        let out = completions("u", &catalog);
        let mut sorted = out.clone();
        sorted.sort_by(|a, b| cmp_ignore_case(a, b));
        assert_eq!(out, sorted);
    }
}

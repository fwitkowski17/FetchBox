//! Version numbers as the update check and the plugins use them: `1.2.3` or `1.2.3-rc.1`, compared
//! as semantic versioning does (SPEC 6.9).

use std::cmp::Ordering;

/// `1.2.3` or `1.2.3-rc.1`: the numbers, and the pre-release part (`rc.1`) if there is one.
pub fn parse_version(v: &str) -> Option<(Vec<u64>, Option<String>)> {
    let (numbers, pre) = match v.split_once('-') {
        Some((n, p)) => (n, Some(p.to_owned())),
        None => (v, None),
    };
    let numbers: Option<Vec<u64>> = numbers.split('.').map(|p| p.parse().ok()).collect();
    let numbers = numbers?;
    (!numbers.is_empty()).then_some((numbers, pre))
}

/// Order of two pre-release parts as semantic versioning has it: dot-separated identifiers, numbers
/// compared as numbers and below words, words as text, and the longer one wins when the shorter is
/// its beginning (`rc.1` < `rc.2` < `rc.10` < `rc.10.1`).
fn cmp_pre(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.split('.'), b.split('.'));
    loop {
        match (x.next(), y.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(p), Some(q)) => {
                let order = match (p.parse::<u64>(), q.parse::<u64>()) {
                    (Ok(m), Ok(n)) => m.cmp(&n),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => p.cmp(q),
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

/// Is `latest` a higher version than `current`? A pre-release ranks below its release
/// (`0.7.0-rc.1` < `0.7.0`) and pre-releases of one version are ordered by their parts
/// (`rc.1` < `rc.2`); anything that is not a version is never "newer".
pub fn is_newer(latest: &str, current: &str) -> bool {
    let (Some((a, a_pre)), Some((b, b_pre))) = (parse_version(latest), parse_version(current))
    else {
        return false;
    };
    let len = a.len().max(b.len());
    for i in 0..len {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    // Same numbers: a release is newer than any pre-release, and pre-releases go by their parts.
    match (a_pre, b_pre) {
        (None, Some(_)) => true,
        (Some(x), Some(y)) => cmp_pre(&x, &y) == Ordering::Greater,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_compared_by_their_numbers() {
        assert!(is_newer("0.7.0", "0.6.1"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.6.1", "0.6.1"));
        assert!(!is_newer("0.6.0", "0.6.1"));
        assert!(!is_newer("1.0.0", "1"));
    }

    #[test]
    fn a_pre_release_ranks_below_its_release() {
        assert!(is_newer("0.7.0", "0.7.0-rc.1"));
        assert!(!is_newer("0.7.0-rc.1", "0.7.0"));
        assert!(is_newer("0.7.1-rc.1", "0.7.0"));
    }

    #[test]
    fn something_that_is_not_a_version_is_never_newer() {
        assert!(!is_newer("nightly", "0.6.1"));
        assert!(!is_newer("0.7.0", "dev"));
        assert!(!is_newer("", "0.6.1"));
    }

    #[test]
    fn pre_releases_follow_semantic_versioning() {
        assert!(is_newer("0.7.0-rc.2", "0.7.0-rc.1"));
        assert!(is_newer("0.7.0-rc.10", "0.7.0-rc.2"), "numbers, not text");
        assert!(is_newer("0.7.0-rc.1", "0.7.0-beta.3"), "rc after beta");
        assert!(is_newer("0.7.0-rc.1.1", "0.7.0-rc.1"));
        assert!(is_newer("0.7.0-alpha", "0.7.0-1"), "words above numbers");
        assert!(!is_newer("0.7.0-rc.1", "0.7.0-rc.1"));
        assert!(!is_newer("0.7.0-rc.1", "0.7.0-rc.2"));
        assert!(is_newer("0.7.1-alpha", "0.7.0"));
    }
}

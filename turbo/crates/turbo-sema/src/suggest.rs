//! "Did you mean?" suggestions.
//!
//! Hand-rolled, allocation-light Levenshtein distance over `&str`,
//! plus a small picker that scores candidate names against a typo and
//! returns the best match if it's "close enough".
//!
//! No external crate — we deliberately avoid pulling `strsim` or
//! similar so the sema crate stays dependency-free.
//!
//! Used at every "undefined identifier" diagnostic site to attach a
//! `help: did you mean `foo`?` hint when one of the in-scope names
//! is within edit-distance threshold of the unknown name.

/// Levenshtein distance between two strings, classic 2-row DP.
///
/// Returns 0 for identical strings; max(len(a), len(b)) is the worst
/// case. Costs are uniform: insertion = deletion = substitution = 1.
pub fn levenshtein(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    if a.is_empty() {
        return b.chars().count();
    }
    if b.is_empty() {
        return a.chars().count();
    }

    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    // Two rows: previous and current.
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            // deletion, insertion, substitution
            let del = prev[j] + 1;
            let ins = curr[j - 1] + 1;
            let sub = prev[j - 1] + cost;
            curr[j] = del.min(ins).min(sub);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

/// Pick the best suggestion for `typo` out of `candidates`.
///
/// Returns `Some(name)` if at least one candidate is within the
/// "close enough" threshold:
///
///   * length 1-2 typos: must match exactly (no suggestion)
///   * length 3-4: distance <= 1
///   * length 5+:  distance <= 2 OR distance <= max(2, len/3)
///
/// Ties are broken by the candidate's natural order in the slice
/// (i.e. the first close match wins). Empty `candidates` always
/// returns `None`.
pub fn suggest_for<'a, I>(typo: &str, candidates: I) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    let typo_len = typo.chars().count();
    if typo_len < 3 {
        // Too short to safely suggest — Levenshtein collapses meaning.
        return None;
    }
    let max_dist = if typo_len <= 4 {
        1
    } else {
        (typo_len / 3).max(2)
    };

    let mut best: Option<(usize, &'a str)> = None;
    for cand in candidates {
        if cand == typo {
            continue;
        }
        let d = levenshtein(typo, cand);
        if d > max_dist {
            continue;
        }
        match best {
            None => best = Some((d, cand)),
            Some((bd, _)) if d < bd => best = Some((d, cand)),
            _ => {}
        }
    }
    best.map(|(_, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lev_basic() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("flaw", "lawn"), 2);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("same", "same"), 0);
    }

    #[test]
    fn lev_unicode() {
        assert_eq!(levenshtein("café", "cafe"), 1);
    }

    #[test]
    fn suggest_picks_closest() {
        let cands = ["count", "counter", "value", "total"];
        assert_eq!(suggest_for("counr", cands.iter().copied()), Some("count"));
    }

    #[test]
    fn suggest_too_short_returns_none() {
        let cands = ["x", "y", "z"];
        assert_eq!(suggest_for("a", cands.iter().copied()), None);
    }

    #[test]
    fn suggest_no_match_when_far() {
        let cands = ["completely", "different", "names"];
        assert_eq!(suggest_for("foo", cands.iter().copied()), None);
    }

    #[test]
    fn suggest_skips_exact() {
        let cands = ["foo", "fop"];
        // Exact match is skipped (we wouldn't be suggesting if it
        // existed). The next closest is `fop`.
        assert_eq!(suggest_for("foo", cands.iter().copied()), Some("fop"));
    }
}

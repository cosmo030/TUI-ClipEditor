//! Matching typed text against names, for the home screen search and the
//! command palette.

/// How well `candidate` matches `query`, ignoring case; lower is better.
/// `None` means no match. An empty query matches everything equally.
///
/// Ranking: starts with the query, then contains it (earlier is better),
/// then contains its characters in order (fewer gaps is better).
pub(super) fn score(candidate: &str, query: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }
    let candidate = candidate.to_lowercase();
    let query = query.to_lowercase();
    if candidate.starts_with(&query) {
        return Some(0);
    }
    if let Some(position) = candidate.find(&query) {
        return Some(1 + u32::try_from(position).unwrap_or(u32::MAX / 2));
    }
    subsequence_gaps(&candidate, &query).map(|gaps| 1_000_u32.saturating_add(gaps))
}

/// The number of skipped characters if `query`'s characters appear in
/// `candidate` in order.
fn subsequence_gaps(candidate: &str, query: &str) -> Option<u32> {
    let mut wanted = query.chars().peekable();
    let mut gaps = 0_u32;
    let mut started = false;
    for character in candidate.chars() {
        match wanted.peek() {
            None => break,
            Some(&next) if next == character => {
                wanted.next();
                started = true;
            }
            Some(_) if started => gaps = gaps.saturating_add(1),
            Some(_) => {}
        }
    }
    wanted.peek().is_none().then_some(gaps)
}

/// Indices of `candidates` that match `query`, best first; equal scores
/// keep their original order.
pub(super) fn ranked<'a>(candidates: impl Iterator<Item = &'a str>, query: &str) -> Vec<usize> {
    let mut scored: Vec<(u32, usize)> = candidates
        .enumerate()
        .filter_map(|(index, candidate)| score(candidate, query).map(|score| (score, index)))
        .collect();
    scored.sort_by_key(|&(score, index)| (score, index));
    scored.into_iter().map(|(_, index)| index).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_everything() {
        assert_eq!(score("anything", ""), Some(0));
    }

    #[test]
    fn prefix_beats_substring_beats_subsequence() {
        let prefix = score("Holiday.mp4", "hol").unwrap();
        let substring = score("my holiday.mp4", "hol").unwrap();
        let subsequence = score("hotel lobby.mp4", "hol").unwrap();
        assert!(prefix < substring, "{prefix} vs {substring}");
        assert!(substring < subsequence, "{substring} vs {subsequence}");
    }

    #[test]
    fn matching_ignores_case() {
        assert_eq!(score("BODYCAM.mp4", "bodycam"), Some(0));
    }

    #[test]
    fn non_matches_are_none() {
        assert_eq!(score("trip.mkv", "zz"), None);
        assert_eq!(score("ab", "abc"), None);
    }

    #[test]
    fn fewer_gaps_rank_higher() {
        let tight = score("export clips", "exc").unwrap();
        let loose = score("e x p o r t c", "exc").unwrap();
        assert!(tight < loose);
    }

    #[test]
    fn ranked_orders_best_first_and_drops_non_matches() {
        let names = ["big folder.mp4", "bodycam.mp4", "notes", "about bob.mkv"];
        assert_eq!(ranked(names.iter().copied(), "bo"), [1, 3, 0]);
        assert_eq!(ranked(names.iter().copied(), ""), [0, 1, 2, 3]);
    }
}

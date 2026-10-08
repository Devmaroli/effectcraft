//! Token / Levenshtein scoring (planner `levenshtein` + `tokenScore`).

use crate::normalize::{compact_size, normalize};

/// `1 - distance / max(len)`. Empty strings score 0 unless both empty (1).
pub fn similarity(a: &str, b: &str) -> f32 {
    let na = normalize(a);
    let nb = normalize(b);
    if na.is_empty() && nb.is_empty() {
        return 1.0;
    }
    if na.is_empty() || nb.is_empty() {
        return 0.0;
    }
    if na == nb {
        return 1.0;
    }
    let d = levenshtein(&na, &nb);
    let max = na.chars().count().max(nb.chars().count());
    if max == 0 {
        return 1.0;
    }
    (1.0 - d as f32 / max as f32).max(0.0)
}

pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a == b {
        return 0;
    }
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = i;
        if let Some(first) = row.first_mut() {
            *first = i + 1;
        }
        for (j, cb) in b.iter().enumerate() {
            let tmp = row.get(j + 1).copied().unwrap_or(usize::MAX);
            let ins = row.get(j + 1).copied().unwrap_or(usize::MAX).saturating_add(1);
            let del = row.get(j).copied().unwrap_or(usize::MAX).saturating_add(1);
            let sub = prev.saturating_add(usize::from(ca != cb));
            if let Some(cell) = row.get_mut(j + 1) {
                *cell = ins.min(del).min(sub);
            }
            prev = tmp;
        }
    }
    row.last().copied().unwrap_or(usize::MAX)
}

const ORIENTATION_ANTONYMS: &[(&str, &str)] = &[("vertical", "horizontal"), ("horizontal", "vertical"), ("portrait", "landscape"), ("landscape", "portrait")];

fn antonym(token: &str) -> Option<&'static str> {
    ORIENTATION_ANTONYMS.iter().find(|(a, _)| *a == token).map(|(_, b)| *b)
}

/// Planner `tokenScore`: substring, compact-size, then per-token Levenshtein.
pub fn token_score(query: &str, target: &str) -> f32 {
    let q = normalize(query);
    if q.is_empty() {
        return 1.0;
    }
    let t = normalize(target);
    if t.is_empty() {
        return 0.0;
    }
    if t.contains(&q) {
        return 1.0;
    }
    if compact_size(&t).contains(&compact_size(&q)) {
        return 0.98;
    }
    let q_tokens: Vec<&str> = q.split(' ').filter(|w| !w.is_empty()).collect();
    let t_tokens: Vec<&str> = t.split(' ').filter(|w| !w.is_empty()).collect();
    if q_tokens.is_empty() {
        return 0.0;
    }
    let mut total = 0.0f32;
    for token in &q_tokens {
        let mut best = 0.0f32;
        let anti = antonym(token);
        for cand in &t_tokens {
            if anti == Some(*cand) {
                continue;
            }
            if anti.is_some() && token == cand {
                best = best.max(1.0);
                continue;
            }
            if cand.contains(token) || token.contains(cand) {
                let mn = token.len().min(cand.len()) as f32;
                let mx = token.len().max(cand.len()) as f32;
                if mx > 0.0 {
                    best = best.max(mn / mx);
                }
                continue;
            }
            let d = levenshtein(token, cand);
            let mx = token.len().max(cand.len());
            if mx > 0 {
                best = best.max(1.0 - d as f32 / mx as f32);
            }
        }
        total += best;
    }
    total / q_tokens.len() as f32
}

/// Screen-specific name match: ≥ 90% similarity on normalized names.
pub const SCREEN_SPECIFIC_NAME_MIN: f32 = 0.90;

pub fn names_match_90(a: &str, b: &str) -> bool {
    similarity(a, b) + f32::EPSILON >= SCREEN_SPECIFIC_NAME_MIN
}

//! Name and paste cleanup (planner `utils.normalize` + `stripLeadingListMarker`).

/// Lowercase, keep letters/digits, collapse other runs to a single space.
pub fn normalize(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut space = false;
    for c in value.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            space = false;
        } else if !space && !out.is_empty() {
            out.push(' ');
            space = true;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Compact form used when comparing sizes inside names (`1920x1080`).
pub fn compact_size(value: &str) -> String {
    normalize(value).replace(' ', "")
}

/// Split a paste blob on newlines, commas and semicolons.
pub fn split_paste(value: &str, cleanup: bool) -> Vec<String> {
    value
        .split(['\n', ',', ';'])
        .map(|line| {
            let mut cleaned = line.trim().to_string();
            cleaned = unglue_list_hyphen(&cleaned);
            if cleanup { strip_leading_list_marker(&cleaned) } else { cleaned }
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// `"4-Slayel"` → `"4 Slayel"` so list-marker stripping can run.
fn unglue_list_hyphen(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i > 4 {
        return line.to_string();
    }
    let rest = line.get(i..).unwrap_or("");
    let hyphen = rest.starts_with('-') || rest.starts_with('–') || rest.starts_with('—');
    if hyphen {
        let after = rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        let tail = rest.get(after..).unwrap_or("");
        if !tail.is_empty() && !tail.starts_with(char::is_whitespace) {
            return format!("{} {}", line.get(..i).unwrap_or(""), tail);
        }
    }
    line.to_string()
}

/// Planner `stripLeadingListMarker`: bullets, `1. `, `1) `, `[1] `, leading 1–4 digits + space.
/// Keeps real sizes (`1920x1080`), format aliases (`1.7HD`) and ordinals (`1st`).
pub fn strip_leading_list_marker(line: &str) -> String {
    let value = line.trim();
    if value.is_empty() {
        return String::new();
    }
    if is_protected_list_token(value) {
        return value.to_string();
    }
    let value = unglue_list_hyphen(value);
    strip_marker_once(&value)
}

fn is_protected_list_token(value: &str) -> bool {
    let t = value.trim();
    let lower = t.to_ascii_lowercase();
    if looks_like_pixel_size(t) {
        return true;
    }
    if lower.starts_with(|c: char| c.is_ascii_digit()) && lower.contains("hd") {
        let rest = lower.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
        if rest.trim_start().starts_with("hd") {
            return true;
        }
    }
    ordinal_prefix(t)
}

fn ordinal_prefix(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return false;
    }
    let rest = value.get(i..).unwrap_or("").to_ascii_lowercase();
    rest.starts_with("st") || rest.starts_with("nd") || rest.starts_with("rd") || rest.starts_with("th")
}

fn looks_like_pixel_size(value: &str) -> bool {
    parse_pixel_size(value).is_some() && !value.contains(' ') || {
        let n = normalize(value);
        n.contains(" x ") && parse_pixel_size(value).is_some() && n.split(' ').filter(|w| !w.is_empty()).count() <= 3
    }
}

fn strip_marker_once(value: &str) -> String {
    let t = value.trim_start();
    let chars: Vec<char> = t.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    // bullet + space
    const BULLETS: &[char] = &['-', '*', '•', '‣', '◦', '▪', '▫', '–', '—'];
    if BULLETS.contains(&chars[0]) && chars.get(1).is_some_and(|c| c.is_whitespace()) {
        return t.chars().skip(2).collect::<String>().trim().to_string();
    }
    // (12) or 12. or 12) or 12: or 12- then space
    let mut i = 0;
    let mut paren = false;
    if chars[0] == '(' {
        paren = true;
        i = 1;
    }
    let digit_start = i;
    while i < chars.len() && chars[i].is_ascii_digit() && i - digit_start < 4 {
        i += 1;
    }
    if i > digit_start {
        if paren {
            if chars.get(i) == Some(&')') {
                i += 1;
                if chars.get(i).is_some_and(|c| c.is_whitespace()) {
                    return chars[i + 1..].iter().collect::<String>().trim().to_string();
                }
            }
        } else if matches!(chars.get(i), Some('.' | ')' | ':' | '-')) && chars.get(i + 1).is_some_and(|c| c.is_whitespace()) {
            return chars[i + 2..].iter().collect::<String>().trim().to_string();
        } else if chars.get(i).is_some_and(|c| c.is_whitespace()) {
            return chars[i + 1..].iter().collect::<String>().trim().to_string();
        }
    }
    // [12] space
    if chars[0] == '[' {
        let mut j = 1;
        while j < chars.len() && chars[j].is_ascii_digit() && j < 5 {
            j += 1;
        }
        if chars.get(j) == Some(&']') && chars.get(j + 1).is_some_and(|c| c.is_whitespace()) {
            return chars[j + 2..].iter().collect::<String>().trim().to_string();
        }
    }
    // a. or a) space
    if chars[0].is_ascii_alphabetic() && matches!(chars.get(1), Some('.' | ')')) && chars.get(2).is_some_and(|c| c.is_whitespace()) {
        return chars[3..].iter().collect::<String>().trim().to_string();
    }
    t.trim().to_string()
}

/// `1920x1080`, `1920 x 1080`, `1920×1080`, `1920*1080`, `1920 by 1080`.
pub fn parse_pixel_size(input: &str) -> Option<(u32, u32)> {
    let value = input.to_ascii_lowercase();
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let wlen = i - start;
            if (3..=5).contains(&wlen) {
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                let sep = if j + 1 < bytes.len() && &value[j..j + 2] == "by" {
                    j += 2;
                    true
                } else if j < bytes.len() && matches!(bytes[j], b'x' | b'*') {
                    // × is utf8 c3 97 — already lowercased to? × doesn't lower. Check original.
                    j += 1;
                    true
                } else {
                    false
                };
                if sep {
                    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    let h0 = j;
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                    let hlen = j - h0;
                    if (3..=5).contains(&hlen) {
                        let w: u32 = value.get(start..start + wlen)?.parse().ok()?;
                        let h: u32 = value.get(h0..h0 + hlen)?.parse().ok()?;
                        if w > 0 && h > 0 {
                            return Some((w, h));
                        }
                    }
                }
            }
        }
        i += 1;
    }
    // Unicode ×
    if let Some((left, right)) = value.split_once('×') {
        let w = left.trim().chars().rev().take_while(|c| c.is_ascii_digit()).collect::<String>().chars().rev().collect::<String>();
        let h = right.trim().chars().take_while(|c| c.is_ascii_digit()).collect::<String>();
        if (3..=5).contains(&w.len()) && (3..=5).contains(&h.len()) {
            let ww: u32 = w.parse().ok()?;
            let hh: u32 = h.parse().ok()?;
            return Some((ww, hh));
        }
    }
    None
}

pub fn format_size(w: u32, h: u32) -> String {
    format!("{w} x {h}")
}

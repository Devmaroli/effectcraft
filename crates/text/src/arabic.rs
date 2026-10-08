//! Arabic / bidi helpers used by layout, editing and the Character / Paragraph panels.
//!
//! These do not replace harfrust shaping. They decide fallback, digit remapping, kashida
//! slots, grapheme bounds and the first strong character for Auto direction.

use unicode_bidi::{BidiClass, bidi_class};

use crate::fonts::{self, FaceId};

/// Preferred Arabic fallback families (installed first wins).
pub const ARABIC_FALLBACK_FAMILIES: &[&str] =
    &["Dubai", "Noto Naskh Arabic", "Noto Sans Arabic", "Segoe UI Arabic", "Tahoma", "Arial", "Cairo", "Tajawal", "GE SS Unique", "GE SS Two", "DejaVu Sans"];

/// How well a face draws Arabic (cmap + contextual GSUB, not just codepoint coverage).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArabicQuality {
    None,
    /// Maps Arabic letters but lacks init/medi/fina (Inter and most Latin-first families).
    Poor,
    Good,
}

/// A letter in an Arabic script block (not digits, punctuation or marks).
pub fn is_arabic_letter(c: char) -> bool {
    matches!(c as u32,
        0x0620..=0x064A | 0x066E..=0x066F | 0x0671..=0x06D3 | 0x06D5
        | 0x06EE..=0x06EF | 0x06FA..=0x06FC | 0x06FF
        | 0x0750..=0x077F | 0x08A0..=0x08C9 | 0x08BE..=0x08D2
        | 0xFB50..=0xFDFF | 0xFE70..=0xFEFC)
        && !is_arabic_indic_digit(c)
}

pub fn is_arabic_indic_digit(c: char) -> bool {
    matches!(c, '\u{0660}'..='\u{0669}' | '\u{06F0}'..='\u{06F9}')
}

pub fn is_western_digit(c: char) -> bool {
    c.is_ascii_digit()
}

/// Combining marks that ride on the previous letter (harakat and similar).
pub fn is_combining_mark(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F | 0x064B..=0x065F | 0x0670
        | 0x06D6..=0x06ED | 0x08D3..=0x08FF | 0xFE20..=0xFE2F)
}

/// Dual-joining or right-joining Arabic letters (a kashida can sit after them).
pub fn can_take_kashida_after(c: char) -> bool {
    is_arabic_letter(c) && !matches!(c, 'ا' | 'أ' | 'إ' | 'آ' | 'ٱ' | 'د' | 'ذ' | 'ر' | 'ز' | 'و' | 'ؤ' | 'ة' | 'ى' | 'ء')
}

pub fn first_strong_rtl(text: &str) -> Option<bool> {
    for c in text.chars() {
        match bidi_class(c) {
            BidiClass::L | BidiClass::LRE | BidiClass::LRO | BidiClass::LRI => return Some(false),
            BidiClass::R | BidiClass::AL | BidiClass::RLE | BidiClass::RLO | BidiClass::RLI => return Some(true),
            _ => {}
        }
    }
    None
}

/// Character index of the grapheme that starts at or before `ci` (letter + its harakat).
pub fn grapheme_start(text: &str, ci: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return 0;
    }
    let mut i = ci.min(chars.len());
    if i == chars.len() || is_combining_mark(chars[i.min(chars.len() - 1)]) {
        i = i.saturating_sub(1);
    }
    while i > 0 && is_combining_mark(chars[i]) {
        i -= 1;
    }
    i
}

/// Exclusive end of the grapheme that contains `ci` (or starts at `ci` when `ci` is a boundary).
pub fn grapheme_end(text: &str, ci: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = ci.min(n);
    if i >= n {
        return n;
    }
    i += 1;
    while i < n && is_combining_mark(chars[i]) {
        i += 1;
    }
    i
}

pub fn grapheme_before(text: &str, ci: usize) -> usize {
    let i = ci.min(text.chars().count());
    if i == 0 {
        return 0;
    }
    grapheme_start(text, i - 1)
}

pub fn grapheme_after(text: &str, ci: usize) -> usize {
    grapheme_end(text, ci)
}

pub fn to_arabic_indic_digit(c: char) -> char {
    match c {
        '0'..='9' => char::from_u32(0x0660 + u32::from(c) - u32::from('0')).unwrap_or(c),
        _ => c,
    }
}

/// Keep Western digits and the % / $ / € that belong to them left-to-right inside an RTL paragraph
/// so "50% OFF" and "2026" do not become "%50" / "6202".
///
/// Digits already have an even (LTR) embedding level. Neutrals such as `%` take the paragraph
/// direction and would otherwise jump to the other side of the number. Copy the neighbouring
/// digit's level onto those marks — do not force level 0, which collapses the RTL paragraph.
pub fn pin_western_numerals_ltr(text: &str, levels: &mut [unicode_bidi::Level]) {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let level_at = |levels: &[unicode_bidi::Level], b: usize| levels.get(b).copied();
    let set_level = |levels: &mut [unicode_bidi::Level], b: usize, c: char, lv: unicode_bidi::Level| {
        for k in 0..c.len_utf8() {
            if let Some(slot) = levels.get_mut(b.saturating_add(k)) {
                *slot = lv;
            }
        }
    };
    for (i, &(b, c)) in chars.iter().enumerate() {
        let prev = i.checked_sub(1).and_then(|j| chars.get(j));
        let next = chars.get(i + 1);
        let from_digit = prev
            .filter(|(_, p)| p.is_ascii_digit())
            .and_then(|(pb, _)| level_at(levels, *pb))
            .or_else(|| next.filter(|(_, n)| n.is_ascii_digit()).and_then(|(nb, _)| level_at(levels, *nb)));
        let Some(lv) = from_digit else { continue };
        let attach = matches!(c, '%' | '$' | '€' | '+' | '#') && prev.is_some_and(|(_, p)| p.is_ascii_digit())
            || (matches!(c, '.' | ',' | ':' | '/') && prev.is_some_and(|(_, p)| p.is_ascii_digit()) && next.is_some_and(|(_, n)| n.is_ascii_digit()));
        if attach {
            set_level(levels, b, c, lv);
        }
    }
}

pub fn to_western_digit(c: char) -> char {
    match c {
        '\u{0660}'..='\u{0669}' => char::from_u32(u32::from('0') + u32::from(c) - 0x0660).unwrap_or(c),
        '\u{06F0}'..='\u{06F9}' => char::from_u32(u32::from('0') + u32::from(c) - 0x06F0).unwrap_or(c),
        _ => c,
    }
}

pub fn arabic_quality(face: FaceId) -> ArabicQuality {
    let f = fonts::face(face);
    let sample = ['ب', 'ع', 'م', 'ن', 'ي'];
    let covered = sample.iter().filter(|c| f.has_char(**c)).count();
    if covered == 0 {
        return ArabicQuality::None;
    }
    if f.has_feature(b"init") && f.has_feature(b"medi") && f.has_feature(b"fina") { ArabicQuality::Good } else { ArabicQuality::Poor }
}

pub fn is_arabic_capable(face: FaceId) -> bool {
    arabic_quality(face) == ArabicQuality::Good
}

/// First installed family from [`ARABIC_FALLBACK_FAMILIES`], else Noto Naskh Arabic.
pub fn default_arabic_fallback() -> String {
    if cfg!(not(target_arch = "wasm32")) && !fonts::system_scanned() {
        fonts::scan_system();
    }
    for name in ARABIC_FALLBACK_FAMILIES {
        let r = fonts::resolve(name, "Regular");
        if !r.missing {
            return (*name).to_string();
        }
    }
    "Noto Naskh Arabic".into()
}

/// Face for an Arabic letter when the chosen family is Latin-first / poor, and a fallback is set.
pub fn arabic_run_face(ch: char, primary: FaceId, fallback_family: &str, style: &str) -> FaceId {
    if !is_arabic_letter(ch) || fallback_family.is_empty() {
        return fonts::fallback_for(ch, primary);
    }
    match arabic_quality(primary) {
        ArabicQuality::Good => fonts::fallback_for(ch, primary),
        ArabicQuality::Poor | ArabicQuality::None => {
            let fb = fonts::resolve(fallback_family, style);
            if fonts::face(fb.face).has_char(ch) { fb.face } else { fonts::fallback_for(ch, primary) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_strong_detects_arabic_and_latin() {
        assert_eq!(first_strong_rtl("خصم 50%"), Some(true));
        assert_eq!(first_strong_rtl("50% OFF خصم"), Some(false));
        assert_eq!(first_strong_rtl("2026"), None);
    }

    #[test]
    fn grapheme_eats_harakat() {
        let t = "مَرْ";
        assert_eq!(grapheme_end(t, 0), 2); // م + َ
        assert_eq!(grapheme_before(t, 2), 0);
        assert_eq!(grapheme_after(t, 0), 2);
    }

    #[test]
    fn sale_string_auto_bidi_is_rtl_and_latin_stays_ltr() {
        let sale = "خصم ٥٠٪ على كل شيء | 50% OFF everything";
        let mut bidi = unicode_bidi::BidiInfo::new(sale, None);
        assert!(bidi.paragraphs[0].level.is_rtl(), "first strong is Arabic");
        pin_western_numerals_ltr(sale, &mut bidi.levels);
        let s = bidi.reorder_line(&bidi.paragraphs[0], 0..sale.len()).into_owned();
        let off = s.find("OFF").expect("OFF");
        let kh = s.find('خ').expect("kh");
        assert!(off < kh, "visual: OFF left of خصم: {s:?}");
        assert!(s.contains("50%"), "50% stays 50% not %50: {s:?}");
    }

    #[test]
    fn digit_remap_round_trips() {
        assert_eq!(to_arabic_indic_digit('5'), '٥');
        assert_eq!(to_western_digit('٥'), '5');
        assert_eq!(to_western_digit('۵'), '5');
    }
}

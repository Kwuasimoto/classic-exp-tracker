//! The game's map names (data/maps.txt, from the MapleStory Classic Wiki), and the
//! rules that match an OCR read to one of them. A map gets a name only from this list
//! (or from the user): OCR alone is wrong about a third of the time, mostly by cutting
//! names short (measured 2026-10-22). The matching rules (ignore case and punctuation,
//! allow a misread letter only when one name is clearly closest and the numbers agree)
//! follow beanguri's Maple-Classic-Exp-Overlay (MIT); no code from it is used.

use std::sync::OnceLock;

const LIST: &str = include_str!("../../data/maps.txt");

/// A cut-off read is completed only when it covers this much of the one name it starts.
const MIN_PREFIX_SHARE: f32 = 0.75;
/// Keys this long may have two misread letters; shorter ones one.
const LONG_KEY: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameMatch {
    Listed(&'static str),
    NoMatch,
}

struct Entry {
    name: &'static str,
    key: String,
}

fn entries() -> &'static [Entry] {
    static ENTRIES: OnceLock<Vec<Entry>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        LIST.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|name| Entry { name, key: key(name) })
            .collect()
    })
}

/// Lower case letters and digits only, Roman numerals I–X as digits, and OCR's usual
/// confusions undone: `Free Market <1>`, `Free Market (1)` and `free market 1` are one key.
fn key(text: &str) -> String {
    const ROMAN: [&str; 10] = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"];
    let cleaned: String = text.chars().map(|c| if matches!(c, '|' | '!') { 'I' } else { c }).collect();
    let words = cleaned.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty());
    let key: String = words
        .map(|w| ROMAN.iter().position(|r| *r == w).map_or_else(|| w.to_ascii_lowercase(), |i| (i + 1).to_string()))
        .collect();
    key.replace("rn", "m")
}

fn digits(key: &str) -> String {
    key.chars().filter(char::is_ascii_digit).collect()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut row = vec![i + 1];
        for (j, &cb) in b.iter().enumerate() {
            row.push((prev[j + 1] + 1).min(row[j] + 1).min(prev[j] + usize::from(ca != cb)));
        }
        prev = row;
    }
    prev[b.len()]
}

/// The list name for one candidate key: exact, one clearly closest name within a
/// misread letter or two (same numbers), or the one name a cut-off read starts.
fn match_key(read: &str) -> Option<&'static str> {
    let list = entries();
    if let Some(e) = list.iter().find(|e| e.key == read) {
        return Some(e.name);
    }
    let allowed = if read.len() >= LONG_KEY { 2 } else { 1 };
    let mut ranked: Vec<(usize, &Entry)> = list
        .iter()
        .filter(|e| digits(&e.key) == digits(read) && e.key.len().abs_diff(read.len()) <= allowed)
        .map(|e| (edit_distance(read, &e.key), e))
        .collect();
    ranked.sort_by_key(|(d, _)| *d);
    if let Some(&(best, e)) = ranked.first() {
        let runner_up = ranked.get(1).map_or(usize::MAX, |(d, _)| *d);
        if best <= allowed && runner_up > best {
            return Some(e.name);
        }
    }
    let mut starts = list.iter().filter(|e| e.key.starts_with(read) && digits(&e.key) == digits(read));
    match (starts.next(), starts.next()) {
        (Some(e), None) if read.len() as f32 >= MIN_PREFIX_SHARE * e.key.len() as f32 => Some(e.name),
        _ => None,
    }
}

/// The list name an OCR read of the map name line (map_strip::name_line) spells, if
/// any: tried whole, then without its last word (OCR adds stray words).
pub fn match_name(ocr: &str) -> NameMatch {
    // Punctuation alone is OCR noise, except at the end, where "!" or "|" is a numeral I.
    let all: Vec<&str> = ocr.split_whitespace().collect();
    let words: Vec<&str> = all
        .iter()
        .enumerate()
        .filter(|&(i, w)| i + 1 == all.len() || w.chars().any(|c| c.is_ascii_alphanumeric()))
        .map(|(_, w)| *w)
        .collect();
    let trimmed = words.split_last().map(|(_, head)| head).filter(|h| !h.is_empty());
    for words in [Some(&words[..]), trimmed].into_iter().flatten() {
        let read = key(&words.join(" "));
        if read.len() >= 4
            && let Some(name) = match_key(&read)
        {
            return NameMatch::Listed(name);
        }
    }
    NameMatch::NoMatch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_from_the_tracker_match_the_list_or_nothing() {
        // OCR reads saved by the tracker before 0.5.0 (2026-10-16 – 2026-10-22).
        // Name-line reads: from the tracker before 0.5.0 (2026-10-16 – 2026-10-22, street
        // removed) and from a recording at 1366x768 (2026-10-22).
        for (read, expected) in [
            ("Swampy Land In A Deep Forest", Some("Swampy Land in a Deep Forest")),
            ("Free Market < 1 >", Some("Free Market (1)")),
            ("Deep Valley 2", Some("Deep Valley II")),
            ("! The Burnt Land 2", Some("The Burnt Land II")),
            ("The Swarnp Of Despair 1", Some("The Swamp of Despair I")),
            ("Deep Valley I Use", Some("Deep Valley I")),
            ("Free Market Entr;", Some("Free Market Entrance")),
            ("Perion Street Corner", Some("Perion Street Corner")),
            // Cut short: no single name, so no name.
            ("Free Market", None),
            ("O", None),
            ("Unno", None),
            ("The Field 'jp North Of Ellini", None),
            // Cut off before its number: not completed to a name with other numbers.
            ("The Land of Wild Bo", None),
            // Not on the list.
            ("Excavation Site 1", None),
        ] {
            let got = match match_name(read) {
                NameMatch::Listed(name) => Some(name),
                NameMatch::NoMatch => None,
            };
            assert_eq!(got, expected, "{read:?}");
        }
    }
}

//! Guard against one class of source damage: UTF-8 Russian text read as
//! Windows-1251 and saved back as UTF-8 (`сеть` -> `СЃРµС‚СЊ`). It once reached
//! the user as garbage in the window and the logs, in nine files. This module
//! makes a test fail at build time if it ever happens again.

/// Bigrams produced only by UTF-8 bytes interpreted as CP1251. None of them can
/// occur in correct Russian text, so a hit is unambiguous. Cover the whole
/// Cyrillic alphabet plus the punctuation that showed up in the damaged files.
const MOJIBAKE_MARKERS: &[&str] = &[
    // 2-byte sequences: lead D0/D1 -> 'Р'/'С', continuation -> low char.
    "Р°", "Р±", "РІ", "Рі", "Рґ", "Рµ", "Р¶", "Р·", "Рё", "Р№", "Рє", "Р»", "Рј", "РЅ", "Рѕ", "Рї",
    "СЂ", "СЃ", "С‚", "Сѓ", "С„", "С…", "С‡", "С€", "С‰", "СЉ", "С‹", "СЊ", "СЌ", "СЋ", "СЏ",
    // 3-byte sequences -> em dash, ellipsis, up-arrow. Deliberately no quote
    // markers: «/» are valid Russian punctuation and would false-positively
    // match correct text like «Complete verification».
    "вЂ", "в†‘", "вЂ¦",
];

/// Whether `text` contains a UTF-8-as-Windows-1251 signature.
pub fn has_mojibake(text: &str) -> bool {
    MOJIBAKE_MARKERS.iter().any(|m| text.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mojibake_is_detected_and_real_russian_is_not() {
        assert!(has_mojibake("СЃРµС‚СЊ")); // "сеть"
        assert!(has_mojibake("вЂ”")); // em dash "—"
        assert!(has_mojibake("РёР·РјРµРЅРёР»Р°СЃСЊ")); // "изменилась"
        assert!(!has_mojibake("сеть — это хорошо"));
        assert!(!has_mojibake("Привет, мир! 24 часа изменилась"));
        assert!(!has_mojibake(""));
    }

    /// Recursively scan every Rust source file. If mojibake lands in a literal
    /// again, this fails with the exact path instead of the user discovering
    /// broken letters in the window.
    #[test]
    fn no_rust_source_contains_mojibake() {
        fn walk(dir: &std::path::Path, bad: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, bad);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    // This file carries the markers as data; scanning it would
                    // forbid the detector's own source (the trap `tray.rs`
                    // avoids by stripping comments before its search).
                    if path.file_name().is_some_and(|n| n == "encoding.rs") {
                        continue;
                    }
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        if has_mojibake(&text) {
                            bad.push(path.display().to_string());
                        }
                    }
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut bad = Vec::new();
        walk(&root, &mut bad);
        assert!(
            bad.is_empty(),
            "these source files contain UTF-8-as-Windows-1251 mojibake: {bad:?}"
        );
    }
}

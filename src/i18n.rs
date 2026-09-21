//! Translations. Message IDs are the English texts in the code; `po/<lang>.po`
//! holds the translations in gettext format and is compiled into the binary,
//! so there is nothing to install. The language follows the system locale.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Embedded catalogs by language code (the part of the locale before `_`).
const CATALOGS: &[(&str, &str)] = &[("de", include_str!("../po/de.po"))];

/// Translates `msgid` into the language of the process locale; falls back to
/// the English `msgid`.
pub fn tr(msgid: &'static str) -> &'static str {
    static CATALOG: OnceLock<Option<HashMap<String, String>>> = OnceLock::new();
    CATALOG
        .get_or_init(|| catalog_for(&locale_from(|var| std::env::var(var).ok())?))
        .as_ref()
        .and_then(|catalog| catalog.get(msgid))
        .map_or(msgid, String::as_str)
}

/// Like [`tr`], but for an explicit locale such as `de_DE.UTF-8` – e.g. the
/// caller's locale when another process talks to the running instance.
pub fn tr_for(locale: &str, msgid: &str) -> String {
    catalog_for(locale)
        .and_then(|mut catalog| catalog.remove(msgid))
        .unwrap_or_else(|| msgid.to_owned())
}

/// The locale that decides the message language: `LC_ALL`, then
/// `LC_MESSAGES`, then `LANG`; empty values count as unset.
pub fn locale_from(getenv: impl Fn(&str) -> Option<String>) -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(getenv)
        .find(|value| !value.is_empty())
}

fn catalog_for(locale: &str) -> Option<HashMap<String, String>> {
    let lang = locale.split(['_', '.', '@']).next()?;
    CATALOGS
        .iter()
        .find(|(code, _)| *code == lang)
        .map(|(_, po)| parse_po(po))
}

/// Minimal `.po` reader: `msgid`/`msgstr` with continuation lines and the
/// usual escapes. Like gettext, it skips the header, fuzzy and untranslated
/// entries.
fn parse_po(po: &str) -> HashMap<String, String> {
    let mut catalog = HashMap::new();
    let (mut msgid, mut msgstr) = (String::new(), String::new());
    let (mut fuzzy, mut in_msgstr) = (false, false);
    for line in po.lines().map(str::trim).chain([""]) {
        if line.is_empty() {
            if !fuzzy && !msgid.is_empty() && !msgstr.is_empty() {
                catalog.insert(std::mem::take(&mut msgid), std::mem::take(&mut msgstr));
            }
            (msgid, msgstr, fuzzy, in_msgstr) = (String::new(), String::new(), false, false);
        } else if let Some(flags) = line.strip_prefix("#,") {
            fuzzy |= flags.contains("fuzzy");
        } else if let Some(rest) = line.strip_prefix("msgid ") {
            in_msgstr = false;
            msgid.push_str(&unquote(rest));
        } else if let Some(rest) = line.strip_prefix("msgstr ") {
            in_msgstr = true;
            msgstr.push_str(&unquote(rest));
        } else if line.starts_with('"') {
            let target = if in_msgstr { &mut msgstr } else { &mut msgid };
            target.push_str(&unquote(line));
        }
    }
    catalog
}

fn unquote(quoted: &str) -> String {
    let inner = quoted
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(quoted);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PO: &str = r#"
msgid ""
msgstr ""
"Content-Type: text/plain; charset=UTF-8\n"

# comment
msgid "Hello"
msgstr "Hallo"

msgid ""
"Two\n"
"lines"
msgstr ""
"Zwei\n"
"Zeilen"

#, fuzzy
msgid "Unsure"
msgstr "Unsicher"

msgid "Missing"
msgstr ""

msgid "Say \"hi\"\tnow"
msgstr "Sag \"hallo\"\tjetzt"
"#;

    #[test]
    fn parses_entries_and_escapes() {
        let catalog = parse_po(PO);
        assert_eq!(catalog.get("Hello").map(String::as_str), Some("Hallo"));
        assert_eq!(catalog.get("Two\nlines").map(String::as_str), Some("Zwei\nZeilen"));
        assert_eq!(
            catalog.get("Say \"hi\"\tnow").map(String::as_str),
            Some("Sag \"hallo\"\tjetzt")
        );
    }

    #[test]
    fn skips_header_fuzzy_and_untranslated() {
        let catalog = parse_po(PO);
        assert!(!catalog.contains_key(""));
        assert!(!catalog.contains_key("Unsure"));
        assert!(!catalog.contains_key("Missing"));
    }

    #[test]
    fn picks_language_from_locale() {
        assert_eq!(tr_for("de_DE.UTF-8", "Capture cancelled."), "Aufnahme abgebrochen.");
        assert_eq!(tr_for("de", "Capture cancelled."), "Aufnahme abgebrochen.");
        assert_eq!(tr_for("en_US.UTF-8", "Capture cancelled."), "Capture cancelled.");
        assert_eq!(tr_for("C", "Capture cancelled."), "Capture cancelled.");
        assert_eq!(tr_for("de_DE.UTF-8", "Not in the catalog"), "Not in the catalog");
    }

    #[test]
    fn locale_precedence() {
        let env = |vars: &'static [(&'static str, &'static str)]| {
            move |var: &str| {
                vars.iter()
                    .find(|(name, _)| *name == var)
                    .map(|(_, value)| value.to_string())
            }
        };
        assert_eq!(locale_from(env(&[("LANG", "de_DE"), ("LC_ALL", "en_US")])), Some("en_US".into()));
        assert_eq!(locale_from(env(&[("LANG", "de_DE"), ("LC_ALL", "")])), Some("de_DE".into()));
        assert_eq!(locale_from(env(&[("LANG", "de_DE"), ("LC_MESSAGES", "en_GB")])), Some("en_GB".into()));
        assert_eq!(locale_from(env(&[])), None);
    }

    /// Placeholders like `{detail}` are filled in after translating, so every
    /// translation must keep the ones of its msgid.
    #[test]
    fn translations_keep_placeholders() {
        for (code, po) in CATALOGS {
            for (msgid, msgstr) in parse_po(po) {
                let placeholders = |s: &str| {
                    let mut found: Vec<String> = s
                        .split('{')
                        .skip(1)
                        .filter_map(|part| part.split_once('}').map(|(name, _)| name.to_owned()))
                        .collect();
                    found.sort();
                    found
                };
                assert_eq!(placeholders(&msgid), placeholders(&msgstr), "{code}: {msgid:?}");
            }
        }
    }
}

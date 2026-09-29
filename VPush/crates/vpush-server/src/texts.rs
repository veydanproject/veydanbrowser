//! Texts of pushes, in the language of the device.
//!
//! The server writes the text because the device may be unable to: a push
//! that finds the app closed is shown as it came. The server knows little,
//! and the text says no more than it knows: that a message came, and for a
//! group, what the owner of the device calls it.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

const FALLBACK: &str = "en";

/// The languages compiled in. A new one is a new file and a line here.
const FILES: &[(&str, &str)] = &[
    ("en", include_str!("../locales/en.toml")),
    ("ru", include_str!("../locales/ru.toml")),
];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Texts {
    dm_title: String,
    dm_body: String,
    dm_body_many: String,
    group_title: String,
    group_title_unnamed: String,
    group_body: String,
    group_body_many: String,
    test_title: String,
    test_body: String,
}

fn all() -> &'static BTreeMap<&'static str, Texts> {
    static ALL: OnceLock<BTreeMap<&'static str, Texts>> = OnceLock::new();
    ALL.get_or_init(|| {
        FILES
            .iter()
            .map(|(lang, text)| {
                let texts = toml::from_str(text)
                    .unwrap_or_else(|e| panic!("locales/{lang}.toml: {e}"));
                (*lang, texts)
            })
            .collect()
    })
}

/// `ru-RU`, `ru_RU` and `RU` are all `ru`.
fn language(locale: &str) -> String {
    locale
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

impl Texts {
    pub fn of(locale: &str) -> &'static Texts {
        let all = all();
        all.get(language(locale).as_str())
            .unwrap_or_else(|| &all[FALLBACK])
    }

    pub fn languages() -> Vec<&'static str> {
        all().keys().copied().collect()
    }

    fn body(one: &str, many: &str, count: u32) -> String {
        if count > 1 {
            many.replace("{count}", &count.to_string())
        } else {
            one.to_string()
        }
    }

    /// Title and body of a push about direct messages.
    pub fn dm(&self, count: u32) -> (String, String) {
        (
            self.dm_title.clone(),
            Self::body(&self.dm_body, &self.dm_body_many, count),
        )
    }

    /// Title and body of a push about a group, by the name the device gave it.
    pub fn group(&self, name: Option<&str>, count: u32) -> (String, String) {
        let title = match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(name) => self.group_title.replace("{name}", name),
            None => self.group_title_unnamed.clone(),
        };
        (
            title,
            Self::body(&self.group_body, &self.group_body_many, count),
        )
    }

    pub fn test(&self) -> (String, String) {
        (self.test_title.clone(), self.test_body.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_file_is_whole() {
        // A missing or a misspelled key fails here, not on a user's phone.
        assert_eq!(Texts::languages(), ["en", "ru"]);
    }

    #[test]
    fn the_language_is_taken_from_the_locale() {
        for locale in ["ru", "ru-RU", "ru_RU", "RU"] {
            assert_eq!(Texts::of(locale).dm(1).0, "Личное сообщение", "{locale}");
        }
        for locale in ["en", "en-US", "de", "", "zz-ZZ"] {
            assert_eq!(Texts::of(locale).dm(1).0, "Direct message", "{locale}");
        }
    }

    #[test]
    fn a_group_is_called_what_the_device_calls_it() {
        let t = Texts::of("ru");
        assert_eq!(t.group(Some("Команда"), 1), ("Группа: Команда".into(), "Новое сообщение".into()));
        assert_eq!(t.group(None, 1).0, "Группа");
        assert_eq!(t.group(Some("  "), 1).0, "Группа");
    }

    #[test]
    fn many_messages_are_counted() {
        assert_eq!(Texts::of("ru").dm(3).1, "Новых сообщений: 3");
        assert_eq!(Texts::of("en").group(Some("Team"), 12).1, "New messages: 12");
    }
}

use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    Spanish,
}

static CURRENT: OnceLock<Language> = OnceLock::new();

#[cfg(test)]
thread_local! {
    static OVERRIDE: std::cell::Cell<Option<Language>> = const { std::cell::Cell::new(None) };
}

impl Language {
    #[must_use]
    pub fn from_locale(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        let spanish = value
            .get(..2)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("es"))
            && value
                .as_bytes()
                .get(2)
                .is_none_or(|next| matches!(next, b'_' | b'.' | b'@' | b'-'));
        Some(if spanish {
            Language::Spanish
        } else {
            Language::English
        })
    }

    #[must_use]
    pub fn detect(get: impl Fn(&str) -> Option<String>) -> Self {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .find_map(|name| get(name).as_deref().and_then(Language::from_locale))
            .unwrap_or_default()
    }

    #[must_use]
    pub fn from_environment() -> Self {
        Self::detect(|name| std::env::var(name).ok())
    }
}

pub fn init(language: Language) {
    let _ = CURRENT.set(language);
}

#[must_use]
pub fn current() -> Language {
    #[cfg(test)]
    if let Some(language) = OVERRIDE.with(std::cell::Cell::get) {
        return language;
    }
    CURRENT.get().copied().unwrap_or_default()
}

#[cfg(test)]
pub fn with_language<R>(language: Language, run: impl FnOnce() -> R) -> R {
    let previous = OVERRIDE.with(|slot| slot.replace(Some(language)));
    let result = run();
    OVERRIDE.with(|slot| slot.set(previous));
    result
}

macro_rules! tr {
    ($spanish:expr, $english:expr) => {
        match $crate::i18n::current() {
            $crate::i18n::Language::Spanish => String::from($spanish),
            $crate::i18n::Language::English => String::from($english),
        }
    };
}

pub(crate) use tr;

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn spanish_locales_are_recognised_and_other_languages_are_not() {
        for value in [
            "es",
            "es_ES.UTF-8",
            "es_MX",
            "ES_es.utf8",
            "es@euro",
            "es-ES",
        ] {
            assert_eq!(
                Language::from_locale(value),
                Some(Language::Spanish),
                "{value}"
            );
        }
        for value in [
            "en_US.UTF-8",
            "C",
            "POSIX",
            "est_EE",
            "ast_ES",
            "fr_FR",
            "e",
        ] {
            assert_eq!(
                Language::from_locale(value),
                Some(Language::English),
                "{value}"
            );
        }
        assert_eq!(Language::from_locale(""), None);
        assert_eq!(Language::from_locale("  "), None);
    }

    #[test]
    fn the_first_variable_that_is_set_decides() {
        assert_eq!(
            Language::detect(env(&[("LANG", "es_ES.UTF-8")])),
            Language::Spanish
        );
        assert_eq!(
            Language::detect(env(&[("LC_ALL", "en_US.UTF-8"), ("LANG", "es_ES.UTF-8")])),
            Language::English
        );
        assert_eq!(
            Language::detect(env(&[
                ("LC_ALL", ""),
                ("LC_MESSAGES", "es_ES"),
                ("LANG", "C")
            ])),
            Language::Spanish
        );
        assert_eq!(Language::detect(env(&[])), Language::English);
    }

    #[test]
    fn the_translation_follows_the_language_in_force() {
        assert_eq!(tr!("hola", "hello"), "hello");
        assert_eq!(
            with_language(Language::Spanish, || tr!("hola", "hello")),
            "hola"
        );
        assert_eq!(
            with_language(Language::Spanish, || tr!(
                format!("n = {}", 1),
                format!("m = {}", 1)
            )),
            "n = 1"
        );
        assert_eq!(tr!("hola", "hello"), "hello");
    }
}

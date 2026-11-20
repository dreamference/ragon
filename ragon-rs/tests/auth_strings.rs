use ragon_launcher::auth::strings::{Key, Lang, fmt_in, text};

const VENDORS: &[&str] = &["openai", "chatgpt", "codex", "anthropic", "claude", "gemini", "google", "microsoft", "github", "copilot"];

#[test]
fn every_key_has_a_text_in_every_language() {
    for key in Key::ALL {
        for lang in Lang::ALL {
            assert!(!text(lang, *key).trim().is_empty(), "{key:?} is empty in {lang:?}");
        }
    }
}

#[test]
fn placeholders_agree_across_languages() {
    for key in Key::ALL {
        let count = |lang| (0..4).filter(|i| text(lang, *key).contains(&format!("{{{i}}}"))).count();
        for lang in Lang::ALL {
            assert_eq!(count(lang), count(Lang::En), "{key:?} placeholders differ in {lang:?}");
        }
    }
}

#[test]
fn no_string_names_a_vendor() {
    for key in Key::ALL {
        for lang in Lang::ALL {
            let lower = text(lang, *key).to_lowercase();
            for vendor in VENDORS {
                assert!(!lower.contains(vendor), "{key:?} in {lang:?} names {vendor}");
            }
        }
    }
}

#[test]
fn translations_differ_from_english_for_longer_strings() {
    for lang in [Lang::Pt, Lang::Es, Lang::De, Lang::Fr] {
        assert_ne!(text(lang, Key::WrongCredentials), text(Lang::En, Key::WrongCredentials));
    }
}

#[test]
fn language_comes_from_lc_all_then_lc_messages_then_lang() {
    let env = |pairs: &'static [(&'static str, &'static str)]| move |name: &str| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string());
    assert_eq!(Lang::from_vars(env(&[("LC_ALL", "de_DE.UTF-8"), ("LANG", "fr_FR")])), Lang::De);
    assert_eq!(Lang::from_vars(env(&[("LC_ALL", ""), ("LC_MESSAGES", "pt_BR.UTF-8"), ("LANG", "fr_FR")])), Lang::Pt);
    assert_eq!(Lang::from_vars(env(&[("LANG", "es_MX.UTF-8")])), Lang::Es);
    assert_eq!(Lang::from_vars(env(&[("LANG", "fr")])), Lang::Fr);
}

#[test]
fn an_unknown_or_missing_language_gives_english() {
    assert_eq!(Lang::from_vars(|_| None), Lang::En);
    assert_eq!(Lang::from_vars(|n| (n == "LANG").then(|| "ja_JP.UTF-8".to_string())), Lang::En);
    assert_eq!(Lang::from_vars(|n| (n == "LANG").then(|| "C".to_string())), Lang::En);
}

#[test]
fn placeholders_are_filled() {
    assert_eq!(fmt_in(Lang::De, Key::SignedIn, &["node", "ann"]), "Bei node als ann angemeldet");
}

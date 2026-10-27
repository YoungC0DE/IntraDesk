use hbb_common::regex::Regex;
use std::ops::Deref;

mod en;
mod pt_BR;

pub const LANGS: &[(&str, &str)] = &[("en", "English"), ("pt-br", "Português (Brasil)")];

/// Only pt-BR and English are shipped; any Portuguese setting or locale maps to pt-BR, the rest
/// (including languages saved by older builds) falls back to English.
fn resolve_lang(saved_lang: &str, locale: &str) -> &'static str {
    let saved_lang = saved_lang.to_lowercase();
    let lang = if saved_lang.is_empty() {
        locale.to_lowercase()
    } else {
        saved_lang
    };
    if lang == "br" || lang.starts_with("pt") {
        "pt-br"
    } else {
        "en"
    }
}

pub fn translate(name: String) -> String {
    let locale = sys_locale::get_locale().unwrap_or_default();
    translate_locale(name, &locale)
}

pub fn translate_locale(name: String, locale: &str) -> String {
    let lang = resolve_lang(&hbb_common::config::LocalConfig::get_option("lang"), locale);
    let m = if lang == "pt-br" {
        pt_BR::T.deref()
    } else {
        en::T.deref()
    };
    let (name, placeholder_value) = extract_placeholder(&name);
    let replace = |s: &&str| {
        let mut s = s.to_string();
        if let Some(value) = placeholder_value.as_ref() {
            s = s.replace("{}", &value);
        }
        if !crate::is_rustdesk() {
            // These name RustDesk itself (credit), not the app.
            if s.contains("RustDesk")
                && name != "powered_by_me"
                && name != "intradesk-license-tip"
            {
                let app_name = crate::get_app_name();
                if !app_name.contains("RustDesk") {
                    s = s.replace("RustDesk", &app_name);
                } else {
                    // https://github.com/rustdesk/rustdesk-server-pro/issues/845
                    // If app_name contains "RustDesk" (e.g., "RustDesk-Admin"), we need to avoid
                    // replacing "RustDesk" within the already-substituted app_name, which would
                    // cause duplication like "RustDesk-Admin" -> "RustDesk-Admin-Admin".
                    //
                    // app_name only contains alphanumeric and hyphen.
                    const PLACEHOLDER: &str = "#A-P-P-N-A-M-E#";
                    if !s.contains(PLACEHOLDER) {
                        s = s.replace(&app_name, PLACEHOLDER);
                        s = s.replace("RustDesk", &app_name);
                        s = s.replace(PLACEHOLDER, &app_name);
                    } else {
                        // It's very unlikely to reach here.
                        // Skip replacement to avoid incorrect result.
                    }
                }
            }
        }
        s
    };
    if let Some(v) = m.get(&name as &str) {
        if !v.is_empty() {
            return replace(v);
        }
    }
    if lang != "en" {
        if let Some(v) = en::T.get(&name as &str) {
            if !v.is_empty() {
                return replace(v);
            }
        }
    }
    replace(&name.as_str())
}

// Matching pattern is {}
// Write {value} in the UI and {} in the translation file
//
// Example:
// Write in the UI: translate("There are {24} hours in a day")
// Write in the translation file: ("There are {} hours in a day", "{} hours make up a day")
fn extract_placeholder(input: &str) -> (String, Option<String>) {
    if let Ok(re) = Regex::new(r#"\{(.*?)\}"#) {
        if let Some(captures) = re.captures(input) {
            if let Some(inner_match) = captures.get(1) {
                let name = re.replace(input, "{}").to_string();
                let value = inner_match.as_str().to_string();
                return (name, Some(value));
            }
        }
    }
    (input.to_string(), None)
}

mod test {
    #[test]
    fn test_extract_placeholders() {
        use super::extract_placeholder as f;

        assert_eq!(f(""), ("".to_string(), None));
        assert_eq!(
            f("{3} sessions"),
            ("{} sessions".to_string(), Some("3".to_string()))
        );
        assert_eq!(f(" } { "), (" } { ".to_string(), None));
        // Allow empty value
        assert_eq!(
            f("{} sessions"),
            ("{} sessions".to_string(), Some("".to_string()))
        );
        // Match only the first one
        assert_eq!(
            f("{2} times {4} makes {8}"),
            ("{} times {4} makes {8}".to_string(), Some("2".to_string()))
        );
    }

    #[test]
    fn test_license_tip_keeps_the_rustdesk_credit() {
        for locale in ["en-US", "pt-BR"] {
            let s = super::translate_locale("intradesk-license-tip".to_owned(), locale);
            assert!(s.contains("RustDesk"), "{locale}: {s}");
        }
    }

    #[test]
    fn test_resolve_lang_maps_portuguese_to_pt_br_and_rest_to_english() {
        use super::resolve_lang as f;

        assert_eq!(f("", "pt-BR"), "pt-br");
        assert_eq!(f("", "pt_PT"), "pt-br");
        assert_eq!(f("pt-pt", "en-US"), "pt-br");
        assert_eq!(f("br", "en-US"), "pt-br");
        assert_eq!(f("", "en-US"), "en");
        assert_eq!(f("", "zh_CN"), "en");
        assert_eq!(f("fr", "pt-BR"), "en");
    }
}

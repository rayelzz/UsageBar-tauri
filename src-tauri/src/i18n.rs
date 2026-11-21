fn is_zh(locale: &str) -> bool {
    locale == "zh" || locale == "zh-CN" || locale == "cn"
}

fn is_ru(locale: &str) -> bool {
    locale == "ru" || locale.starts_with("ru-")
}

pub fn quit(locale: &str) -> &'static str {
    if is_zh(locale) {
        "退出 UsageBar"
    } else if is_ru(locale) {
        "Выйти из UsageBar"
    } else {
        "Quit UsageBar"
    }
}

pub fn show(locale: &str) -> &'static str {
    if is_zh(locale) {
        "显示面板"
    } else if is_ru(locale) {
        "Показать панель"
    } else {
        "Show overlay"
    }
}

pub fn hide(locale: &str) -> &'static str {
    if is_zh(locale) {
        "隐藏面板"
    } else if is_ru(locale) {
        "Скрыть панель"
    } else {
        "Hide overlay"
    }
}

pub fn refresh(locale: &str) -> &'static str {
    if is_zh(locale) {
        "立即刷新"
    } else if is_ru(locale) {
        "Обновить сейчас"
    } else {
        "Refresh now"
    }
}

pub fn settings(locale: &str) -> &'static str {
    if is_zh(locale) {
        "设置"
    } else if is_ru(locale) {
        "Настройки"
    } else {
        "Settings"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_supports_russian_and_falls_back_to_english() {
        assert_eq!(quit("ru"), "Выйти из UsageBar");
        assert_eq!(quit("ru-RU"), "Выйти из UsageBar");
        assert_eq!(quit("unsupported"), "Quit UsageBar");
    }

    #[test]
    fn tray_actions_are_localized() {
        assert_eq!(show("ru"), "Показать панель");
        assert_eq!(hide("zh"), "隐藏面板");
        assert_eq!(refresh("en"), "Refresh now");
        assert_eq!(settings("ru-RU"), "Настройки");
    }
}

fn is_zh(locale: &str) -> bool {
    locale == "zh" || locale == "zh-CN" || locale == "cn"
}

pub fn quit(locale: &str) -> &'static str {
    if is_zh(locale) {
        "退出 UsageBar"
    } else {
        "Quit UsageBar"
    }
}

pub fn is_zh(locale: &str) -> bool {
    locale == "zh" || locale == "zh-CN" || locale == "cn"
}

pub fn refresh_now(locale: &str) -> &'static str {
    if is_zh(locale) {
        "立即刷新"
    } else {
        "Refresh now"
    }
}

pub fn auto_refresh(locale: &str) -> &'static str {
    if is_zh(locale) {
        "自动刷新"
    } else {
        "Auto refresh"
    }
}

pub fn lock_position(locale: &str, locked: bool) -> &'static str {
    if is_zh(locale) {
        if locked {
            "解锁位置"
        } else {
            "锁定位置"
        }
    } else if locked {
        "Unlock position"
    } else {
        "Lock position"
    }
}

pub fn click_through(locale: &str) -> &'static str {
    if is_zh(locale) {
        "不阻挡下方点击"
    } else {
        "Don’t block clicks below"
    }
}

pub fn snap_left(locale: &str) -> &'static str {
    if is_zh(locale) {
        "贴左边"
    } else {
        "Snap to left"
    }
}

pub fn snap_right(locale: &str) -> &'static str {
    if is_zh(locale) {
        "贴右边"
    } else {
        "Snap to right"
    }
}

pub fn snap_top(locale: &str) -> &'static str {
    if is_zh(locale) {
        "贴上边"
    } else {
        "Snap to top"
    }
}

pub fn snap_bottom(locale: &str) -> &'static str {
    if is_zh(locale) {
        "贴下边"
    } else {
        "Snap to bottom"
    }
}

pub fn display_style(locale: &str) -> &'static str {
    if is_zh(locale) {
        "显示样式"
    } else {
        "Display style"
    }
}

pub fn ring_usage(locale: &str) -> &'static str {
    if is_zh(locale) {
        "圆环用量"
    } else {
        "Ring usage"
    }
}

pub fn transparent_icons(locale: &str) -> &'static str {
    if is_zh(locale) {
        "透明图标"
    } else {
        "Transparent icons"
    }
}

pub fn display_value(locale: &str) -> &'static str {
    if is_zh(locale) {
        "显示值"
    } else {
        "Display value"
    }
}

pub fn used_quota(locale: &str) -> &'static str {
    if is_zh(locale) {
        "已使用额度"
    } else {
        "Used quota"
    }
}

pub fn remaining_quota(locale: &str) -> &'static str {
    if is_zh(locale) {
        "剩余额度"
    } else {
        "Remaining quota"
    }
}

pub fn language(locale: &str) -> &'static str {
    if is_zh(locale) {
        "语言"
    } else {
        "Language"
    }
}

pub fn tools(locale: &str) -> &'static str {
    if is_zh(locale) {
        "提供商…"
    } else {
        "Providers…"
    }
}

pub fn tools_window(locale: &str) -> &'static str {
    if is_zh(locale) {
        "提供商"
    } else {
        "Providers"
    }
}

pub fn open_at_login(locale: &str) -> &'static str {
    if is_zh(locale) {
        "登录时打开"
    } else {
        "Open at login"
    }
}

pub fn quit(locale: &str) -> &'static str {
    if is_zh(locale) {
        "退出 UsageBar"
    } else {
        "Quit UsageBar"
    }
}

pub fn interval_label(locale: &str, sec: u64) -> String {
    if is_zh(locale) {
        match sec {
            0 => "关闭".into(),
            120 => "2 分钟".into(),
            300 => "5 分钟".into(),
            600 => "10 分钟".into(),
            n => format!("{n} 秒"),
        }
    } else {
        match sec {
            0 => "Off".into(),
            120 => "2 minutes".into(),
            300 => "5 minutes".into(),
            600 => "10 minutes".into(),
            n => format!("{n} seconds"),
        }
    }
}

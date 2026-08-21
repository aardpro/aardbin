//! Internationalisation: server-side translation table + language resolution.
//!
//! Resolution order: `aardbin_lang` Cookie → `Accept-Language` header → `en`.
//! Missing keys fall back to English with a `tracing::warn!`.

use std::collections::HashMap;
use std::sync::LazyLock;

/// Supported language codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    En,
    Zh,
}

impl Lang {
    pub fn as_str(&self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Zh => "zh",
        }
    }

    pub fn html_attr(&self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Zh => "zh-CN",
        }
    }
}

/// Translate `key` for `lang`.  Falls back to English; returns the key
/// itself if even English is missing (with a warning log).
pub fn t(lang: Lang, key: &str) -> String {
    if let Some(v) = table(lang).get(key) {
        return v.to_string();
    }
    if lang != Lang::En {
        if let Some(v) = table(Lang::En).get(key) {
            tracing::warn!(
                lang = lang.as_str(),
                key,
                "i18n key missing, fell back to en"
            );
            return v.to_string();
        }
        tracing::warn!(lang = lang.as_str(), key, "i18n key missing in all languages");
    }
    key.to_string()
}

fn table(lang: Lang) -> &'static HashMap<&'static str, &'static str> {
    match lang {
        Lang::En => &EN,
        Lang::Zh => &ZH,
    }
}

// ---------------------------------------------------------------------------
// Resolve from HTTP context
// ---------------------------------------------------------------------------

/// Determine language from cookie value + Accept-Language header.
pub fn resolve(cookie_val: Option<&str>, accept_lang: Option<&str>) -> Lang {
    // 1. Explicit cookie
    if let Some(c) = cookie_val {
        match c.trim() {
            "zh" | "zh-CN" | "zh-cn" => return Lang::Zh,
            "en" | "en-US" | "en-us" => return Lang::En,
            _ => {}
        }
    }
    // 2. Accept-Language first token
    if let Some(al) = accept_lang {
        for part in al.split(',') {
            let tag = part.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
            if tag.starts_with("zh") {
                return Lang::Zh;
            }
            if tag.starts_with("en") {
                return Lang::En;
            }
        }
    }
    // 3. Default
    Lang::En
}

/// Extract `aardbin_lang` cookie value from a Cookie header string.
pub fn extract_lang_cookie(cookie_header: Option<&str>) -> Option<&str> {
    let header = cookie_header?;
    for pair in header.split(';') {
        let pair = pair.trim();
        if let Some(val) = pair.strip_prefix("aardbin_lang=") {
            return Some(val);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Translation tables — keep keys in parity (test asserts this).
// ---------------------------------------------------------------------------

static EN: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    // -- base.html --
    m.insert("nav.new", "+ New");
    m.insert("nav.logout", "Logout");
    // -- login.html --
    m.insert("login.title", "aardbin · Login");
    m.insert("login.heading", "aardbin");
    m.insert("login.tagline", "Your private bin.");
    m.insert("login.access_key_label", "Access Key");
    m.insert("login.access_key_placeholder", "Access Key");
    m.insert("login.button", "Login");
    m.insert("login.error", "Invalid access key");
    // -- records.html --
    m.insert("records.empty", "Nothing here yet. Click ");
    m.insert("records.empty_new", "+ New");
    m.insert("records.empty_tail", " to drop something.");
    m.insert("records.undecryptable", "⚠ Unable to decrypt");
    m.insert("records.untitled", "Untitled");
    m.insert("records.copy", "Copy");
    m.insert("records.edit", "Edit");
    m.insert("records.delete", "Delete");
    m.insert("records.delete_confirm", "Delete this record?");
    m.insert("records.prev", "← Prev");
    m.insert("records.next", "Next →");
    m.insert("records.page_info", "Page {page} / {total_pages} · {total} records");
    // -- form.html --
    m.insert("form.title_label", "Title");
    m.insert("form.title_placeholder", "Optional title");
    m.insert("form.content_label", "Content");
    m.insert("form.content_placeholder", "Paste text, code, config...");
    m.insert("form.attachments", "Attachments");
    m.insert("form.max_each", "max {size} each");
    m.insert("form.dropzone", "Drop files here or click to select");
    m.insert("form.cancel", "Cancel");
    m.insert("form.save", "Save");
    m.insert("form.new_title", "New");
    m.insert("form.edit_title", "Edit");
    // -- attachments.html --
    m.insert("att.download", "Download");
    m.insert("att.delete_confirm", "Delete this attachment?");
    m.insert("att.delete_title", "Delete attachment");
    // -- JS / shared --
    m.insert("js.copied", "Copied");
    m.insert("js.copy_failed", "Copy failed");
    m.insert("js.just_now", "just now");
    m.insert("js.minute_ago", "1 minute ago");
    m.insert("js.minutes_ago", "{n} minutes ago");
    m.insert("js.hour_ago", "1 hour ago");
    m.insert("js.hours_ago", "{n} hours ago");
    m.insert("js.yesterday", "yesterday");
    m.insert("js.exceeds_max", "exceeds max");
    m.insert("js.skipped", "skipped");
    // -- rate limit --
    m.insert(
        "rate.too_many",
        "Too many attempts. Try again in {minutes} minute{plural}.",
    );
    m
});

static ZH: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    // -- base.html --
    m.insert("nav.new", "+ 新建");
    m.insert("nav.logout", "退出");
    // -- login.html --
    m.insert("login.title", "aardbin · 登录");
    m.insert("login.heading", "aardbin");
    m.insert("login.tagline", "你的私人记事本。");
    m.insert("login.access_key_label", "访问密钥");
    m.insert("login.access_key_placeholder", "访问密钥");
    m.insert("login.button", "登录");
    m.insert("login.error", "访问密钥无效");
    // -- records.html --
    m.insert("records.empty", "暂无内容。点击");
    m.insert("records.empty_new", "+ 新建");
    m.insert("records.empty_tail", "添加记录。");
    m.insert("records.undecryptable", "⚠ 无法解密");
    m.insert("records.untitled", "无标题");
    m.insert("records.copy", "复制");
    m.insert("records.edit", "编辑");
    m.insert("records.delete", "删除");
    m.insert("records.delete_confirm", "确定删除此记录？");
    m.insert("records.prev", "← 上页");
    m.insert("records.next", "下页 →");
    m.insert(
        "records.page_info",
        "第 {page} / {total_pages} 页 · {total} 条记录",
    );
    // -- form.html --
    m.insert("form.title_label", "标题");
    m.insert("form.title_placeholder", "可选标题");
    m.insert("form.content_label", "正文");
    m.insert("form.content_placeholder", "粘贴文本、代码、配置…");
    m.insert("form.attachments", "附件");
    m.insert("form.max_each", "每个最大 {size}");
    m.insert("form.dropzone", "拖放文件到此处或点击选择");
    m.insert("form.cancel", "取消");
    m.insert("form.save", "保存");
    m.insert("form.new_title", "新建");
    m.insert("form.edit_title", "编辑");
    // -- attachments.html --
    m.insert("att.download", "下载");
    m.insert("att.delete_confirm", "确定删除此附件？");
    m.insert("att.delete_title", "删除附件");
    // -- JS / shared --
    m.insert("js.copied", "已复制");
    m.insert("js.copy_failed", "复制失败");
    m.insert("js.just_now", "刚刚");
    m.insert("js.minute_ago", "1 分钟前");
    m.insert("js.minutes_ago", "{n} 分钟前");
    m.insert("js.hour_ago", "1 小时前");
    m.insert("js.hours_ago", "{n} 小时前");
    m.insert("js.yesterday", "昨天");
    m.insert("js.exceeds_max", "超过最大");
    m.insert("js.skipped", "已跳过");
    // -- rate limit --
    m.insert(
        "rate.too_many",
        "尝试次数过多，请在 {minutes} 分钟后重试。",
    );
    m
});

/// All known translation keys (for parity testing).
#[cfg(test)]
pub fn all_keys() -> &'static [&'static str] {
    &[
        "nav.new",
        "nav.logout",
        "login.title",
        "login.heading",
        "login.tagline",
        "login.access_key_label",
        "login.access_key_placeholder",
        "login.button",
        "login.error",
        "records.empty",
        "records.empty_new",
        "records.empty_tail",
        "records.undecryptable",
        "records.untitled",
        "records.copy",
        "records.edit",
        "records.delete",
        "records.delete_confirm",
        "records.prev",
        "records.next",
        "records.page_info",
        "form.title_label",
        "form.title_placeholder",
        "form.content_label",
        "form.content_placeholder",
        "form.attachments",
        "form.max_each",
        "form.dropzone",
        "form.cancel",
        "form.save",
        "form.new_title",
        "form.edit_title",
        "att.download",
        "att.delete_confirm",
        "att.delete_title",
        "js.copied",
        "js.copy_failed",
        "js.just_now",
        "js.minute_ago",
        "js.minutes_ago",
        "js.hour_ago",
        "js.hours_ago",
        "js.yesterday",
        "js.exceeds_max",
        "js.skipped",
        "rate.too_many",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_parity() {
        for &k in all_keys() {
            assert!(EN.contains_key(k), "EN missing key: {k}");
            assert!(ZH.contains_key(k), "ZH missing key: {k}");
        }
    }

    #[test]
    fn resolve_cookie_wins() {
        assert_eq!(resolve(Some("zh"), Some("en-US")), Lang::Zh);
        assert_eq!(resolve(Some("en"), Some("zh-CN")), Lang::En);
    }

    #[test]
    fn resolve_accept_language() {
        assert_eq!(
            resolve(None, Some("zh-CN,zh;q=0.9,en;q=0.8")),
            Lang::Zh
        );
        assert_eq!(resolve(None, Some("en-US,en;q=0.9")), Lang::En);
    }

    #[test]
    fn resolve_default_en() {
        assert_eq!(resolve(None, None), Lang::En);
        assert_eq!(resolve(None, Some("fr,de;q=0.9")), Lang::En);
    }

    #[test]
    fn fallback_to_en_for_missing_key() {
        assert_eq!(t(Lang::En, "nonexistent.key"), "nonexistent.key");
    }

    #[test]
    fn extract_cookie_works() {
        assert_eq!(
            extract_lang_cookie(Some("other=x; aardbin_lang=zh; more=y")),
            Some("zh")
        );
        assert_eq!(extract_lang_cookie(Some("other=x")), None);
        assert_eq!(extract_lang_cookie(None), None);
    }
}

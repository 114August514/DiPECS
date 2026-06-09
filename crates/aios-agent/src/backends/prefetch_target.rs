use aios_spec::ExtensionCategory;

const DOCUMENT_FALLBACK_URL: &str =
    "https://www.w3.org/WAI/ER/tests/xhtml/testfiles/resources/pdf/dummy.pdf";
const IMAGE_FALLBACK_URL: &str = "https://httpbin.org/image/png";
const VIDEO_FALLBACK_URL: &str = "https://www.w3schools.com/html/mov_bbb.mp4";
const AUDIO_FALLBACK_URL: &str = "https://www.w3schools.com/html/horse.mp3";
const ARCHIVE_FALLBACK_URL: &str =
    "https://github.com/114August514/DiPECS/archive/refs/heads/main.zip";
const CODE_FALLBACK_URL: &str =
    "https://raw.githubusercontent.com/114August514/DiPECS/main/README.md";
const OTHER_FALLBACK_URL: &str = "https://httpbin.org/json";

pub(crate) fn default_prefetch_target(
    extension_category: &ExtensionCategory,
    package_name: Option<&str>,
) -> String {
    let url = package_name
        .and_then(|package_name| known_package_url(package_name, extension_category))
        .unwrap_or_else(|| fallback_url(extension_category));
    format!("url:{url}")
}

pub(crate) fn looks_like_package_name(raw: &str) -> bool {
    raw.contains('.')
        && !raw.contains('/')
        && !raw.contains(':')
        && raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

fn known_package_url(
    package_name: &str,
    extension_category: &ExtensionCategory,
) -> Option<&'static str> {
    let package_name = package_name.trim().to_ascii_lowercase();

    if package_name.contains("lark") || package_name.contains("feishu") {
        return Some(match extension_category {
            ExtensionCategory::Document => "https://www.feishu.cn/docx/",
            _ => "https://www.feishu.cn/",
        });
    }

    if package_name == "com.google.android.apps.docs" || package_name.contains("drive") {
        return Some(match extension_category {
            ExtensionCategory::Image | ExtensionCategory::Video => "https://photos.google.com/",
            _ => "https://drive.google.com/drive/my-drive",
        });
    }

    if package_name.contains("dropbox") {
        return Some("https://www.dropbox.com/home");
    }

    if package_name.contains("skydrive") || package_name.contains("onedrive") {
        return Some("https://onedrive.live.com/");
    }

    if package_name.contains("slack") {
        return Some("https://app.slack.com/client");
    }

    if package_name.contains("telegram") {
        return Some("https://web.telegram.org/");
    }

    if package_name.contains("whatsapp") {
        return Some("https://www.whatsapp.com/");
    }

    if package_name.contains("tencent.mm") || package_name.contains("wechat") {
        return Some("https://weixin.qq.com/");
    }

    if package_name.contains("tencent.mobileqq") || package_name.contains("mobileqq") {
        return Some("https://im.qq.com/");
    }

    if package_name.contains("chrome") || package_name.contains("browser") {
        return Some("https://www.google.com/");
    }

    None
}

fn fallback_url(extension_category: &ExtensionCategory) -> &'static str {
    match extension_category {
        ExtensionCategory::Document => DOCUMENT_FALLBACK_URL,
        ExtensionCategory::Image => IMAGE_FALLBACK_URL,
        ExtensionCategory::Video => VIDEO_FALLBACK_URL,
        ExtensionCategory::Audio => AUDIO_FALLBACK_URL,
        ExtensionCategory::Archive => ARCHIVE_FALLBACK_URL,
        ExtensionCategory::Code => CODE_FALLBACK_URL,
        ExtensionCategory::Other | ExtensionCategory::Unknown => OTHER_FALLBACK_URL,
    }
}

#[cfg(test)]
mod tests {
    use super::{default_prefetch_target, looks_like_package_name};
    use aios_spec::ExtensionCategory;

    #[test]
    fn default_prefetch_target_uses_package_specific_url_when_known() {
        let target =
            default_prefetch_target(&ExtensionCategory::Document, Some("com.ss.android.lark"));
        assert_eq!(target, "url:https://www.feishu.cn/docx/");
    }

    #[test]
    fn default_prefetch_target_uses_extension_fallback_for_unknown_package() {
        let target = default_prefetch_target(&ExtensionCategory::Code, Some("com.example.custom"));
        assert_eq!(
            target,
            "url:https://raw.githubusercontent.com/114August514/DiPECS/main/README.md"
        );
    }

    #[test]
    fn package_name_heuristic_rejects_non_package_targets() {
        assert!(looks_like_package_name("com.example.files"));
        assert!(!looks_like_package_name("https://example.test/feed.json"));
        assert!(!looks_like_package_name("content://downloads/document/1"));
    }
}

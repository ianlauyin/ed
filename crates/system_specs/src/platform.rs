pub fn os_name() -> String {
    #[cfg(target_os = "macos")]
    {
        "macOS".to_string()
    }
    #[cfg(target_os = "linux")]
    {
        "Linux".to_string()
    }
}

/// Note: This might do blocking IO! Only call from background threads
pub fn os_version() -> String {
    cfg_select! {
       target_os = "linux" => {
           let content = std::fs::read_to_string("/etc/os-release")
               .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
               .unwrap_or_default();
           util::parse_os_release(&content).unwrap_or_else(|| "unknown".to_string())
       }
       target_os = "macos" => {
           static MACOS_VERSION_REGEX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
               regex::Regex::new(r"(\s*\(Build [^)]*[0-9]\))").unwrap()
           });
           use objc2_foundation::NSProcessInfo;
           let process_info = NSProcessInfo::processInfo();
           let version_nsstring = process_info.operatingSystemVersionString();
           // "Version 15.6.1 (Build 24G90)" -> "15.6.1 (Build 24G90)"
           let version_string = version_nsstring.to_string().replace("Version ", "");
           // "15.6.1 (Build 24G90)" -> "15.6.1"
           // "26.0.0 (Build 25A5349a)" -> unchanged (Beta or Rapid Security Response; ends with letter)
           MACOS_VERSION_REGEX
               .replace_all(&version_string, "")
               .to_string()
       }
    }
}

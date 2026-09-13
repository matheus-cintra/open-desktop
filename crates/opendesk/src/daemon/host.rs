const FALLBACK_HOSTNAME: &str = "opendesk";

pub fn hostname() -> String {
    #[cfg(target_os = "macos")]
    if let Ok(output) = std::process::Command::new("/usr/sbin/scutil")
        .args(["--get", "LocalHostName"])
        .output()
        && output.status.success()
        && let Ok(name) = String::from_utf8(output.stdout)
    {
        return name.trim().to_owned();
    }

    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| FALLBACK_HOSTNAME.to_owned())
}

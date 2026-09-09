const FALLBACK_HOSTNAME: &str = "opendesk";

pub fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| FALLBACK_HOSTNAME.to_owned())
}

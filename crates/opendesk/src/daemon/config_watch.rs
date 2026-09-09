use std::path::Path;

use anyhow::Context;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc::UnboundedSender;

pub struct ConfigChanged;

pub fn spawn_config_watch(
    config_path: &Path,
    events: UnboundedSender<ConfigChanged>,
) -> anyhow::Result<RecommendedWatcher> {
    let directory = config_path
        .parent()
        .context("config path has no parent directory")?
        .to_path_buf();
    std::fs::create_dir_all(&directory)
        .with_context(|| format!("creating {}", directory.display()))?;
    let watched_name = config_path.file_name().map(|name| name.to_owned());
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
        let Ok(event) = result else {
            return;
        };
        if !matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
        ) {
            return;
        }
        let touches_config = event
            .paths
            .iter()
            .any(|path| path.file_name().map(|name| name.to_owned()) == watched_name);
        if touches_config {
            let _ = events.send(ConfigChanged);
        }
    })
    .context("creating config watcher")?;
    watcher
        .watch(&directory, RecursiveMode::NonRecursive)
        .with_context(|| format!("watching {}", directory.display()))?;
    Ok(watcher)
}

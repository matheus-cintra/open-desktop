//! Validation, ray routing and optimistic, durable map revisions.
use opendesk_proto::{
    control::{PeerId, Side},
    map::{DesktopMap, Revision},
};
use std::path::Path;

pub fn validate(map: &DesktopMap) -> Result<(), String> {
    if map.screens.is_empty() || map.screens.len() > 64 {
        return Err("O mapa deve conter de 1 a 64 computadores".into());
    }
    for (i, a) in map.screens.iter().enumerate() {
        if a.width <= 0
            || a.height <= 0
            || a.width > 100_000
            || a.height > 100_000
            || a.x.abs_diff(0) > 1_000_000
            || a.y.abs_diff(0) > 1_000_000
            || a.name.len() > 256
        {
            return Err("Geometria ou nome inválido".into());
        }
        for b in &map.screens[..i] {
            if a.peer == b.peer {
                return Err("Computador duplicado".into());
            }
            if a.x < b.x + b.width
                && b.x < a.x + a.width
                && a.y < b.y + b.height
                && b.y < a.y + a.height
            {
                return Err("As telas não podem se sobrepor".into());
            }
            let corner_x = a.x == b.x + b.width || b.x == a.x + a.width;
            let corner_y = a.y == b.y + b.height || b.y == a.y + a.height;
            if corner_x && corner_y {
                return Err("Encaixe uma borda; apenas um canto não cria passagem".into());
            }
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq)]
pub struct Crossing {
    pub peer: PeerId,
    pub entry: Side,
    pub fraction: f32,
}

/// Cast an axis-aligned ray; unavailable screens do not obstruct it. Half-open
/// edge segments prevent a corner from becoming a diagonal passage.
pub fn resolve(
    map: &DesktopMap,
    from: PeerId,
    side: Side,
    fraction: f32,
    eligible: impl Fn(PeerId) -> bool,
) -> Option<Crossing> {
    if !fraction.is_finite() || !(0.0..1.0).contains(&fraction) || validate(map).is_err() {
        return None;
    }
    let a = map.screens.iter().find(|s| s.peer == from)?;
    let (along, edge) = match side {
        Side::Left => (a.y as f64 + fraction as f64 * a.height as f64, a.x),
        Side::Right => (
            a.y as f64 + fraction as f64 * a.height as f64,
            a.x + a.width,
        ),
        Side::Top => (a.x as f64 + fraction as f64 * a.width as f64, a.y),
        Side::Bottom => (
            a.x as f64 + fraction as f64 * a.width as f64,
            a.y + a.height,
        ),
    };
    map.screens
        .iter()
        .filter(|b| b.peer != from && eligible(b.peer))
        .filter_map(|b| {
            let (start, length, distance) = match side {
                Side::Left => (b.y, b.height, edge - b.x - b.width),
                Side::Right => (b.y, b.height, b.x - edge),
                Side::Top => (b.x, b.width, edge - b.y - b.height),
                Side::Bottom => (b.x, b.width, b.y - edge),
            };
            (distance >= 0 && along >= start as f64 && along < (start + length) as f64).then_some((
                distance,
                b.peer,
                (along - start as f64) as f32 / length as f32,
            ))
        })
        .min_by_key(|(distance, peer, _)| (*distance, *peer))
        .map(|(_, peer, fraction)| Crossing {
            peer,
            entry: side.opposite(),
            fraction,
        })
}

pub fn next_revision(current: Option<Revision>, author: PeerId) -> Result<Revision, String> {
    Ok(Revision {
        counter: current
            .map_or(0, |r| r.counter)
            .checked_add(1)
            .ok_or("Revisão esgotada")?,
        author,
    })
}

pub fn load(path: &Path) -> Result<Option<DesktopMap>, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let map = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    validate(&map)?;
    Ok(Some(map))
}

pub fn save(path: &Path, map: &DesktopMap) -> Result<(), String> {
    use std::io::Write;
    validate(map)?;
    let parent = path.parent().ok_or("Mapa sem diretório")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("json.pending");
    let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(map).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())?;
    std::fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}

/// Persist a Lamport high-water mark separately from the map revision. Restarting
/// never restores control ownership, but must not reuse an old control counter.
pub fn load_control_clock(path: &Path) -> Result<u64, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(e.to_string()),
    }
}
pub fn save_control_clock(path: &Path, counter: u64) -> Result<(), String> {
    use std::io::Write;
    let parent = path.parent().ok_or("Contador sem diretório")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("pending");
    let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
    file.write_all(counter.to_string().as_bytes())
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    std::fs::rename(temporary, path).map_err(|e| e.to_string())?;
    std::fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}

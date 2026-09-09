use std::io::Write;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};

use rustix::fs::MemfdFlags;

use crate::error::WaylandError;

pub fn read_keymap(fd: &impl AsFd, size: u32) -> Result<String, WaylandError> {
    let length = usize::try_from(size).map_err(|_| WaylandError::KeymapNotUtf8)?;
    let mapping = map_read_only(fd, length)?;
    let end = mapping
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(mapping.len());
    let text = std::str::from_utf8(&mapping[..end]).map_err(|_| WaylandError::KeymapNotUtf8)?;
    Ok(text.to_owned())
}

fn map_read_only(fd: &impl AsFd, length: usize) -> Result<memmap2::Mmap, WaylandError> {
    let raw_fd = fd.as_fd().as_raw_fd();
    let mapping = unsafe { memmap2::MmapOptions::new().len(length).map(raw_fd) }?;
    Ok(mapping)
}

pub fn keymap_memfd(xkb: &str) -> Result<(OwnedFd, u32), WaylandError> {
    let fd = rustix::fs::memfd_create("opendesk-keymap", MemfdFlags::CLOEXEC)
        .map_err(std::io::Error::from)?;
    let mut file = std::fs::File::from(fd);
    file.write_all(xkb.as_bytes())?;
    file.write_all(&[0])?;
    file.flush()?;
    let size = u32::try_from(xkb.len() + 1).map_err(|_| WaylandError::KeymapNotUtf8)?;
    Ok((OwnedFd::from(file), size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memfd_round_trips_the_keymap_text() {
        let text = "xkb_keymap {\n\txkb_keycodes { include \"evdev\" };\n};\n";
        let (fd, size) = keymap_memfd(text).unwrap();
        assert_eq!(size as usize, text.len() + 1);
        assert_eq!(read_keymap(&fd, size).unwrap(), text);
    }

    #[test]
    fn read_stops_at_the_first_nul_byte() {
        let (fd, size) = keymap_memfd("abc").unwrap();
        assert_eq!(read_keymap(&fd, size).unwrap(), "abc");
    }
}

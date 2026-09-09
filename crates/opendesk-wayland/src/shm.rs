use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use wayland_client::protocol::wl_shm;

use crate::error::WaylandError;

const BYTES_PER_PIXEL: i32 = 4;

pub fn transparent_buffer(
    pool: &mut SlotPool,
    width: u32,
    height: u32,
) -> Result<Buffer, WaylandError> {
    let width = i32::try_from(width.max(1)).unwrap_or(i32::MAX);
    let height = i32::try_from(height.max(1)).unwrap_or(i32::MAX);
    let stride = width.saturating_mul(BYTES_PER_PIXEL);
    let (buffer, canvas) = pool.create_buffer(width, height, stride, wl_shm::Format::Argb8888)?;
    canvas.fill(0);
    Ok(buffer)
}

use std::ops::Range;
use std::path::Path;
use std::process::Command;

use opendesk_proto::control::Side;

use crate::spike_support::SpikeResult;

pub type Rgb = [u8; 3];

pub struct Capture {
    pub width: usize,
    pub height: usize,
    rgb: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct EdgeRun {
    pub matching: usize,
    pub longest: usize,
    pub start: usize,
    pub end: usize,
}

impl EdgeRun {
    pub fn center(&self) -> f64 {
        (self.start + self.end) as f64 / 2.0
    }
}

impl Capture {
    pub fn grab(output: &str) -> SpikeResult<Capture> {
        let result = Command::new("grim")
            .args(["-t", "ppm", "-o", output, "-"])
            .output()?;
        if !result.status.success() {
            return Err(format!("grim failed: {}", String::from_utf8_lossy(&result.stderr)).into());
        }
        parse_ppm(&result.stdout)
    }

    pub fn save_png(output: &str, path: &Path) -> SpikeResult<()> {
        let status = Command::new("grim")
            .arg("-o")
            .arg(output)
            .arg(path)
            .status()?;
        if !status.success() {
            return Err(format!("grim could not write {}", path.display()).into());
        }
        Ok(())
    }

    pub fn pixel(&self, x: usize, y: usize) -> Rgb {
        let offset = (y * self.width + x) * 3;
        [self.rgb[offset], self.rgb[offset + 1], self.rgb[offset + 2]]
    }

    pub fn edge_run(
        &self,
        side: Side,
        window: Range<usize>,
        matches: impl Fn(Rgb) -> bool,
    ) -> EdgeRun {
        let pixels: Vec<(usize, Rgb)> = match side {
            Side::Left => window.map(|y| (y, self.pixel(1, y))).collect(),
            Side::Right => window.map(|y| (y, self.pixel(self.width - 2, y))).collect(),
            Side::Top => window.map(|x| (x, self.pixel(x, 1))).collect(),
            Side::Bottom => window
                .map(|x| (x, self.pixel(x, self.height - 2)))
                .collect(),
        };
        let mut run = EdgeRun::default();
        let mut current_start = 0;
        let mut current_length = 0;
        for (index, pixel) in pixels {
            if matches(pixel) {
                run.matching += 1;
                if current_length == 0 {
                    current_start = index;
                }
                current_length += 1;
                if current_length > run.longest {
                    run.longest = current_length;
                    run.start = current_start;
                    run.end = index;
                }
            } else {
                current_length = 0;
            }
        }
        run
    }
}

pub fn within(pixel: Rgb, expected: Rgb, tolerance: u8) -> bool {
    pixel
        .iter()
        .zip(expected.iter())
        .all(|(actual, wanted)| actual.abs_diff(*wanted) <= tolerance)
}

pub fn blend(bar: Rgb, alpha: u8, background: Rgb) -> Rgb {
    let opacity = f64::from(alpha) / 255.0;
    let mut blended = [0; 3];
    for channel in 0..3 {
        let value =
            opacity * f64::from(bar[channel]) + (1.0 - opacity) * f64::from(background[channel]);
        blended[channel] = value.round() as u8;
    }
    blended
}

fn parse_ppm(bytes: &[u8]) -> SpikeResult<Capture> {
    let mut cursor = 0;
    let mut fields = Vec::new();
    while fields.len() < 4 {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let start = cursor;
        while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if start == cursor {
            return Err("truncated ppm header".into());
        }
        fields.push(String::from_utf8_lossy(&bytes[start..cursor]).into_owned());
    }
    cursor += 1;
    if fields[0] != "P6" || fields[3] != "255" {
        return Err(format!("unsupported ppm header {fields:?}").into());
    }
    let width: usize = fields[1].parse()?;
    let height: usize = fields[2].parse()?;
    let rgb = bytes
        .get(cursor..cursor + width * height * 3)
        .ok_or("ppm data shorter than the header claims")?
        .to_vec();
    Ok(Capture { width, height, rgb })
}

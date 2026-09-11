#[path = "../spike_support/mod.rs"]
mod spike_support;

mod capture;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use capture::{Capture, EdgeRun, Rgb, blend, within};
use opendesk_proto::control::{OutputGeometry, Side};
use opendesk_wayland::{BarStyle, StripSpec, WaylandCommand, WaylandEvent};
use spike_support::{
    Spike, SpikeResult, cursor_position, first_output, hyprctl, park_cursor_at_center, sleep_millis,
};

const STYLE: BarStyle = BarStyle {
    red: 0x5e,
    green: 0x81,
    blue: 0xac,
    alpha: 0xCC,
};
const LAYERRULE: &str = "no_anim on, match:namespace ^opendesk-bar$";
const COLOR_TOLERANCE: u8 = 24;
const BACKGROUND_TOLERANCE: u8 = 12;
const ROW_TOLERANCE: usize = 6;
const PROGRESS_30_ROWS: usize = 94;
const PROGRESS_90_ROWS: usize = 202;
const ARRIVAL_ROWS: usize = 220;
const WINDOW_HALF_SPAN: usize = 250;

struct Bench {
    output: OutputGeometry,
    background: Rgb,
    expected: Rgb,
    png_dir: PathBuf,
}

impl Bench {
    fn measure_solid(&self, side: Side, label: &str) -> SpikeResult<EdgeRun> {
        let expected = self.expected;
        self.measure(side, label, "solid", move |pixel| {
            within(pixel, expected, COLOR_TOLERANCE)
        })
    }

    fn measure_visible(&self, side: Side, label: &str) -> SpikeResult<EdgeRun> {
        let background = self.background;
        self.measure(side, label, "visible", move |pixel| {
            !within(pixel, background, BACKGROUND_TOLERANCE)
        })
    }

    fn measure(
        &self,
        side: Side,
        label: &str,
        kind: &str,
        matches: impl Fn(Rgb) -> bool,
    ) -> SpikeResult<EdgeRun> {
        let capture = Capture::grab(&self.output.name)?;
        let middle = (self.output.height / 2) as usize;
        let window = middle.saturating_sub(WINDOW_HALF_SPAN)..middle + WINDOW_HALF_SPAN;
        let run = capture.edge_run(side, window.clone(), matches);
        println!(
            "{label}: {side} edge rows {}..{} {kind} {} longest run {} ({}..={}, center {:.1})",
            window.start,
            window.end,
            run.matching,
            run.longest,
            run.start,
            run.end,
            run.center()
        );
        Ok(run)
    }

    fn save_png(&self, name: &str) -> SpikeResult<PathBuf> {
        let path = self.png_dir.join(name);
        Capture::save_png(&self.output.name, &path)?;
        println!("png: {}", path.display());
        Ok(path)
    }

    fn run_matches(&self, run: &EdgeRun, expected_rows: usize) -> bool {
        let middle = f64::from(self.output.height) / 2.0;
        run.longest.abs_diff(expected_rows) <= ROW_TOLERANCE
            && (run.center() - middle).abs() <= ROW_TOLERANCE as f64
    }
}

fn sample_colors(output: &OutputGeometry) -> SpikeResult<(Rgb, Rgb)> {
    let capture = Capture::grab(&output.name)?;
    let background = capture.pixel(capture.width / 4, capture.height / 4);
    let expected = blend(
        [STYLE.red, STYLE.green, STYLE.blue],
        STYLE.alpha,
        background,
    );
    println!(
        "capture {}x{}, background {background:?}, expected bar color {expected:?}",
        capture.width, capture.height
    );
    Ok((background, expected))
}

fn main() -> SpikeResult<()> {
    let (mut spike, outputs) = Spike::start()?;
    let output = first_output(&outputs)?;
    println!(
        "output {} {}x{} at {},{}",
        output.name, output.width, output.height, output.x, output.y
    );
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")?;
    let png_dir = Path::new(&runtime_dir).join("opendesk-nested-bar");
    std::fs::create_dir_all(&png_dir)?;
    let middle_y = f64::from(output.y + output.height / 2);
    let edge_x = f64::from(output.x + output.width - 1);

    println!(
        "layerrule `{LAYERRULE}`: {}",
        hyprctl(&["keyword", "layerrule", LAYERRULE])?
    );
    park_cursor_at_center(&spike, &output)?;
    let (background, expected) = sample_colors(&output)?;
    let bench = Bench {
        output: output.clone(),
        background,
        expected,
        png_dir,
    };
    spike.send(WaylandCommand::SetBarStyle { style: STYLE })?;

    spike.send(WaylandCommand::ShowProgressBar {
        side: Side::Right,
        position: middle_y,
        progress: 0.3,
    })?;
    sleep_millis(300);
    let run = bench.measure_solid(Side::Right, "progress 0.3")?;
    bench.save_png("bar-30.png")?;
    spike.check(
        "progress 0.3 draws ~94 rows centered on the right edge",
        bench.run_matches(&run, PROGRESS_30_ROWS),
    );

    spike.send(WaylandCommand::ShowProgressBar {
        side: Side::Right,
        position: middle_y,
        progress: 0.9,
    })?;
    sleep_millis(300);
    let run = bench.measure_solid(Side::Right, "progress 0.9")?;
    bench.save_png("bar-90.png")?;
    spike.check(
        "progress 0.9 draws ~202 rows centered on the right edge",
        bench.run_matches(&run, PROGRESS_90_ROWS),
    );

    spike.send(WaylandCommand::HideProgressBar)?;
    sleep_millis(200);
    let run = bench.measure_visible(Side::Right, "hidden")?;
    spike.check("HideProgressBar leaves 0 visible rows", run.matching == 0);

    let shown_at = Instant::now();
    spike.send(WaylandCommand::ShowArrivalBar {
        side: Side::Left,
        position: middle_y,
    })?;
    sleep_millis(100);
    let run = bench.measure_visible(Side::Left, "arrival at 100 ms")?;
    bench.save_png("arrival.png")?;
    spike.check(
        "arrival bar draws ~220 rows centered on the left edge",
        bench.run_matches(&run, ARRIVAL_ROWS),
    );
    let mid_fade = bench.measure_solid(Side::Left, "arrival while fading")?;
    spike.check(
        "arrival bar is already dimmer than the solid color while fading",
        mid_fade.matching == 0,
    );
    let deadline = shown_at + Duration::from_millis(900);
    std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
    let run = bench.measure_visible(Side::Left, "arrival at 900 ms")?;
    spike.check("arrival bar is gone after 900 ms", run.matching == 0);

    spike.send(WaylandCommand::ConfigureStrips {
        strips: vec![StripSpec {
            side: Side::Right,
            output: output.name.clone(),
        }],
    })?;
    sleep_millis(300);
    spike.send(WaylandCommand::ShowProgressBar {
        side: Side::Right,
        position: middle_y,
        progress: 0.5,
    })?;
    sleep_millis(300);
    spike.collect_for(Duration::from_millis(50));
    spike.send(WaylandCommand::InjectAbsoluteMotion {
        x: edge_x,
        y: middle_y,
    })?;
    let entered = spike.wait_for(Duration::from_secs(1), |event| {
        matches!(event, WaylandEvent::EdgeEntered { .. })
    });
    println!(
        "edge event under the bar: {entered:?}, cursorpos: {:?}",
        cursor_position()?
    );
    spike.check(
        "EdgeEntered still arrives with the progress bar over the strip",
        entered.is_some(),
    );
    spike.send(WaylandCommand::HideProgressBar)?;

    spike.finish()
}

//! A car dashboard in a terminal: a routable city, A* navigation, and a map drawn from geometry
//! every frame rather than from tiles.
//!
//! The map pane is real pixels where the terminal can take them — kitty, Ghostty, WezTerm — and
//! braille where it cannot. Everything else is terminal cells in both cases, which is the whole
//! argument: pixels for geometry, cells for language.
//!
//! ```text
//! cargo run --release                 drive, at 60 fps
//! cargo run --release -- --time       what a frame costs, in milliseconds, by thread count
//! cargo run --release -- --probe      three patches, to find out what the terminal will show
//! cargo run --release -- --inline     send the pixels in the escape, not through a temp file
//! cargo run --release -- --light      paper, for sunlight, instead of night
//! cargo run --release -- --braille    drive without the picture
//! cargo run -- --dump 160 44          one frame as text, to check the layout
//! cargo run -- --png /tmp/dash.png    one frame as a picture, to check the render
//! ```

mod cabin;
mod dash;
mod dots;
mod hud;
mod kitty;
mod paint;
mod png;
mod route;
mod scene;
mod skin;
mod ui;
mod view;
mod world;

use std::io::{self, Write};
use std::process::ExitCode;

use conui::headless::Screen;
use conui::{App, Config, Event};

use crate::dash::{Dash, Search};
use crate::kitty::Medium;
use crate::paint::Surface;
use crate::skin::Skin;
use crate::ui::Paint;

/// What a cell is assumed to be when the tty will not say and `--cell` was not given.
///
/// The image is placed by cell count, so a wrong guess costs a few percent of aspect error and some
/// scaling — not a broken screen.
const CELL: (u16, u16) = (8, 17);

/// Frames a second, by default.
///
/// A frame of map is drawn on every core at once, which at a typical window costs about two
/// milliseconds of the sixteen this allows — so the figure is chosen for how the motion looks rather
/// than for what the renderer can manage. `--time` prints the real numbers for any given size.
const FPS: u32 = 60;

/// Not `-> io::Result<()>`, which would be shorter and would print the wrong thing.
///
/// Rust's own `Termination` for a `Result` prints the error's `Debug`, so the one message this program
/// most often has to give — that there is no terminal to take over — arrived as
/// `Error: Custom { kind: Unsupported, error: "..." }`. The sentence was in there, wrapped in a struct
/// nobody outside this process has any business seeing.
fn main() -> ExitCode {
    match dispatch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("dash: {error}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|arg| arg == name);
    let value = |name: &str| {
        args.iter().position(|arg| arg == name).and_then(|at| args.get(at + 1)).cloned()
    };
    let cell = value("--cell")
        .and_then(|text| parse_size(&text))
        .or_else(kitty::Graphics::cell_size)
        .unwrap_or(CELL);

    // Night or paper. The hues are the same in both; what changes is which end of the scale the ink
    // is on, which is the only thing a change of ambient light actually demands.
    let skin = Skin::pick(flag("--light"));

    if flag("--probe") {
        return kitty::probe();
    }
    if let Some(at) = args.iter().position(|arg| arg == "--dump") {
        let (width, height, seconds) = frame_args(&args, at);
        return dump(width, height, seconds, value("--find").as_deref(), skin);
    }
    if let Some(at) = args.iter().position(|arg| arg == "--png") {
        let path = args.get(at + 1).cloned().unwrap_or_else(|| "dash.png".to_string());
        let (width, height, seconds) = frame_args(&args, at + 1);
        return still(&path, width, height, seconds, cell, skin);
    }
    if let Some(at) = args.iter().position(|arg| arg == "--time") {
        let (width, height, _) = frame_args(&args, at);
        return time(width, height, cell, skin);
    }

    let fps = value("--fps").and_then(|text| text.parse().ok()).unwrap_or(FPS);
    let pixels = !flag("--braille") && kitty::Graphics::available();
    let medium = if flag("--inline") { Medium::Inline } else { Medium::File };
    drive(fps, pixels, cell, medium, skin)
}

/// One frame of map, drawn on every core this machine will admit to.
fn draw(surface: &mut Surface, dash: &Dash, plan: &ui::Plan, cores: u32, skin: &Skin) {
    let bands = surface.bands(cores);
    surface.paint(bands, |band| {
        scene::render(
            band,
            &dash.world,
            dash.route.as_ref(),
            &plan.view,
            dash.heading,
            &plan.labels,
            plan.card.as_ref(),
            plan.gauge.as_ref(),
            &skin.map,
        )
    });
}

/// How many threads to draw a frame with. One is a perfectly good answer.
fn cores() -> u32 {
    std::thread::available_parallelism().map_or(1, |cores| cores.get() as u32)
}

fn drive(
    fps: u32,
    pixels: bool,
    cell: (u16, u16),
    medium: Medium,
    skin: &'static Skin,
) -> io::Result<()> {
    let cores = cores();
    let mut dash = Dash::new();
    let paint = if pixels { Paint::Pixels { cell } } else { Paint::Dots };
    let mut graphics = pixels.then(|| kitty::Graphics::new(medium));
    let mut surface: Option<Surface> = None;

    let mut app =
        App::with(Config::new().theme(skin.theme).fps(fps).min_size(84, 30).mouse(false))?;
    let mut last = app.elapsed();

    while app.is_running() {
        for event in app.poll()? {
            match event {
                Event::Key(key) => dash.key(key),
                Event::Resize { .. } => {
                    // The old placement is the old size. Take it down rather than scale it.
                    if let Some(graphics) = &mut graphics {
                        graphics.clear()?;
                    }
                    surface = None;
                    app.force_repaint();
                }
                _ => {}
            }
        }

        // The clock drives the car, not the frame count, so a slow terminal makes the animation
        // coarser rather than slower.
        let now = app.elapsed();
        let step = (now.saturating_sub(last)).as_secs_f32().min(0.25);
        last = now;
        dash.tick(step);

        let (width, height) = app.size();
        let plan = ui::plan(&dash, width, height, paint, skin);

        if let Some(graphics) = &mut graphics {
            if app.is_too_small() {
                graphics.clear()?;
                surface = None;
            } else {
                let (pw, ph) = plan.pixels();
                if surface.as_ref().is_some_and(|had| (had.width, had.height) != (pw, ph)) {
                    surface = None;
                }
                let canvas = surface.get_or_insert_with(|| Surface::new(pw, ph, skin.map.ground));
                draw(canvas, &dash, &plan, cores, skin);
                graphics.show(canvas, plan.map.x, plan.map.y, plan.map.width, plan.map.height)?;
                // conui moves the cursor by relative jumps from where it thinks it is, and the
                // escape above moved it. Tell it to stop believing itself.
                app.terminal().painter().invalidate();
            }
        }

        app.draw(|frame| ui::compose(frame, &dash, &plan, skin))?;
        if dash.quit {
            app.quit();
        }
    }

    if let Some(graphics) = &mut graphics {
        graphics.clear()?;
    }
    app.leave()
}

/// One frame, as plain text, after `seconds` of driving.
///
/// `find` seeds the search overlay, which is the only way to see where it lands without a keyboard.
fn dump(
    width: u16,
    height: u16,
    seconds: f32,
    find: Option<&str>,
    skin: &'static Skin,
) -> io::Result<()> {
    let mut dash = driven(seconds);
    if let Some(query) = find {
        let hits = dash.matches(query);
        dash.search = Some(Search { text: query.to_string(), hits, cursor: 0 });
    }
    let dash = dash;
    let plan = ui::plan(&dash, width, height, Paint::Dots, skin);

    let mut screen = Screen::themed(width, height, skin.theme);
    screen.draw(|frame| ui::compose(frame, &dash, &plan, skin));
    // One write rather than a `print!` per row: a dump exists to be piped, and `--dump | head -3`
    // would abort on the broken pipe partway through.
    io::stdout().write_all(screen.to_string().as_bytes())
}

/// One map pane, as a PNG, which is how the pixel render gets checked without a terminal.
fn still(
    path: &str,
    width: u16,
    height: u16,
    seconds: f32,
    cell: (u16, u16),
    skin: &'static Skin,
) -> io::Result<()> {
    let dash = driven(seconds);
    let plan = ui::plan(&dash, width, height, Paint::Pixels { cell }, skin);
    let (pw, ph) = plan.pixels();
    let mut surface = Surface::new(pw, ph, skin.map.ground);
    draw(&mut surface, &dash, &plan, cores(), skin);
    std::fs::write(path, png::encode(&surface))?;
    println!("{path}: {}x{} px", surface.width, surface.height);
    Ok(())
}

/// What a frame actually costs, broken down, because a frame rate is a claim about milliseconds.
///
/// Needs no terminal, so it can be run anywhere and compared. The three figures are the three things
/// that happen per frame: work out what to draw, draw it, hand it over.
fn time(width: u16, height: u16, cell: (u16, u16), skin: &'static Skin) -> io::Result<()> {
    use std::time::Instant;

    const FRAMES: usize = 120;

    let dash = driven(20.0);
    let plan = ui::plan(&dash, width, height, Paint::Pixels { cell }, skin);
    let (pw, ph) = plan.pixels();
    let mut surface = Surface::new(pw, ph, skin.map.ground);
    let path = std::env::temp_dir().join("tty-graphics-protocol-dash-time.rgb");

    println!(
        "{width}x{height} cells at {}x{} → {pw}x{ph} px ({:.1} Mpx), {FRAMES} frames each",
        cell.0,
        cell.1,
        (pw * ph) as f32 / 1e6,
    );
    println!("  {:<7}{:>10}{:>10}{:>12}", "bands", "draw", "p95", "with write");

    let cores = cores();
    let mut counts = vec![1];
    for bands in [2, 4, 8, cores, cores * 2] {
        let bands = surface.bands(bands);
        if !counts.contains(&bands) {
            counts.push(bands);
        }
    }

    for bands in counts {
        let mut drawing = Vec::with_capacity(FRAMES);
        let mut writing = Vec::with_capacity(FRAMES);
        for _ in 0..FRAMES {
            let start = Instant::now();
            surface.paint(bands, |band| {
                scene::render(
                    band,
                    &dash.world,
                    dash.route.as_ref(),
                    &plan.view,
                    dash.heading,
                    &plan.labels,
                    plan.card.as_ref(),
                    plan.gauge.as_ref(),
                    &skin.map,
                )
            });
            drawing.push(start.elapsed());
            let start = Instant::now();
            std::fs::write(&path, &surface.pixels)?;
            writing.push(start.elapsed());
        }
        drawing.sort();
        writing.sort();
        let at = |taken: &[std::time::Duration], part: f32| {
            taken[(taken.len() as f32 * part) as usize].as_secs_f32() * 1e3
        };
        let (median, worst, write) = (at(&drawing, 0.5), at(&drawing, 0.95), at(&writing, 0.5));
        println!(
            "  {bands:<7}{median:>7.2} ms{worst:>7.2} ms{:>9.2} ms   → {:.0} fps",
            median + write,
            1e3 / (median + write),
        );
    }
    let _ = std::fs::remove_file(&path);
    println!("  plan {:.3} ms — the camera and the labels, which is not where the time goes", {
        let start = Instant::now();
        for _ in 0..FRAMES {
            let _ = ui::plan(&dash, width, height, Paint::Pixels { cell }, skin);
        }
        start.elapsed().as_secs_f32() * 1e3 / FRAMES as f32
    });
    Ok(())
}

fn driven(seconds: f32) -> Dash {
    let mut dash = Dash::new();
    let mut clock = 0.0;
    while clock < seconds {
        dash.tick(1.0 / 30.0);
        clock += 1.0 / 30.0;
    }
    dash
}

/// `W H [SECONDS]` following a flag, with the defaults a terminal usually has.
fn frame_args(args: &[String], at: usize) -> (u16, u16, f32) {
    let word = |offset: usize| args.get(at + offset);
    let cells = |offset: usize| word(offset).and_then(|arg| arg.parse::<u16>().ok());
    let seconds = word(3).and_then(|arg| arg.parse::<f32>().ok());
    (cells(1).unwrap_or(160), cells(2).unwrap_or(44), seconds.unwrap_or(25.0))
}

fn parse_size(text: &str) -> Option<(u16, u16)> {
    let (width, height) = text.split_once(['x', 'X'])?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

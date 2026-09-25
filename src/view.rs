//! Where the camera is and what gets named, shared by both ways of drawing the map.
//!
//! The projection has to be the same for the braille fallback and the pixel render, or a street name
//! would sit next to the street it names in one of them. So it lives here once, counting in whatever
//! units the pane uses — braille dots or pixels — and told how big a cell is in those units.

use conui::Role;

use crate::route::Route;
use crate::world::{Class, World, distance};

pub struct Camera {
    pub at: (f32, f32),
    /// Radians, as [`Route::locate`] reports it.
    pub heading: f32,
    pub north_up: bool,
    /// How much of the world the pane spans, west to east.
    pub metres_across: f32,
}

/// Where the car sits in the pane, as a fraction of its width and height.
///
/// Heading-up puts it low, because what is behind you is not navigation. North-up cannot: any
/// direction may be the one being driven, so the car goes in the middle. The low figure is the one
/// anything avoiding the car has to clear, and it is a constant rather than a number the layout
/// re-derives, or a plate would sit on the car in one of the two orientations.
pub const CAR: (f32, f32) = (0.5, 0.72);

pub struct Projector {
    sin: f32,
    cos: f32,
    scale: f32,
    at: (f32, f32),
    anchor: (f32, f32),
    cell: (f32, f32),
    pub width: f32,
    pub height: f32,
}

impl Projector {
    /// `width` and `height` are the pane in its own units; `cell` is one terminal cell in them.
    pub fn new(camera: &Camera, width: f32, height: f32, cell: (f32, f32)) -> Projector {
        let rotation =
            if camera.north_up { 0.0 } else { -camera.heading - std::f32::consts::FRAC_PI_2 };
        let down = if camera.north_up { 0.5 } else { CAR.1 };

        Projector {
            sin: rotation.sin(),
            cos: rotation.cos(),
            scale: width / camera.metres_across,
            at: camera.at,
            anchor: (width * CAR.0, height * down),
            cell,
            width,
            height,
        }
    }

    /// Pane units per metre.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn anchor(&self) -> (f32, f32) {
        self.anchor
    }

    /// One terminal cell, in this pane's units.
    pub fn cell(&self) -> (f32, f32) {
        self.cell
    }

    /// The cell the camera itself is in, which is where the car goes.
    pub fn anchor_cell(&self) -> (i32, i32) {
        ((self.anchor.0 / self.cell.0) as i32, (self.anchor.1 / self.cell.1) as i32)
    }

    pub fn project(&self, point: (f32, f32)) -> (f32, f32) {
        let (dx, dy) = (point.0 - self.at.0, point.1 - self.at.1);
        (
            self.anchor.0 + (dx * self.cos - dy * self.sin) * self.scale,
            self.anchor.1 + (dx * self.sin + dy * self.cos) * self.scale,
        )
    }

    pub fn unproject(&self, point: (f32, f32)) -> (f32, f32) {
        let (dx, dy) =
            ((point.0 - self.anchor.0) / self.scale, (point.1 - self.anchor.1) / self.scale);
        (self.at.0 + dx * self.cos + dy * self.sin, self.at.1 - dx * self.sin + dy * self.cos)
    }

    /// The cell a world point falls in, which is the only place text can go.
    pub fn cell_of(&self, point: (f32, f32)) -> (i32, i32) {
        let (x, y) = self.project(point);
        ((x / self.cell.0).floor() as i32, (y / self.cell.1).floor() as i32)
    }

    pub fn middle(&self) -> (f32, f32) {
        self.unproject((self.width * 0.5, self.height * 0.5))
    }

    /// Both ends of a straight street, far enough out to cross any view of it.
    pub fn street_ends(
        &self,
        world: &World,
        street: &crate::world::Street,
        down: bool,
    ) -> [(f32, f32); 2] {
        let span = world.width.max(world.height) * 1.5;
        if down {
            [
                (street.offset - street.slope * span, -span),
                (street.offset + street.slope * span * 2.0, span * 2.0),
            ]
        } else {
            [
                (-span, street.offset - street.slope * span),
                (span * 2.0, street.offset + street.slope * span * 2.0),
            ]
        }
    }
}

pub struct Label {
    pub col: u16,
    pub row: u16,
    pub text: String,
    pub role: Role,
}

/// Text on the map, and the row occupancy that stops two names becoming one long one.
pub struct Labels {
    cols: u16,
    rows: u16,
    taken: Vec<bool>,
    placed: Vec<Label>,
}

impl Labels {
    pub fn new(cols: u16, rows: u16) -> Labels {
        Labels {
            cols,
            rows,
            taken: vec![false; usize::from(cols) * usize::from(rows)],
            placed: Vec::new(),
        }
    }

    pub fn placed(&self) -> &[Label] {
        &self.placed
    }

    /// Keep a run of cells clear, for something that is not text — the car, say.
    pub fn reserve(&mut self, col: i32, row: i32, width: u16) {
        if row < 0 || row >= i32::from(self.rows) {
            return;
        }
        for offset in 0..i32::from(width) {
            let at = col + offset;
            if at >= 0 && at < i32::from(self.cols) {
                self.taken[row as usize * usize::from(self.cols) + at as usize] = true;
            }
        }
    }

    /// A label, placed only if it fits and nothing else is there.
    pub fn place(&mut self, col: i32, row: i32, text: &str, role: Role, marker: bool) -> bool {
        let shown = if marker { format!("◆ {text}") } else { text.to_string() };
        let width = shown.chars().count() as u16;
        if row < 0 || row >= i32::from(self.rows) || col < 0 {
            return false;
        }
        let (col, row) = (col as u16, row as u16);
        if col + width + 1 >= self.cols {
            return false;
        }
        // One cell of slack each side, so two labels never end up looking like one.
        let from = col.saturating_sub(1);
        let to = (col + width + 1).min(self.cols);
        let row_at = usize::from(row) * usize::from(self.cols);
        if (from..to).any(|c| self.taken[row_at + usize::from(c)]) {
            return false;
        }
        for c in from..to {
            self.taken[row_at + usize::from(c)] = true;
        }
        self.placed.push(Label { col, row, text: shown, role });
        true
    }
}

/// Everything worth naming in this view, in the order it deserves the space.
pub fn name_the_view(
    world: &World,
    route: Option<&Route>,
    projector: &Projector,
    labels: &mut Labels,
) {
    // The car first, so nothing is ever written across it.
    let (col, row) = projector.anchor_cell();
    labels.reserve(col - 1, row, 3);

    if let Some(route) = route {
        let end = *route.points.last().expect("a route has a last point");
        let (col, row) = projector.cell_of(end);
        labels.place(col, row, &route.destination, Role::Accent, true);
    }

    // Points of interest, nearest first, so the ones you could actually stop at win the space.
    let here = projector.middle();
    let mut near: Vec<&crate::world::Poi> = world.pois.iter().collect();
    near.sort_by(|a, b| distance(a.at, here).total_cmp(&distance(b.at, here)));
    for poi in near.into_iter().take(6) {
        let (col, row) = projector.cell_of(poi.at);
        labels.place(col, row, poi.name, Role::Info, true);
    }

    // Street names where each street crosses the middle of the pane, so they follow the view.
    let mut streets: Vec<(f32, (f32, f32), &'static str, Class)> = Vec::new();
    for street in &world.down {
        let at = (street.offset + street.slope * here.1, here.1);
        streets.push((distance(at, here), at, street.name, street.class));
    }
    for street in &world.across {
        let at = (here.0, street.offset + street.slope * here.0);
        streets.push((distance(at, here), at, street.name, street.class));
    }
    streets.sort_by(|a, b| b.3.speed().total_cmp(&a.3.speed()).then(a.0.total_cmp(&b.0)));
    for (_, at, name, class) in streets {
        let role = if class == Class::Residential { Role::Muted } else { Role::Text };
        let (col, row) = projector.cell_of(at);
        labels.place(col, row, name, role, false);
    }
}

//! The map in braille, for terminals that cannot show a picture.
//!
//! Eight dots a cell is the most positions a character cell can hold, and a braille dot is very
//! nearly square in a terminal cell, so one scale serves both axes. Areas are cell backgrounds
//! rather than dots: they carry no detail worth eight bits, and blocking them leaves the dots for
//! the lines. It is a good fallback, and running it beside [`crate::scene`] is the clearest
//! statement of what a cell grid costs.

use conui::{Canvas, Color, Rect, Role, Style};

use crate::route::Route;
use crate::skin::Skin;
use crate::view::Projector;
use crate::world::{Class, World, distance};

/// Which dot of a braille cell each (x, y) within it is.
const DOT_BIT: [[u8; 4]; 2] = [[0, 1, 2, 6], [3, 4, 5, 7]];

pub struct Dots {
    cols: u16,
    rows: u16,
    width: f32,
    height: f32,
    bits: Vec<u8>,
    pri: Vec<u8>,
    role: Vec<Role>,
    bg: Vec<Option<Color>>,
    skin: &'static Skin,
}

impl Dots {
    pub fn new(cols: u16, rows: u16, skin: &'static Skin) -> Dots {
        let cells = usize::from(cols) * usize::from(rows);
        Dots {
            cols,
            rows,
            width: f32::from(cols) * 2.0,
            height: f32::from(rows) * 4.0,
            bits: vec![0; cells],
            pri: vec![0; cells],
            role: vec![Role::Dim; cells],
            bg: vec![None; cells],
            skin,
        }
    }

    fn dot(&mut self, x: f32, y: f32, priority: u8, role: Role) {
        if x < 0.0 || y < 0.0 || x >= self.width || y >= self.height {
            return;
        }
        let (dx, dy) = (x as usize, y as usize);
        let index = (dy / 4) * usize::from(self.cols) + dx / 2;
        if priority >= self.pri[index] {
            self.pri[index] = priority;
            self.role[index] = role;
        }
        self.bits[index] |= 1 << DOT_BIT[dx % 2][dy % 4];
    }

    /// One already-projected way, `wide` dots across.
    fn way(&mut self, points: &[(f32, f32)], wide: f32, priority: u8, role: Role) {
        let wide = (wide.round() as i32).clamp(1, 5);
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let length = dx.hypot(dy);
            if length < 0.001 {
                continue;
            }
            // Both ends off the same side means nothing on this segment can be visible.
            if (a.0 < 0.0 && b.0 < 0.0)
                || (a.1 < 0.0 && b.1 < 0.0)
                || (a.0 >= self.width && b.0 >= self.width)
                || (a.1 >= self.height && b.1 >= self.height)
            {
                continue;
            }
            let (nx, ny) = (-dy / length, dx / length);
            let steps = (length * 1.4) as i32 + 1;
            for step in 0..=steps {
                let t = step as f32 / steps as f32;
                for lane in 0..wide {
                    let offset = lane as f32 - (wide - 1) as f32 / 2.0;
                    self.dot(
                        a.0 + dx * t + nx * offset,
                        a.1 + dy * t + ny * offset,
                        priority,
                        role,
                    );
                }
            }
        }
    }

    /// Water, parks and footprints, by asking each cell what it sits on.
    ///
    /// One inverse projection per cell rather than a polygon rasteriser: two thousand cells against
    /// a handful of shapes is nothing, and it stays correct when the view is rotated.
    fn ground(&mut self, world: &World, view: &Projector) {
        let reach = self.width / view.scale();
        let here = view.middle();
        let near: Vec<&(f32, f32, f32, f32)> = world
            .buildings
            .iter()
            .filter(|(x, y, _, _)| distance((*x, *y), here) < reach)
            .collect();

        for row in 0..usize::from(self.rows) {
            for col in 0..usize::from(self.cols) {
                let at = view.unproject((col as f32 * 2.0 + 1.0, row as f32 * 4.0 + 2.0));
                let index = row * usize::from(self.cols) + col;

                if world.river_distance(at) < crate::world::RIVER * 0.5 {
                    self.bg[index] = Some(self.skin.blocks.water);
                    continue;
                }
                let inside = |(x, y, w, h): &&(f32, f32, f32, f32)| {
                    at.0 > *x && at.0 < x + w && at.1 > *y && at.1 < y + h
                };
                if world.parks.iter().any(|park| inside(&park)) {
                    self.bg[index] = Some(self.skin.blocks.park);
                } else if near.iter().any(inside) {
                    self.bg[index] = Some(self.skin.blocks.building);
                }
            }
        }
    }

    pub fn draw(&mut self, world: &World, route: Option<&Route>, view: &Projector) {
        self.ground(world, view);
        let scale = view.scale();

        for (streets, down) in [(&world.down, true), (&world.across, false)] {
            for street in streets.iter() {
                let ends = view.street_ends(world, street, down);
                let line = [view.project(ends[0]), view.project(ends[1])];
                self.way(
                    &line,
                    street.class.width() * scale,
                    priority(street.class),
                    role(street.class),
                );
            }
        }

        let motorway: Vec<(f32, f32)> = world.motorway.iter().map(|&p| view.project(p)).collect();
        self.way(&motorway, Class::Motorway.width() * scale, 4, Role::Warn);

        if let Some(route) = route {
            let line: Vec<(f32, f32)> = route.points.iter().map(|&p| view.project(p)).collect();
            self.way(&line, (18.0 * scale).max(2.0), 5, Role::Accent);
        }
    }

    pub fn blit(&self, canvas: &mut Canvas<'_>) {
        for row in 0..usize::from(self.rows) {
            for col in 0..usize::from(self.cols) {
                let index = row * usize::from(self.cols) + col;
                if let Some(color) = self.bg[index] {
                    canvas.style_area(
                        Rect::new(col as u16, row as u16, 1, 1),
                        Style::EMPTY.bg(color),
                    );
                }
                if self.bits[index] != 0 {
                    let glyph = char::from_u32(0x2800 + u32::from(self.bits[index])).unwrap_or(' ');
                    let style = Style::EMPTY
                        .fg(self.skin.theme.color(self.role[index]))
                        .bg(self.bg[index].unwrap_or(self.skin.theme.background));
                    canvas.put_styled(col as i32, row as i32, &glyph.to_string(), style);
                }
            }
        }
    }

    /// The background a cell ended up with, so text on top of it can match.
    pub fn background(&self, col: u16, row: u16) -> Color {
        let index = usize::from(row) * usize::from(self.cols) + usize::from(col);
        self.bg.get(index).copied().flatten().unwrap_or(self.skin.theme.background)
    }
}

fn priority(class: Class) -> u8 {
    match class {
        Class::Residential => 1,
        Class::Primary => 3,
        Class::Motorway => 4,
    }
}

fn role(class: Class) -> Role {
    match class {
        Class::Residential => Role::Dim,
        Class::Primary => Role::Muted,
        Class::Motorway => Role::Warn,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::Camera;

    /// Backgrounds carry no glyph, so a text dump cannot show them. Ask the grid instead.
    #[test]
    fn the_river_paints_the_cells_it_runs_through() {
        let world = World::generate();
        let camera =
            Camera { at: world.river[14], heading: 0.0, north_up: true, metres_across: 900.0 };
        let view = Projector::new(&camera, 120.0, 80.0, (2.0, 4.0));
        let mut dots = Dots::new(60, 20, &Skin::DARK);
        dots.ground(&world, &view);

        let water = Skin::DARK.blocks.water;
        assert_eq!(dots.background(30, 10), water, "the cell the camera sits on is water");
        let wet = (0..60)
            .flat_map(|col| (0..20).map(move |row| (col, row)))
            .filter(|&(col, row)| dots.background(col, row) == water)
            .count();
        assert!(wet > 40, "a river across the view should wet more than {wet} cells");
    }
}

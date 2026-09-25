//! A pixel surface, for the one panel that is geometry rather than language.
//!
//! Everything here is coverage-based: a shape says which fraction of each pixel it covers and that
//! fraction is blended in. It is the cheapest way to get edges that do not stair-step, and on a map
//! the stair-stepping is what reads as "computer drawing" rather than "map".
//!
//! Drawing happens into a [`Band`] — a horizontal strip of the surface — rather than into the
//! surface itself, so a frame can be drawn on every core at once. Every operation here is a per-pixel
//! composite, and the order of the composites a given pixel sees is the same whether the pane is one
//! band or sixteen, so the split changes the time and not a single byte of the result. A band also
//! culls against its own rows, which is why sixteen bands cost rather less than a sixteenth each.

/// Sub-scanlines per pixel row for shapes whose edges matter.
const SHARP: u32 = 4;
/// Sub-scanlines for large flat areas, where an extra sample buys nothing.
const FLAT: u32 = 1;

/// Fewest rows worth giving a thread. Below this the spawn costs more than the work.
const LEAST: u32 = 24;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb`, so a palette can be written the way it is written everywhere else.
    pub const fn hex(text: &str) -> Rgb {
        let bytes = text.as_bytes();
        Rgb(byte(bytes, 1), byte(bytes, 3), byte(bytes, 5))
    }

    pub(crate) fn mix(self, other: Rgb, amount: f32) -> Rgb {
        let blend = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * amount) as u8;
        Rgb(blend(self.0, other.0), blend(self.1, other.1), blend(self.2, other.2))
    }
}

const fn byte(bytes: &[u8], at: usize) -> u8 {
    nibble(bytes[at]) * 16 + nibble(bytes[at + 1])
}

const fn nibble(character: u8) -> u8 {
    match character {
        b'0'..=b'9' => character - b'0',
        b'a'..=b'f' => character - b'a' + 10,
        b'A'..=b'F' => character - b'A' + 10,
        _ => 0,
    }
}

pub struct Surface {
    pub width: u32,
    pub height: u32,
    /// Three bytes a pixel, top row first — exactly what `f=24` wants, so there is no repacking.
    pub pixels: Vec<u8>,
}

impl Surface {
    pub fn new(width: u32, height: u32, ground: Rgb) -> Surface {
        let mut surface = Surface { width, height, pixels: vec![0; (width * height * 3) as usize] };
        for pixel in surface.pixels.chunks_exact_mut(3) {
            pixel.copy_from_slice(&[ground.0, ground.1, ground.2]);
        }
        surface
    }

    /// How many bands this surface is worth splitting into, given how many cores there are.
    pub fn bands(&self, cores: u32) -> u32 {
        cores.clamp(1, (self.height / LEAST).max(1))
    }

    /// Draw the whole surface, `bands` strips at a time, one thread each.
    ///
    /// `draw` is called once per band with the full scene to draw; it does not need to know it is
    /// one of several, because each band clips and culls to its own rows.
    pub fn paint<F>(&mut self, bands: u32, draw: F)
    where
        F: Fn(&mut Band<'_>) + Sync,
    {
        let mut strips = self.split(bands);
        // One band is the common case for a still, and spawning a thread to wait for it is silly.
        if let [only] = strips.as_mut_slice() {
            draw(only);
            return;
        }
        std::thread::scope(|scope| {
            for strip in &mut strips {
                let draw = &draw;
                scope.spawn(move || draw(strip));
            }
        });
    }

    fn split(&mut self, bands: u32) -> Vec<Band<'_>> {
        let (width, height) = (self.width, self.height);
        let bands = bands.clamp(1, height.max(1));
        let rows = height.div_ceil(bands);
        let mut strips = Vec::with_capacity(bands as usize);
        let mut rest = self.pixels.as_mut_slice();
        let mut top = 0;
        while top < height {
            let bottom = (top + rows).min(height);
            let (mine, tail) = rest.split_at_mut(((bottom - top) * width * 3) as usize);
            rest = tail;
            strips.push(Band {
                width,
                top,
                bottom,
                pixels: mine,
                coverage: vec![0.0; width as usize + 2],
            });
            top = bottom;
        }
        strips
    }
}

/// A horizontal strip of a [`Surface`], and everything that can be drawn into one.
pub struct Band<'a> {
    width: u32,
    /// First and last-plus-one row of the surface this band owns.
    top: u32,
    bottom: u32,
    pixels: &'a mut [u8],
    /// One row of per-pixel coverage, reused by every fill so no shape allocates.
    coverage: Vec<f32>,
}

impl Band<'_> {
    pub fn fill_all(&mut self, color: Rgb) {
        for pixel in self.pixels.chunks_exact_mut(3) {
            pixel.copy_from_slice(&[color.0, color.1, color.2]);
        }
    }

    fn blend(&mut self, x: u32, y: u32, color: Rgb, alpha: f32) {
        let at = (((y - self.top) * self.width + x) * 3) as usize;
        let blend = |under: u8, over: u8| {
            (f32::from(under) + (f32::from(over) - f32::from(under)) * alpha) as u8
        };
        self.pixels[at] = blend(self.pixels[at], color.0);
        self.pixels[at + 1] = blend(self.pixels[at + 1], color.1);
        self.pixels[at + 2] = blend(self.pixels[at + 2], color.2);
    }

    /// A polygon, sampled `sub` times per row. Convex shapes come out exact; a concave one is
    /// filled to its span, which is all this renderer ever asks of it.
    fn fill(&mut self, points: &[(f32, f32)], color: Rgb, alpha: f32, sub: u32) {
        if points.len() < 3 || alpha <= 0.0 {
            return;
        }
        let top = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
        let bottom = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
        let first = (top.floor().max(0.0) as u32).max(self.top);
        let last = (bottom.ceil() as i64).clamp(0, i64::from(self.bottom)) as u32;
        let weight = 1.0 / sub as f32;

        for y in first..last {
            let (mut lo, mut hi) = (self.width, 0u32);
            for step in 0..sub {
                let at = y as f32 + (step as f32 + 0.5) * weight;
                let Some((left, right)) = span(points, at) else { continue };
                let left = left.max(0.0);
                let right = right.min(self.width as f32);
                if right <= left {
                    continue;
                }
                let from = left.floor() as u32;
                let to = (right.ceil() as u32).min(self.width);
                for x in from..to {
                    let covered = (right.min(x as f32 + 1.0) - left.max(x as f32)).clamp(0.0, 1.0);
                    self.coverage[x as usize] += covered * weight;
                }
                lo = lo.min(from);
                hi = hi.max(to);
            }
            for x in lo..hi {
                let covered = self.coverage[x as usize];
                self.coverage[x as usize] = 0.0;
                if covered > 0.002 {
                    self.blend(x, y, color, (covered * alpha).min(1.0));
                }
            }
        }
    }

    pub fn area(&mut self, points: &[(f32, f32)], color: Rgb) {
        self.fill(points, color, 1.0, FLAT);
    }

    pub fn shape(&mut self, points: &[(f32, f32)], color: Rgb) {
        self.fill(points, color, 1.0, SHARP);
    }

    pub fn disc(&mut self, at: (f32, f32), radius: f32, color: Rgb) {
        let sides = ((radius * 3.0) as usize).clamp(8, 28);
        let points: Vec<(f32, f32)> = (0..sides)
            .map(|step| {
                let angle = step as f32 / sides as f32 * std::f32::consts::TAU;
                (at.0 + angle.cos() * radius, at.1 + angle.sin() * radius)
            })
            .collect();
        self.fill(&points, color, 1.0, SHARP);
    }

    /// A polyline of a given width, with round joins and caps — a chain of capsules.
    pub fn stroke(&mut self, points: &[(f32, f32)], width: f32, color: Rgb) {
        let half = (width * 0.5).max(0.4);
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let length = dx.hypot(dy);
            if length < 0.0001 || !self.touches(a, b, half) {
                continue;
            }
            let (nx, ny) = (-dy / length * half, dx / length * half);
            let quad = [
                (a.0 + nx, a.1 + ny),
                (b.0 + nx, b.1 + ny),
                (b.0 - nx, b.1 - ny),
                (a.0 - nx, a.1 - ny),
            ];
            self.fill(&quad, color, 1.0, SHARP);
        }
        // Round off every vertex, which covers both the joins and the two ends in one pass.
        if half > 0.9 {
            for &point in points {
                if self.touches(point, point, half) {
                    self.disc(point, half, color);
                }
            }
        }
    }

    /// A road: a dark casing under a lighter fill, which is what makes crossings read as crossings.
    pub fn road(&mut self, points: &[(f32, f32)], width: f32, fill: Rgb, casing: Rgb) {
        self.stroke(points, width + (width * 0.35).clamp(1.5, 4.0), casing);
        self.stroke(points, width, fill);
    }

    /// Dashes along a polyline, for a motorway's centre line.
    pub fn dashes(&mut self, points: &[(f32, f32)], width: f32, color: Rgb, on: f32, off: f32) {
        let mut carry = 0.0;
        let mut drawing = true;
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let length = (b.0 - a.0).hypot(b.1 - a.1);
            if length < 0.0001 {
                continue;
            }
            let mut walked = 0.0;
            while walked < length {
                let want = if drawing { on } else { off } - carry;
                let step = want.min(length - walked);
                if drawing {
                    let at = |d: f32| {
                        let t = d / length;
                        (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
                    };
                    self.stroke(&[at(walked), at(walked + step)], width, color);
                }
                walked += step;
                carry += step;
                if carry >= if drawing { on } else { off } - 0.0001 {
                    carry = 0.0;
                    drawing = !drawing;
                }
            }
        }
    }

    /// A translucent shape whose edges matter — a panel the map shows through.
    pub fn panel(&mut self, points: &[(f32, f32)], color: Rgb, alpha: f32) {
        self.fill(points, color, alpha, SHARP);
    }

    /// A translucent plate, for putting under text that the terminal will draw on top.
    ///
    /// The image sits above the cell backgrounds, so a label cannot get its contrast from a cell
    /// background the way it does in the braille version. It has to come from the picture.
    pub fn wash(&mut self, points: &[(f32, f32)], color: Rgb, alpha: f32) {
        self.fill(points, color, alpha, FLAT);
    }

    /// A soft edge down the side that meets the rail, so the map does not end in a hard line.
    pub fn fade_right(&mut self, columns: u32, ground: Rgb) {
        for x in self.width.saturating_sub(columns)..self.width {
            let amount = (x - (self.width - columns)) as f32 / columns as f32;
            for y in self.top..self.bottom {
                let at = (((y - self.top) * self.width + x) * 3) as usize;
                let under = Rgb(self.pixels[at], self.pixels[at + 1], self.pixels[at + 2]);
                let mixed = under.mix(ground, amount * amount);
                self.pixels[at] = mixed.0;
                self.pixels[at + 1] = mixed.1;
                self.pixels[at + 2] = mixed.2;
            }
        }
    }

    /// Whether a capsule could put ink on this band at all.
    ///
    /// Most of a city is off-screen, and most of what is on screen misses any given band — which is
    /// where the other half of the speedup comes from.
    fn touches(&self, a: (f32, f32), b: (f32, f32), half: f32) -> bool {
        let pad = half + 1.0;
        a.0.min(b.0) < self.width as f32 + pad
            && a.0.max(b.0) > -pad
            && a.1.min(b.1) < self.bottom as f32 + pad
            && a.1.max(b.1) > self.top as f32 - pad
    }
}

/// Where a horizontal line at `y` enters and leaves a polygon.
fn span(points: &[(f32, f32)], y: f32) -> Option<(f32, f32)> {
    let (mut left, mut right) = (f32::INFINITY, f32::NEG_INFINITY);
    for index in 0..points.len() {
        let (a, b) = (points[index], points[(index + 1) % points.len()]);
        if (a.1 <= y) == (b.1 <= y) {
            continue;
        }
        let x = a.0 + (y - a.1) / (b.1 - a.1) * (b.0 - a.0);
        left = left.min(x);
        right = right.max(x);
    }
    (left <= right).then_some((left, right))
}

#[cfg(test)]
mod tests {
    use super::{Band, Rgb, Surface};

    /// Shapes that straddle every plausible band edge, drawn in a fixed order.
    fn scene(band: &mut Band<'_>) {
        band.fill_all(Rgb::hex("#101418"));
        band.stroke(&[(-20.0, 5.0), (150.0, 190.0)], 9.0, Rgb::hex("#8ea2a7"));
        band.stroke(&[(10.0, 180.0), (90.0, 20.0), (170.0, 170.0)], 4.0, Rgb::hex("#3c555f"));
        band.area(&[(20.0, 20.0), (80.0, 24.0), (76.0, 96.0), (16.0, 90.0)], Rgb::hex("#1a3a2f"));
        band.disc((100.0, 100.0), 37.0, Rgb::hex("#62f5b5"));
        band.wash(&[(0.0, 48.0), (200.0, 48.0), (200.0, 66.0), (0.0, 66.0)], Rgb::RED, 0.5);
        band.dashes(&[(0.0, 120.0), (200.0, 128.0)], 3.0, Rgb::hex("#f0c987"), 11.0, 7.0);
        band.fade_right(12, Rgb::hex("#101418"));
    }

    impl Rgb {
        const RED: Rgb = Rgb(255, 0, 0);
    }

    #[test]
    fn splitting_the_work_across_threads_changes_the_time_and_not_the_picture() {
        let ground = Rgb::hex("#000000");
        let mut once = Surface::new(200, 200, ground);
        once.paint(1, scene);

        // Including counts that do not divide 200, so the last band is a different height.
        for bands in [2, 3, 4, 7, 8, 16, 200] {
            let mut split = Surface::new(200, 200, ground);
            split.paint(bands, scene);
            let differing = once.pixels.iter().zip(&split.pixels).filter(|(a, b)| a != b).count();
            assert_eq!(differing, 0, "{bands} bands drew a different picture");
        }
    }

    #[test]
    fn a_short_surface_is_not_split_into_slivers() {
        let surface = Surface::new(100, 40, Rgb::hex("#000000"));
        assert_eq!(surface.bands(12), 1);
        assert_eq!(Surface::new(100, 400, Rgb::hex("#000000")).bands(12), 12);
    }
}

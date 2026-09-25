//! The two blocks a driver is allowed to need: what to do next, and how fast they are going.
//!
//! Everything here follows from how far away the screen is. At fifty centimetres a terminal cell on a
//! ten-inch panel is about three millimetres tall, which subtends roughly twenty arcminutes — the
//! floor for legibility, and too small to glance at. Block digits are three rows, so about seventy,
//! which is what the arrival clock in the rail is drawn at and comfortably more than a glance needs.
//! Shapes are the different case: an arrow is *recognised* rather than read, and recognition wants
//! room, so the pictogram is drawn in the picture at five rows and more.
//!
//! So the split inside these two blocks is geometry in pixels, numbers and words in cells. The
//! terminal draws digits at its own resolution, which is sharper than anything this program could
//! paint, and one number treatment across the whole dash means a distance, a speed and an arrival
//! time all read as the same kind of fact — which is most of what makes a readout learnable.
//!
//! Three rules are doing as much work as the size:
//!
//! - **Nothing moves.** Each block is the same rectangle in the same corner at every moment of every
//!   drive. The arrow does not shift when lanes appear and the figure does not slide when it gains a
//!   digit. A driver who has to find a number has already spent the glance.
//! - **Nothing speaks until it matters.** The lane arrows appear inside [`Card::CLOSE`] and the
//!   countdown turns from calm to pressing inside [`Card::URGENT`]. Permanent furniture teaches the
//!   eye to skip it, so the only way for "now" to mean anything is for it to be absent beforehand.
//! - **The lanes are a road, not a legend.** The bank under the arrow is the carriageway seen from
//!   above: solid lines at its kerbs, dashed ones between the lanes, one small arrow per lane. They
//!   were briefly merged into the big arrow as a single shape, which sounded better than it looked —
//!   the merged tail read as a taper on the arrow rather than as a lane. Two elements, one job each.
//!
//! The rows are named here rather than in either renderer, because both draw into them: the picture
//! puts the arrow, the lanes and the countdown in, the cell grid writes the figures and the words on
//! top, and the braille fallback does the lot in cells. Same rectangles, same rows, whichever is
//! running.

use conui::{Canvas, Color, Rect, Role, Style};

use crate::paint::{Band, Rgb};
use crate::route::Turn;
use crate::skin::Palette;

/// Cells the card occupies. Fixed, because a critical readout that resizes is one you have to find.
pub const WIDTH: u16 = 40;
pub const HEIGHT: u16 = 14;

/// The pictogram — the manoeuvre, by itself — as column, row, width, height.
pub const SIGN: (u16, u16, u16, u16) = (1, 1, 12, 4);
/// The lane bank, under it and in the same columns, so it reads as the road the manoeuvre is taken
/// from without being drawn as part of the arrow.
///
/// A row of clear space between the two, which is not decoration: the pictogram's tail ends in a round
/// cap and the lane markings start in one, and a cap's width of space between them is the difference
/// between two shapes and one shape with a kink in it.
pub const LANES: (u16, u16, u16, u16) = (1, 6, 12, 3);
/// The distance, as block digits: column, row, and the three rows a block digit is.
pub const FIGURE: (u16, u16, u16) = (14, 1, 3);

/// Rows inside the card, all fixed for the same reason as its size.
pub const WORD: u16 = 5;
pub const ONTO: u16 = 9;
pub const COUNTDOWN: u16 = 10;
pub const THEN: u16 = 12;

/// The speedometer's own block, in cells: the digits, and a column of small print beside them.
pub const GAUGE: (u16, u16) = (25, 5);

/// Inside the gauge: the column the speed's digits end at, and the column the small print starts at.
///
/// The speed is right-aligned against a fixed column so that it does not shuffle sideways between 99
/// and 100. Everything else is a left-aligned stack to its right, because those three lines are read
/// as a list rather than as numbers — and because a limit drawn as a ring around a number is either
/// bigger than what it holds or too tight to hold three digits.
const SPEED_RIGHT: u16 = 12;
const ASIDE: u16 = 14;

pub struct Card {
    /// Where the card sits, in cells of the map pane.
    pub area: Rect,
    pub turn: Turn,
    /// Metres still to run before the manoeuvre.
    pub away: f32,
    pub lanes: Vec<bool>,
}

impl Card {
    /// Where the lane arrows appear. Roughly ten seconds at town speed: long enough to change lane,
    /// short enough that their appearance is news.
    pub const CLOSE: f32 = 250.0;
    /// Where the countdown stops being informative and starts being an instruction.
    pub const URGENT: f32 = 150.0;
    /// The countdown's full length, in metres.
    ///
    /// Fixed, rather than the length of the leg being driven: a half-empty bar then always means the
    /// same distance, which is the only way a bar ever teaches anybody anything.
    pub const WINDOW: f32 = 500.0;

    pub fn urgent(&self) -> bool {
        self.away < Self::URGENT
    }

    pub fn lanes_shown(&self) -> bool {
        self.away < Self::CLOSE && self.lanes.len() > 1
    }

    /// How much of the countdown is still to run, `1.0` at a distance and `0.0` at the turn.
    pub fn left(&self) -> f32 {
        (self.away / Self::WINDOW).clamp(0.0, 1.0)
    }
}

/// The speedometer: the number the driver is answerable for, and the number they are answerable to.
pub struct Gauge {
    pub area: Rect,
    pub kmh: u32,
    pub limit: u32,
}

/// What the speed is doing about the limit, which is the only reason a speedometer has a colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Watch {
    /// Under it, with room to spare. No colour, because nothing is happening.
    Clear,
    /// Within a couple of per cent of it — the band in which a driver is about to break it without
    /// having decided to. Amber is a warning rather than a fault, and this is the only part of the
    /// scale where a dash can be useful instead of merely accurate.
    Close,
    /// Over it. Red at one over rather than at four: a readout that quietly allows the first three
    /// is a readout that has taken a view on the law, which is not a speedometer's job.
    Over,
}

impl Gauge {
    /// How near the limit is worth a colour: within two per cent of it.
    const CLOSE: f32 = 0.98;

    pub fn watch(&self) -> Watch {
        if self.kmh > self.limit {
            Watch::Over
        } else if f32::from(self.kmh as u16) >= f32::from(self.limit as u16) * Self::CLOSE {
            Watch::Close
        } else {
            Watch::Clear
        }
    }

    /// The role the number and its limit are drawn in. Named here so the plate, the rail's fallback
    /// and the braille build cannot disagree about what amber means.
    pub fn role(&self) -> Role {
        match self.watch() {
            Watch::Clear => Role::Text,
            Watch::Close => Role::Warn,
            Watch::Over => Role::Danger,
        }
    }

    /// By how much, when it is worth saying at all.
    ///
    /// The number a driver can act on is the difference, not the two figures it is between: "twelve
    /// over" is an amount to come off, and a red 62 beside a 50 is a subtraction to do at the wheel.
    /// `None` is not "under" — it is "nothing to say", which is why nothing is said.
    ///
    /// The colour arrives at one over and this line at four, which is not a disagreement: a number
    /// turning red is the fact, and an amount to come off is only worth a row of its own once it is
    /// more than the speedometer's own error.
    pub fn over_by(&self) -> Option<u32> {
        (self.kmh > self.limit + 3).then(|| self.kmh - self.limit)
    }

    /// The same reading with nowhere to put it, for the rail's fallback: it draws the numbers itself,
    /// at its own columns, so the only part of a gauge it needs is what the gauge says.
    pub fn reading(kmh: u32, limit: u32) -> Gauge {
        Gauge { area: Rect::new(0, 0, 0, 0), kmh, limit }
    }
}

/// A distance as a number and its unit, apart.
///
/// The number is block digits and the unit is small text beside them, so the two cannot travel as one
/// string.
pub fn figure(metres: f32) -> (String, &'static str) {
    if metres < 950.0 {
        ((((metres / 10.0).round() as u32) * 10).to_string(), "m")
    } else {
        (format!("{:.1}", metres / 1000.0), "km")
    }
}

/// The card's plate, pictogram, lanes and countdown, in pixels. The numbers and words go on in cells.
pub fn draw(band: &mut Band<'_>, card: &Card, cell: (f32, f32), map: &Palette) {
    let at = grid(card.area, cell);
    let (w, h) = span(card.area, cell);
    let colour = if card.urgent() { map.press } else { map.calm };
    plate(band, at(0.0, 0.0), (w, h), cell, colour, map);

    let sign = at(f32::from(SIGN.0), f32::from(SIGN.1));
    match card.turn {
        // Arriving is not a manoeuvre out of a lane, so there is no bank and the pin takes the rows
        // the bank would have had: a pin with a lane stalk under its point reads as a keyhole.
        Turn::Arrive => {
            let deep = LANES.1 + LANES.3 - SIGN.1;
            arrow(band, card.turn, sign, span_of(SIGN.2, deep, cell), colour, map);
        }
        _ => {
            arrow(band, card.turn, sign, span_of(SIGN.2, SIGN.3, cell), colour, map);
            let bank = at(f32::from(LANES.0), f32::from(LANES.1));
            lanes(band, card, bank, span_of(LANES.2, LANES.3, cell), colour, map);
        }
    }

    let bar = (w - 4.0 * cell.0, cell.1 * 0.42);
    countdown(band, card.left(), at(2.0, f32::from(COUNTDOWN) + 0.4), bar, colour, map);
}

/// The card's numbers and words, in cells, over whatever drew the card itself.
///
/// `plate` is the braille fallback: with no picture underneath, the card's own furniture has to be
/// cells too. The rows are the same either way, which is why they are named in this module.
pub fn cells(
    canvas: &mut Canvas<'_>,
    card: &Card,
    onto: &str,
    then: Option<(Turn, &str)>,
    plate: Option<Color>,
) {
    let (x, y) = (i32::from(card.area.x), i32::from(card.area.y));
    // Brighter, not amber: `Warn` is the colour the traffic speaks in, and a card wearing it says
    // "there is a jam" to anybody who only glanced. See `Palette::press`.
    let edge = if card.urgent() { Role::Text } else { Role::Accent };
    let (number, unit) = figure(card.away);

    if let Some(colour) = plate {
        canvas.fill(card.area, ' ', Role::Surface);
        canvas.style_area(card.area, Style::EMPTY.bg(colour));
        canvas.border_rounded(card.area, edge);
        // No picture, so the pictogram is one glyph and the bank is a row of them.
        canvas.put(x + 3, y + 2, card.turn.arrow(), edge);
        canvas.bar(x + 2, y + i32::from(COUNTDOWN), card.left(), WIDTH - 4, edge);
        if card.lanes_shown() {
            for (lane, &ours) in card.lanes.iter().enumerate() {
                let (glyph, role) =
                    if ours { (card.turn.arrow(), edge) } else { ("↑", Role::Dim) };
                canvas.put(x + 3 + lane as i32 * 2, y + i32::from(LANES.1) + 1, glyph, role);
            }
        }
    }

    // The distance, in the same three-row block digits the arrival clock in the rail uses, and in
    // cells in both renderers: a number is read rather than recognised, three rows is already well
    // over what reading needs at this distance, and one number treatment for the whole dash means a
    // distance and an arrival time look like the same kind of fact.
    canvas.digits(x + i32::from(FIGURE.0), y + i32::from(FIGURE.1), &number, edge);
    // The unit beside the figure rather than under it, because it is the one part of a distance that
    // never changes and so is the one part worth making small.
    let after = x + i32::from(FIGURE.0) + i32::from(Canvas::digits_size(&number).0) + 1;
    canvas.put(after, y + i32::from(FIGURE.1) + i32::from(FIGURE.2) - 1, unit, Role::Muted);

    // The manoeuvre in words as well as in the arrow. Redundant on purpose: a direction is the one
    // thing on this card that is expensive to get wrong, so it is said twice, two different ways.
    canvas.put(x + i32::from(FIGURE.0), y + i32::from(WORD), card.turn.word(), Role::Dim);

    // Which street, and what follows it — read once on the approach and then never again, so cells.
    canvas.put_truncated(x + 2, y + i32::from(ONTO), onto, WIDTH - 4, Role::Text);
    if let Some((turn, street)) = then {
        canvas.put(x + 2, y + i32::from(THEN), "then", Role::Dim);
        canvas.put(x + 7, y + i32::from(THEN), turn.arrow(), Role::Info);
        canvas.put_truncated(x + 9, y + i32::from(THEN), street, WIDTH - 11, Role::Muted);
    }
}

/// The speedometer, in pixels — which is the plate and nothing else. Both its numbers are cells.
pub fn gauge(band: &mut Band<'_>, gauge: &Gauge, cell: (f32, f32), map: &Palette) {
    let at = grid(gauge.area, cell);
    let (w, h) = span(gauge.area, cell);
    // The plate's edge takes the same three states as the number on it. Amber and red belong to the
    // traffic everywhere else on this dash, and this is the one place they are borrowed: a limit is
    // the other thing on a road that goes green, amber, red, and a driver already reads it that way.
    let edge = match gauge.watch() {
        Watch::Clear => map.calm,
        Watch::Close => map.busy,
        Watch::Over => map.stopped,
    };
    plate(band, at(0.0, 0.0), (w, h), cell, edge, map);
}

/// The speedometer's numbers and words, in cells. `plate` is the braille fallback, as on the card.
pub fn gauge_cells(canvas: &mut Canvas<'_>, gauge: &Gauge, plate: Option<Color>) {
    let (x, y) = (i32::from(gauge.area.x), i32::from(gauge.area.y));
    let role = gauge.role();

    if let Some(colour) = plate {
        canvas.fill(gauge.area, ' ', Role::Surface);
        canvas.style_area(gauge.area, Style::EMPTY.bg(colour));
        canvas.border_rounded(gauge.area, role);
    }

    let text = gauge.kmh.to_string();
    let wide = i32::from(Canvas::digits_size(&text).0);
    canvas.digits(x + i32::from(SPEED_RIGHT) - wide, y + 1, &text, role);
    aside(canvas, gauge, x + i32::from(ASIDE), y + 1);
}

/// The three lines beside the speed, in three rows that never move.
///
/// The unit in the middle, level with the figure's waist, so it belongs to the number rather than to
/// the block; the limit below it, quiet, because it is checked rather than watched; and the top row
/// empty until the limit is actually broken. Additive, like the card's lanes: a row that is blank most
/// of the time is the only kind of row whose filling up means anything.
pub fn aside(canvas: &mut Canvas<'_>, gauge: &Gauge, x: i32, y: i32) {
    if let Some(over) = gauge.over_by() {
        canvas.put(x, y, &format!("▲ {over} over"), Role::Danger);
    }
    canvas.put(x, y + 1, "km/h", Role::Muted);
    // The limit joins in: it is the thing being exceeded, and a red number beside a grey limit reads
    // as one of the two being broken without saying which.
    let role = if gauge.watch() == Watch::Clear { Role::Dim } else { gauge.role() };
    canvas.put(x, y + 2, &format!("LIMIT {}", gauge.limit), role);
}

// ---- The pieces -----------------------------------------------------------------------------

/// A cell coordinate inside `area` as a pixel one.
fn grid(area: Rect, cell: (f32, f32)) -> impl Fn(f32, f32) -> (f32, f32) {
    let (cw, ch) = cell;
    move |col, row| (f32::from(area.x) * cw + col * cw, f32::from(area.y) * ch + row * ch)
}

fn span(area: Rect, cell: (f32, f32)) -> (f32, f32) {
    (f32::from(area.width) * cell.0, f32::from(area.height) * cell.1)
}

fn span_of(cols: u16, rows: u16, cell: (f32, f32)) -> (f32, f32) {
    (f32::from(cols) * cell.0, f32::from(rows) * cell.1)
}

/// A bevelled plate the map shows through, with an outline and a coloured edge down one side.
///
/// A bevel rather than a rounded rectangle: at this size the eye reads them the same, and one
/// polygon means one pass of alpha, so the corners do not come out darker than the middle.
fn plate(
    band: &mut Band<'_>,
    at: (f32, f32),
    size: (f32, f32),
    cell: (f32, f32),
    edge: Rgb,
    map: &Palette,
) {
    let (x, y) = at;
    let (w, h) = size;
    let (cw, ch) = cell;
    let bevel = cw.min(ch) * 1.1;
    let shape = [
        (x + bevel, y),
        (x + w - bevel, y),
        (x + w, y + bevel),
        (x + w, y + h - bevel),
        (x + w - bevel, y + h),
        (x + bevel, y + h),
        (x, y + h - bevel),
        (x, y + bevel),
    ];
    band.panel(&shape, map.plate, map.plate_alpha);
    // Then an outline, because the fill has to stay close enough to the map's ground for the map to
    // read through it, and anything that close to the ground has no edge of its own.
    let mut outline = shape.to_vec();
    outline.push(shape[0]);
    band.stroke(&outline, (cw * 0.16).max(1.6), map.plate_edge);
    // A solid edge down the side the eye lands on first, in the colour that says how urgent this is.
    // It is the cheapest possible "this block is the important one".
    let thick = (cw * 0.34).max(3.0);
    band.area(
        &[(x, y + bevel), (x + thick, y + bevel), (x + thick, y + h - bevel), (x, y + h - bevel)],
        edge,
    );
}

/// The lane bank: the carriageway the manoeuvre is taken from, seen from above.
///
/// The markings are what make it a road rather than a row of bars — solid at the kerbs, dashed between
/// the lanes — and they are drawn at every distance, so the bank is furniture. What appears inside
/// [`Card::CLOSE`] is the guidance: one small arrow per lane, ours in the card's colour and the rest
/// dim and straight on. That is a picture a driver can match against the windscreen, rather than a
/// legend they have to decode and then relate to the pictogram six rows above it.
fn lanes(
    band: &mut Band<'_>,
    card: &Card,
    origin: (f32, f32),
    size: (f32, f32),
    colour: Rgb,
    map: &Palette,
) {
    let (w, h) = size;
    let count = card.lanes.len().max(1);
    // A lane is narrower than it is deep, which is what one looks like from the driver's seat. Capped
    // and centred, or a one-lane street comes out as a single room the width of the card.
    let each = (w / count as f32).min(h * 0.72);
    let wide = each * count as f32;
    let (left, right) = (origin.0 + (w - wide) * 0.5, origin.0 + (w + wide) * 0.5);
    let (top, bottom) = (origin.1, origin.1 + h);
    let thin = (each * 0.09).max(1.3);
    // The markings are the unused lane's own colour taken half way back to the plate, rather than a
    // colour of their own: whichever skin is running, paint on the road then has less presence than an
    // arrow on the road, which is the order they have to be read in.
    let paint = map.track.mix(map.plate, 0.55);

    band.stroke(&[(left, top), (left, bottom)], thin, paint);
    band.stroke(&[(right, top), (right, bottom)], thin, paint);
    for index in 1..count {
        let between = left + each * index as f32;
        let marks = [(between, top), (between, bottom)];
        band.dashes(&marks, thin, paint, h * 0.20, h * 0.14);
    }

    if !card.lanes_shown() {
        return;
    }
    // Inset, so each arrow is inside its own lane rather than across the marking beside it.
    let pad = each * 0.14;
    for (index, &ours) in card.lanes.iter().enumerate() {
        let way = if ours { card.turn } else { Turn::Straight };
        let ink = if ours { colour } else { map.track };
        let at = (left + each * index as f32 + pad, top + pad * 0.5);
        marking(band, way, at, (each - pad * 2.0, h - pad), ink);
    }
}

/// One lane's arrow, drawn the way it is painted on the road rather than the way a sign draws it.
///
/// [`arrow`] is built for a box wider than it is tall, and a lane is the other way round: put a turn
/// arrow in a lane-shaped box and the elbow lands halfway up, which leaves the shaft stubby and the
/// whole mark reading as *smaller* than the straight ones beside it rather than as a different
/// direction. So a lane's shaft runs the full depth of the lane whichever way it goes, and the bend is
/// near the top — which is exactly what is under the tyres.
fn marking(band: &mut Band<'_>, turn: Turn, origin: (f32, f32), size: (f32, f32), ink: Rgb) {
    let (w, h) = size;
    let point = |fx: f32, fy: f32| (origin.0 + fx * w, origin.1 + fy * h);
    let thick = (w * 0.30).min(h * 0.16);

    match turn {
        Turn::Left | Turn::Right => {
            let flip = |fx: f32| if turn == Turn::Left { 1.0 - fx } else { fx };
            let side = |fx: f32, fy: f32| point(flip(fx), fy);
            // The shaft is offset, not centred: a turn arrow painted on asphalt leaves the room for
            // its own head on the side it points at, which is the only way the head gets to be a
            // head in a box this narrow.
            band.stroke(&[side(0.34, 0.97), side(0.34, 0.34), side(0.58, 0.34)], thick, ink);
            band.shape(&[side(0.50, 0.02), side(1.0, 0.34), side(0.50, 0.66)], ink);
        }
        _ => {
            band.stroke(&[point(0.5, 0.97), point(0.5, 0.28)], thick, ink);
            band.shape(&[point(0.14, 0.30), point(0.5, 0.02), point(0.86, 0.30)], ink);
        }
    }
}

/// The manoeuvre itself, as a shape rather than a word.
///
/// Drawn as a thick stroked polyline plus a head: the stroke gives a properly rounded elbow for
/// free, which is what makes a turn arrow read as a turn rather than as two rectangles. Every
/// manoeuvre's tail is at the bottom centre of the box, so a left and a right are the same weight on
/// screen — which they have to be, because a driver tells them apart by direction and not by how much
/// ink each one uses. Everything is a fraction of the box it is given, which is what lets the same
/// shape be the card's pictogram and one lane's arrow in the bank underneath it.
fn arrow(
    band: &mut Band<'_>,
    turn: Turn,
    origin: (f32, f32),
    size: (f32, f32),
    colour: Rgb,
    map: &Palette,
) {
    let (w, h) = size;
    let point = |fx: f32, fy: f32| (origin.0 + fx * w, origin.1 + fy * h);
    let thick = w.min(h) * 0.24;

    match turn {
        Turn::Arrive => {
            // A pin, not a ring. A ring on a road sign means a roundabout, and a driver who has to
            // work out which of the two a circle meant has been given a puzzle instead of an answer.
            let head = point(0.5, 0.36);
            let radius = w.min(h) * 0.30;
            band.shape(
                &[
                    (head.0 - radius * 0.86, head.1 + radius * 0.5),
                    point(0.5, 0.99),
                    (head.0 + radius * 0.86, head.1 + radius * 0.5),
                ],
                colour,
            );
            band.disc(head, radius, colour);
            band.disc(head, radius * 0.40, map.plate);
        }
        Turn::Straight => {
            band.stroke(&[point(0.5, 0.98), point(0.5, 0.30)], thick, colour);
            band.shape(&[point(0.14, 0.36), point(0.5, 0.02), point(0.86, 0.36)], colour);
        }
        Turn::Left | Turn::Right => {
            let flip = |fx: f32| if turn == Turn::Left { 1.0 - fx } else { fx };
            let side = |fx: f32, fy: f32| point(flip(fx), fy);
            band.stroke(&[side(0.5, 0.98), side(0.5, 0.42), side(0.70, 0.42)], thick, colour);
            band.shape(&[side(0.62, 0.08), side(0.98, 0.42), side(0.62, 0.76)], colour);
        }
    }
}

/// How much of the approach is left, as a length rather than a number.
///
/// The figure above it is the precise answer; this is the one that can be taken in without reading,
/// which is the only kind of answer a moving car has time for. It drains rather than filling: what is
/// left is what the driver has, and a bar that empties says "running out" without anyone being told.
fn countdown(
    band: &mut Band<'_>,
    left: f32,
    origin: (f32, f32),
    size: (f32, f32),
    colour: Rgb,
    map: &Palette,
) {
    let (w, h) = size;
    let middle = origin.1 + h * 0.5;
    let (from, to) = (origin.0 + h * 0.5, origin.0 + w - h * 0.5);
    band.stroke(&[(from, middle), (to, middle)], h, map.track);
    if left > 0.01 {
        band.stroke(&[(from, middle), (from + (to - from) * left, middle)], h, colour);
    }
}

#[cfg(test)]
mod tests {
    use conui::text_width;

    use super::*;

    /// The card is furniture: it has to be in the same place, with the same rows, at every distance
    /// and in both renderers, or a glance has to become a search.
    #[test]
    fn the_rows_are_fixed_and_the_lane_arrows_are_the_only_thing_that_appears() {
        let card = |away: f32, lanes: usize| Card {
            area: Rect::new(1, 0, WIDTH, HEIGHT),
            turn: Turn::Left,
            away,
            lanes: vec![true; lanes],
        };

        // The layout is all constants, so its own consistency is a thing the compiler can settle.
        const { assert!(THEN < HEIGHT, "every named row has to be inside the card") };
        const { assert!(SIGN.1 + SIGN.3 <= LANES.1, "the arrow must not reach the lane bank") };
        const { assert!(LANES.1 + LANES.3 <= ONTO, "the bank must not reach the street name") };
        const { assert!(FIGURE.1 + FIGURE.2 <= WORD, "the figure must not reach the word below it") };

        assert!(!card(900.0, 3).lanes_shown(), "lanes at 900 m are noise");
        assert!(card(200.0, 3).lanes_shown(), "lanes at 200 m are the whole point");
        assert!(!card(200.0, 1).lanes_shown(), "one lane is not a choice");
        assert!(!card(200.0, 3).urgent() && card(100.0, 3).urgent());

        // And the countdown drains, rather than filling.
        assert!(card(500.0, 2).left() > card(100.0, 2).left());
        assert_eq!(card(0.0, 2).left(), 0.0);
        assert_eq!(card(4000.0, 2).left(), 1.0);
    }

    #[test]
    fn a_distance_is_a_number_and_a_unit_beside_it() {
        assert_eq!(figure(243.0), ("240".to_string(), "m"));
        assert_eq!(figure(1420.0), ("1.4".to_string(), "km"));
        // The figure and its unit together have to stay inside the card at the widest they get.
        for metres in [0.0, 950.0, 12_345.0] {
            let (number, unit) = figure(metres);
            let end = FIGURE.0 + Canvas::digits_size(&number).0 + 1 + unit.len() as u16;
            assert!(end < WIDTH, "{number} {unit} runs off the card at {end}");
        }
    }

    /// Both blocks have to fit the pane they are placed in — and everything in the gauge has to be
    /// inside the gauge, which is the whole reason the limit is not in a little box of its own.
    #[test]
    fn neither_block_is_wider_than_the_smallest_map_pane() {
        assert!(GAUGE.0 < WIDTH, "the gauge is the narrower of the two, by design");
        assert!(HEIGHT + GAUGE.1 + 2 <= 27, "both have to fit in a 30-row window's map pane");

        // A three-digit speed and the longest line of small print beside it: all inside the plate,
        // with the borders the braille fallback draws counted as occupied.
        assert!(SPEED_RIGHT > Canvas::digits_size("999").0, "a 3-digit speed starts inside");
        const { assert!(ASIDE > SPEED_RIGHT, "the small print sits beside the speed, not over it") };
        let widest = ["▲ 99 over", "km/h", "LIMIT 130"].map(text_width).into_iter().max().unwrap();
        assert!(ASIDE + widest < GAUGE.0, "the small print has to be inside the plate");
        // And the gauge is three rows of digits between two rows of border.
        assert_eq!(GAUGE.1, Canvas::digits_size("0").1 + 2);
    }
}

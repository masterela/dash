//! The frame, in three tiers of how badly the driver needs it.
//!
//! The screen is ten inches, half a metre from someone who is looking at the road. That is the only
//! design input that matters, and it gives a hierarchy rather than a taste:
//!
//! 1. **Act on it.** The next manoeuvre and the speed. Each gets a plate in the picture, in opposite
//!    corners of the map, holding an arrow five cell rows tall — a hundred and ten arcminutes at that
//!    distance, which is recognised rather than read — and a number in three-row block digits, which
//!    is the treatment every number on this dash gets. Neither plate ever moves, so a glance is aimed
//!    rather than spent searching.
//! 2. **Confirm it.** Which street, where we are going, when we arrive, how much is left. Ordinary
//!    cell text in the rail: a driver reads these once on the approach and then stops caring.
//! 3. **Ambient.** Fuel, range, trip, traffic. Dim, bottom of the rail, and silent until one of them
//!    stops being ambient — low fuel and a stopped road are the two things down there that interrupt.
//!
//! The tiers are also what decides how much room the map gets. A map is the largest thing on a
//! dashboard and, for a driver being told where to turn, very nearly the least important — so the
//! rail takes two fifths of a small window and never less than a quarter of a large one, and of what
//! is left the two tier-one blocks cover a corner each. The map is the ground the answers are drawn
//! on, not the answer.
//!
//! Above all of it is the strip, which is the cabin rather than the drive: the time of day, the sky,
//! and what is playing. It is tier three by need and tier one by size, which is not a contradiction —
//! a passenger reads the clock from the back seat, and none of it is ever in the way of the map.
//!
//! Two kinds of content, two treatments. Geometry wants pixels — so on a terminal that can show a
//! picture it gets one, and the cell grid keeps only the words. Everything in tiers two and three is
//! language and gets whole cells either way, because the terminal draws text at its own resolution,
//! which is sharper than any bitmap font this program could carry.

use conui::widget::{Hints, Reading};
use conui::{Canvas, Color, Frame, Rect, Role, Style, View, clip, text_width};

use crate::dash::{Dash, clock_text};
use crate::dots::Dots;
use crate::hud::{self, Card, Gauge};
use crate::route::{Route, Turn};
use crate::skin::{Skin, ink};
use crate::view::{self, Camera, Labels, Projector, name_the_view};
use crate::world::{self, distance};

/// Columns the right-hand rail takes: the width its content was designed at, two fifths of a small
/// window, and never less than a quarter of a big one.
///
/// Thirty-eight is the width the three columns were drawn for. On a desktop terminal that is a tenth
/// of the screen, which is how the complaint a map always earns — that it has taken everything — comes
/// back at a size nobody designed for: the rail stops being a panel and becomes a margin. So a quarter
/// is the floor, and on a screen that wide the readings themselves get bigger rather than the gaps.
const RAIL: u16 = 38;
const RAIL_SHARE: (u16, u16) = (2, 5);
const RAIL_FLOOR: u16 = 4;
const RAIL_CAP: u16 = 96;

/// Minutes into the day when the engine started, so the arrival clock reads like a real one.
const DEPARTED: f32 = 18.0 * 60.0 + 31.0;

/// The widest the map is ever rendered, in pixels. Roughly a 24 ms frame.
const WIDEST: f32 = 2400.0;

/// Rows the ambient block takes off the bottom of the rail.
const AMBIENT: u16 = 5;

/// Rows one turn of the itinerary takes when the rail has rows to spare: the street, three of block
/// digits under it, and one blank so the next street is a heading rather than the next line down.
const TURN_TALL: usize = 5;

/// The rail's three columns: a dim label, the value, and the note that qualifies it.
///
/// Fixed columns rather than a label pushed left and a value pushed right, because a column is what
/// makes a stack of readings scannable — the eye runs down the values without reading the labels at
/// all, which is most of what a rail is for. The third column is the difference between a fact and a
/// decision: `24.8 km` is how far, `via A12` is which way, and `+6 min` is only worth knowing at all
/// once it says where. Below [`NARROW`] there is no room for it, so it is dropped rather than wrapped.
const LABEL: i32 = 0;
const VALUE: i32 = 8;
const NOTE: i32 = 17;
const NARROW: u16 = 30;

/// How the map pane is drawn.
#[derive(Clone, Copy)]
pub enum Paint {
    /// Real pixels, sent to the terminal alongside the frame. `cell` is one cell in pixels.
    Pixels { cell: (u16, u16) },
    /// Braille, for a terminal that cannot show a picture.
    Dots,
}

/// Everything about a frame that both the cell grid and the picture have to agree on.
///
/// Built once, before drawing, because the labels and the card have to be known to the pixel
/// renderer — it puts a plate under each of them — and to the cell grid, which writes their text.
pub struct Plan {
    pub map: Rect,
    pub view: Projector,
    pub labels: Labels,
    pub card: Option<Card>,
    pub gauge: Option<Gauge>,
    dots: Option<Dots>,
}

/// How many columns the rail gets at a given window width.
fn rail_width(width: u16) -> u16 {
    let designed = RAIL.min(width * RAIL_SHARE.0 / RAIL_SHARE.1).max(20);
    designed.max(width / RAIL_FLOOR).min(RAIL_CAP)
}

/// Rows the top strip takes, its rule included.
///
/// Three rows and a rule when the window can carry a strip worth glancing at — the clock in block
/// digits, the weather, what is playing — and one row and a rule when it cannot. It is the only band
/// whose height changes, so both the layout and the map have to ask rather than assume.
fn strip_height(width: u16, height: u16) -> u16 {
    if width >= 76 && height >= 24 { 4 } else { 2 }
}

pub fn plan(dash: &Dash, width: u16, height: u16, paint: Paint, skin: &'static Skin) -> Plan {
    let rail = rail_width(width);
    let strip = strip_height(width, height);
    let map = Rect::new(
        0,
        strip,
        width.saturating_sub(rail + 1).max(1),
        height.saturating_sub(strip + 1).max(1),
    );

    let camera = Camera {
        at: dash.at,
        heading: dash.heading,
        north_up: dash.north_up,
        metres_across: dash.metres_across,
    };
    // Same camera, same labels, different units: braille dots or pixels.
    let cell = match paint {
        Paint::Pixels { cell } => {
            let cell = (f32::from(cell.0), f32::from(cell.1));
            // A very wide window at a very dense cell would put the frame budget, not the map, in
            // charge. Past the cap the terminal scales the last few percent, which nobody sees.
            let over = (f32::from(map.width) * cell.0 / WIDEST).max(1.0);
            (cell.0 / over, cell.1 / over)
        }
        Paint::Dots => (2.0, 4.0),
    };
    let view = Projector::new(
        &camera,
        f32::from(map.width) * cell.0,
        f32::from(map.height) * cell.1,
        cell,
    );

    let card = card(dash, map);
    let gauge = gauge(dash, map);
    let mut labels = Labels::new(map.width, map.height);
    // Tier one goes first, and as whole blocks: a street name that ran under either of them would
    // make the two things on screen that have to be readable the two things that are not.
    let blocks = [card.as_ref().map(|card| card.area), gauge.as_ref().map(|gauge| gauge.area)];
    for area in blocks.into_iter().flatten() {
        for row in area.y..area.y + area.height {
            labels.reserve(i32::from(area.x) - 1, i32::from(row), area.width + 2);
        }
    }
    name_the_view(&dash.world, dash.route.as_ref(), &view, &mut labels);

    let dots = matches!(paint, Paint::Dots).then(|| {
        let mut dots = Dots::new(map.width, map.height, skin);
        dots.draw(&dash.world, dash.route.as_ref(), &view);
        dots
    });

    Plan { map, view, labels, card, gauge, dots }
}

/// The next manoeuvre, as a card, if there is one and there is room for it.
///
/// Top-left of the map, because that is where a left-to-right reader's eye lands when it comes back
/// from the road, and because the map's own subject — the car — sits low and centre.
fn card(dash: &Dash, map: Rect) -> Option<Card> {
    let route = dash.route.as_ref()?;
    if map.width < hud::WIDTH + 2 || map.height < hud::HEIGHT + 1 {
        return None;
    }
    let next = route.next_maneuver(dash.travelled);
    Some(Card {
        area: Rect::new(1, 0, hud::WIDTH, hud::HEIGHT),
        turn: next.turn,
        away: (next.at - dash.travelled).max(0.0),
        lanes: next.lanes.clone(),
    })
}

/// The speedometer, bottom-right of the map: the far corner from the manoeuvre.
///
/// The two tier-one blocks want to be as far apart as the pane allows, so that a glance aimed at one
/// of them cannot land on the other. The card takes the corner a left-to-right reader's eye goes to
/// first, and the speed — which is checked rather than followed — takes the one it goes to last.
fn gauge(dash: &Dash, map: Rect) -> Option<Gauge> {
    if map.width < hud::GAUGE.0 + 2 || map.height < hud::HEIGHT + hud::GAUGE.1 + 2 {
        return None;
    }
    let area = Rect::new(
        map.width - hud::GAUGE.0 - 1,
        map.height - hud::GAUGE.1 - 1,
        hud::GAUGE.0,
        hud::GAUGE.1,
    );
    // And not over the car. The plate is opaque and the car is the one thing on this map that is not
    // furniture, so on a pane small enough for the two to want the same cells the speed goes to the
    // rail instead — a fallback, the same as everywhere else here, rather than a covered-up car.
    if !clears(area, map) {
        return None;
    }
    Some(Gauge {
        area,
        kmh: (dash.speed * 3.6).round().max(0.0) as u32,
        limit: dash.class().limit(),
    })
}

/// Whether `block` leaves the car alone: either it ends before the car's column or it starts below
/// the car's row, with a cell of margin, because the marker is drawn a little wider than the cell it
/// is centred in.
fn clears(block: Rect, map: Rect) -> bool {
    let car = (f32::from(map.width) * view::CAR.0, f32::from(map.height) * view::CAR.1);
    car.0 + 2.0 < f32::from(block.x) || car.1 + 2.0 < f32::from(block.y)
}

impl Plan {
    /// How big the picture of the map pane has to be, in pixels.
    pub fn pixels(&self) -> (u32, u32) {
        (self.view.width.round().max(1.0) as u32, self.view.height.round().max(1.0) as u32)
    }
}

pub fn compose(frame: &mut Frame<'_>, dash: &Dash, plan: &Plan, skin: &'static Skin) {
    let (width, height) = frame.size();
    let rail = rail_width(width);
    let strip_rows = strip_height(width, height);
    let rows = height - strip_rows - 1;

    frame.canvas(Rect::new(0, 0, width, strip_rows), |canvas| strip(canvas, dash));
    frame.canvas(plan.map, |canvas| draw_map(canvas, dash, plan, skin));
    frame.canvas(Rect::new(width - rail - 1, strip_rows, 1, rows), |canvas| {
        canvas.rule_vertical(0, 0, rows, Role::Dim);
    });
    frame.canvas(Rect::new(width - rail, strip_rows, rail, rows), |canvas| {
        draw_rail(canvas, dash, plan, skin);
    });
    frame.canvas(Rect::new(0, height - 1, width, 1), |canvas| footer(canvas, dash));
}

/// The band above the map: the time, where we are, the sky, and what is playing.
///
/// Nothing up here is driving. It is the row a passenger reads and a driver checks at a red light, and
/// at three rows it can afford the clock in block digits — which is the point of giving it three, since
/// the time of day is the one thing in the frame that is read from further away than the wheel.
fn strip(canvas: &mut Canvas<'_>, dash: &Dash) {
    let area = canvas.screen_area();
    let (width, height) = (area.width, area.height);
    let clock = clock_text(DEPARTED + dash.clock / 60.0);
    let street = dash.street();
    let view = if dash.north_up { "north up" } else { "heading up" };
    let scale = format!("{} m · {view}", dash.metres_across as u32);

    if height < 4 {
        canvas.put(0, 0, "DASH", Role::Dim);
        canvas.put(6, 0, &clock, Role::Muted);
        // The street under the wheels, which is the one thing up here a driver ever looks for.
        canvas.put(13, 0, "on", Role::Dim);
        canvas.put(16, 0, street, Role::Text);
        // Only worth saying at all because the zoom keys exist; said as quietly as that deserves.
        if i32::from(width) - i32::from(text_width(&scale)) > 18 + i32::from(text_width(street)) {
            canvas.put_right(i32::from(width) - 1, 0, &scale, Role::Dim);
        }
        canvas.rule(0, height as i32 - 1, width, Role::Dim);
        return;
    }

    canvas.digits(0, 0, &clock, Role::Text);
    let mut x = i32::from(Canvas::digits_size(&clock).0) + 2;
    canvas.put(x, 0, "DASH", Role::Dim);
    canvas.put_truncated(x, 1, street, width.saturating_sub(x as u16 + 2), Role::Text);
    canvas.put(x, 2, &scale, Role::Dim);
    x += i32::from(text_width(street).max(text_width(&scale))) + 3;

    // The music takes the right end, because it is the one block up here that is not about the world
    // outside the car — and the weather goes between them, where there is no competition for the eye.
    let played = music(canvas, dash, width);
    let room = played - x;
    if room > 20 {
        sky(canvas, dash, x + (room - 18) / 2);
    }
    canvas.rule(0, 3, width, Role::Dim);
}

/// The weather: a glyph, the temperature in block digits, and the word for what the sky is doing.
///
/// The same shape as the speedometer — a number at size with its small print stacked beside it — because
/// a second treatment for a second number is a second thing to learn.
fn sky(canvas: &mut Canvas<'_>, dash: &Dash, x: i32) {
    let weather = &dash.weather;
    canvas.put(x, 1, weather.sky.glyph(), Role::Warn);
    let degrees = weather.degrees().to_string();
    canvas.digits(x + 3, 0, &degrees, Role::Text);
    let aside = x + 3 + i32::from(Canvas::digits_size(&degrees).0) + 1;
    canvas.put(aside, 0, "°C", Role::Muted);
    canvas.put(aside, 2, weather.sky.word(), Role::Dim);
}

/// What is playing, and the three controls that are the whole of what a dash lets anybody do about it.
///
/// Returns the column it starts at, so the strip knows how much room is left in the middle.
fn music(canvas: &mut Canvas<'_>, dash: &Dash, width: u16) -> i32 {
    let player = &dash.player;
    let block = (width / 4).clamp(18, 40);
    if width < 76 {
        return i32::from(width);
    }
    let x = i32::from(width - block);
    let track = player.track();

    canvas.put(x, 0, "♫", Role::Accent);
    canvas.put_truncated(x + 2, 0, track.title, block - 2, Role::Text);
    // The times are the first thing to go: a truncated artist is not an artist, and the bar under it
    // already says roughly where in the track this is.
    let times = format!("{} / {}", player.at(), player.of());
    let clocks = block >= 26;
    let room = if clocks { block - text_width(&times) - 3 } else { block - 2 };
    canvas.put_truncated(x + 2, 1, track.artist, room, Role::Dim);
    if clocks {
        canvas.put_right(i32::from(width), 1, &times, Role::Dim);
    }

    // The middle control shows what it will do, the way every player a driver has ever used does: a
    // pause sign while it is playing, and a play sign while it is not.
    let transport =
        if player.playing { "◀◀   ▌▐   ▶▶" } else { "◀◀   ▶    ▶▶" };
    let wide = canvas.put(x, 2, transport, Role::Accent);
    // The bar is where the time has gone, beside the controls that are the only way to change it.
    canvas.bar(x + i32::from(wide) + 2, 2, player.through(), block - wide - 2, Role::Muted);
    x
}

/// The map pane's share of the frame: the words, the card's figures, and the dots if no picture.
fn draw_map(canvas: &mut Canvas<'_>, dash: &Dash, plan: &Plan, skin: &'static Skin) {
    if let Some(dots) = &plan.dots {
        dots.blit(canvas);
        let (x, y) = plan.view.anchor_cell();
        canvas.set(x, y, car_glyph(dash), Role::Accent);
    }

    for label in plan.labels.placed() {
        let (x, y) = (i32::from(label.col), i32::from(label.row));
        match &plan.dots {
            // Carry the ground colour up into the text, or a name over water looks cut out.
            Some(dots) => {
                let style = Style::EMPTY
                    .fg(skin.theme.color(label.role))
                    .bg(dots.background(label.col, label.row));
                canvas.put_styled(x, y, &label.text, style);
            }
            // Over a picture there is nothing to match: the plate is already in the pixels.
            None => {
                canvas.put(x, y, &label.text, label.role);
            }
        }
    }

    // In braille neither block has a picture to sit on, so each brings its own plate.
    let plate = plan.dots.is_some().then_some(skin.theme.surface);
    if let (Some(card), Some(route)) = (&plan.card, &dash.route) {
        let next = route.next_maneuver(dash.travelled);
        let then = route.after_next(dash.travelled).map(|after| (after.turn, after.onto.as_str()));
        hud::cells(canvas, card, &next.onto, then, plate);
    }
    if let Some(gauge) = &plan.gauge {
        hud::gauge_cells(canvas, gauge, plate);
    }

    if plan.dots.is_none() {
        // conui gives every cell a background colour, every frame. The protocol says an image at
        // `z=-1` goes over those and under the text, but a terminal that reads it as "under the
        // cell entirely" would hide the map and leave exactly the names showing. The terminal's own
        // default background is the one thing nothing can be painted over, and the picture is
        // supplying the colour anyway — so the pane asks for that instead and the question is moot.
        let area = canvas.screen_area();
        let all = Rect::new(0, 0, area.width, area.height);
        canvas.style_area(all, Style::EMPTY.bg(Color::Reset));
    }
}

/// Which way the car points on screen, which in heading-up is always up.
fn car_glyph(dash: &Dash) -> char {
    if !dash.north_up {
        return '▲';
    }
    let (east, south) = (dash.heading.cos(), dash.heading.sin());
    if east.abs() > south.abs() {
        if east > 0.0 { '▶' } else { '◀' }
    } else if south > 0.0 {
        '▼'
    } else {
        '▲'
    }
}

/// The rail: tier two, tier three, and tier one only when the map pane was too small to draw it.
///
/// The speedometer and the manoeuvre live in the picture now, on plates of their own. But a window
/// can be too small for either block — see [`card`] and [`gauge`] — and a window too small for
/// the picture still has a driver in front of it, so the rail keeps cell versions of both and puts
/// them above everything else. What is left is the material a driver reads once on the approach and
/// then stops caring about, which is what cell text is better at than any bitmap would be.
fn draw_rail(canvas: &mut Canvas<'_>, dash: &Dash, plan: &Plan, skin: &'static Skin) {
    let area = canvas.screen_area();
    let (width, height) = (area.width, area.height);
    let mut top = 0;

    if plan.card.is_none() {
        top += fallen_next(canvas, dash, width, top);
    }
    if plan.gauge.is_none() {
        top += fallen_speed(canvas, dash, width, top);
    }

    // Tier three keeps the bottom of the rail, unless keeping it would leave tier two nothing.
    let mut floor = i32::from(height);
    if floor - i32::from(AMBIENT) >= top + 2 {
        ambient(canvas, dash, width, floor);
        floor -= i32::from(AMBIENT);
    }

    // Choosing where to go takes the rest of the rail, and squeezes the journey down to what still
    // matters while it is open: a driver doing this is stopped, and the thing they are doing is the
    // list. It used to be a panel over the map, which meant the question covered the answer to the
    // last one — in the rail it covers nothing at all, and the map stays whole.
    if dash.search.is_some() {
        // The rule, the query and the count, plus as much of the list as it has and eight rows allow.
        let wants =
            3 + dash.search.as_ref().map_or(0, |search| search.hits.len().clamp(1, 8)) as i32;
        let shrunk = (floor - wants).max(top);
        floor = search(canvas, dash, width, (shrunk, floor));
    }

    match &dash.route {
        Some(route) => {
            let y = journey(canvas, dash, route, (top, (floor - top).max(0) as u16), width, skin);
            itinerary(canvas, dash, route, (y, floor), width);
        }
        None => {
            canvas.put(LABEL, top, "NO ROUTE", Role::Dim);
            canvas.put(LABEL, top + 2, "press s to pick a destination", Role::Muted);
        }
    }
}

/// One line of the rail: a dim label, a value, and a note that qualifies it. See [`VALUE`].
///
/// A thin wrapper over [`Reading`] rather than a call site, because the three columns and the width
/// they collapse at are this rail's rather than the widget's, and stating them at five call sites is
/// how one of them ends up a column out.
fn line(canvas: &mut Canvas<'_>, y: i32, width: u16, label: &str, value: (&str, Role), note: &str) {
    let (text, role) = value;
    let reading = Reading::new(label, text)
        .role(role)
        .note(note)
        .columns(VALUE as u16, NOTE as u16)
        .narrow(NARROW);
    reading.render(&mut canvas.shifted(LABEL, y, width, 1));
}

/// Tier two: where we are going, when we arrive, and how much of it is left. Returns the row below it.
///
/// Three depths, because the rail is the part of the frame that shrinks. With room, the arrival time is
/// block digits; with less, the same facts one to a line; with almost none, only the two a driver would
/// still act on. The order they are dropped in is the order they matter in, which is what makes it
/// degrade rather than break.
fn journey(
    canvas: &mut Canvas<'_>,
    dash: &Dash,
    route: &Route,
    at: (i32, u16),
    width: u16,
    skin: &'static Skin,
) -> i32 {
    let (top, rows) = at;
    let eta = dash.eta();
    let minutes = format!("in {} min", (eta / 60.0).ceil() as u32);
    let left = metres(dash.remaining());

    canvas.put(LABEL, top, "TO", Role::Dim);
    canvas.put_truncated(3, top, &route.destination, width.saturating_sub(3), Role::Text);
    if rows < 5 {
        canvas.put(LABEL, top + 1, &minutes, Role::Info);
        canvas.put_right(i32::from(width), top + 1, &left, Role::Muted);
        return top + 2;
    }

    canvas.rule(LABEL, top + 1, width, Role::Dim);
    // The arrival as a time of day rather than as a countdown: a driver compares it against the hour
    // they said they would be there, and a clock is already in those terms. The minutes are the same
    // fact in the terms the next twenty of them are spent in, which is the other question.
    let arrival = clock_text(DEPARTED + (dash.clock + eta) / 60.0);
    let via = dash.via().map(|road| format!("via {road}")).unwrap_or_default();
    // Wide enough for the third column, too: on a narrow rail the clock has nowhere to put the minutes
    // beside it, and a label alone over three rows of digits is a caption, not a line.
    let deep = rows >= 9 && width >= NARROW && dash.eta() > 0.0;

    // One under the other, never side by side. They were beside each other for a while, on a rail wide
    // enough for two block numbers — and two big numbers a hand's width apart are a sum a driver has to
    // work out which way round: `18:35` and `2.9` do not belong to the same question, and read as a pair
    // they look like they might. Stacked, the big one is when and the small one under it is how far,
    // which is the order the questions are actually asked in.
    let mut y = if deep {
        line(canvas, top + 2, width, "ARRIVAL", ("", Role::Info), &minutes);
        canvas.digits(VALUE, top + 3, &arrival, Role::Info);
        line(canvas, top + 6, width, "LEFT", (&left, Role::Text), &via);
        top + 7
    } else {
        line(canvas, top + 2, width, "ARRIVAL", (&arrival, Role::Info), &minutes);
        line(canvas, top + 3, width, "LEFT", (&left, Role::Text), &via);
        top + 4
    };

    traffic(canvas, dash, y, width, skin);
    y += 1;
    y
}

/// The rest of the turns, filling whatever the rail has left between the journey and the ambient block.
///
/// Not a second copy of the manoeuvre: the next one is on the card, or at the top of the rail if the
/// card did not fit, and this starts after it. It is the part of a route a driver wants before setting
/// off and again whenever the traffic changes — "how many more turns is this" — and it is the one thing
/// in the rail that is happy to be any height at all, which is what makes it the right thing to give
/// the leftover rows to.
fn itinerary(canvas: &mut Canvas<'_>, dash: &Dash, route: &Route, span: (i32, i32), width: u16) {
    let (top, bottom) = span;
    let rows = bottom - top - 2;
    if rows < 2 {
        return;
    }

    let rest: Vec<_> = route.to_come(dash.travelled).skip(1).collect();
    if rest.is_empty() {
        return;
    }
    canvas.rule(LABEL, top, width, Role::Dim);
    let turns = rest.len() - usize::from(rest.last().is_some_and(|last| last.turn == Turn::Arrive));
    let counted = match turns {
        1 => "1 more turn".to_string(),
        other => format!("{other} more turns"),
    };
    canvas.put(LABEL, top + 1, "ROUTE", Role::Dim);
    if width >= NARROW {
        canvas.put(NOTE, top + 1, &counted, Role::Dim);
    }

    // As many as fit, and a count of what did not: a list that runs off the bottom of the rail has
    // quietly become a lie about how many turns are left.
    //
    // Each turn gets a street on one row and its distance in block digits under it when the block can
    // give every one of them that — all of them or none, because a list whose first two entries are
    // twice the height of the rest is two lists. This is where the rail's spare rows go, and a tall
    // rail has plenty: the alternative was thirty blank rows above the fuel gauge and a route written
    // in the smallest type on the screen, which is the wrong way round for the one block a driver
    // reads before setting off. The digits stay quieter than the arrival clock, so the biggest thing
    // in the rail is still the only one in `Info`: the hierarchy is kept in colour, not in size.
    let room = (rows - 1) as usize;
    let tall = width >= NARROW && room >= rest.len() * TURN_TALL;
    let per = if tall { TURN_TALL } else { 1 };
    let capacity = room / per;
    let shown = if rest.len() > capacity { capacity.saturating_sub(1) } else { rest.len() };
    for (row, turn) in rest.iter().take(shown).enumerate() {
        let y = top + 2 + (row * per) as i32;
        let away = (turn.at - dash.travelled).max(0.0);
        canvas.put(LABEL, y, turn.turn.arrow(), Role::Muted);
        if !tall {
            canvas.put(2, y, &metres(away), Role::Muted);
            canvas.put_truncated(VALUE + 2, y, &turn.onto, width - VALUE as u16 - 2, Role::Dim);
            continue;
        }
        canvas.put_truncated(2, y, &turn.onto, width - 2, Role::Text);
        let (number, unit) = hud::figure(away);
        canvas.digits(2, y + 1, &number, Role::Muted);
        let after = 2 + i32::from(Canvas::digits_size(&number).0) + 1;
        canvas.put(after, y + 2, unit, Role::Dim);
    }
    if shown < rest.len() {
        let more = format!("+{} more", rest.len() - shown);
        canvas.put(2, top + 2 + (shown * per) as i32, &more, Role::Dim);
    }
}

/// The traffic line: what it costs, and where.
///
/// The one line in the rail that is not in a theme role. It is said in the map's own traffic colours,
/// which is what puts the two in agreement — the red stretch drawn on the route and the words here are
/// the same number read twice. And it names the street, because a driver told "stopped" has a fact
/// while a driver told "stopped, Bd Voltaire" has a decision.
fn traffic(canvas: &mut Canvas<'_>, dash: &Dash, y: i32, width: u16, skin: &'static Skin) {
    let worst = dash.named_worst();
    let jam = worst.map_or(1.0, |(_, jam)| jam);
    let (word, colour) = if jam < world::BUSY {
        ("clear", skin.map.clear)
    } else if jam < world::STOPPED {
        ("slow", skin.map.busy)
    } else {
        ("stopped", skin.map.stopped)
    };
    let style = Style::EMPTY.fg(ink(colour));

    canvas.put(LABEL, y, "TRAFFIC", Role::Dim);
    // Minutes lost if there are any, and the word if there are not: a delay is what a driver acts on,
    // and "clear" is worth a line only because its absence would leave them wondering.
    let cost = match dash.lost() {
        Some(lost) => format!("+{lost} min"),
        None => word.to_string(),
    };
    if width < NARROW {
        let x = i32::from(width) - i32::from(text_width(&cost));
        canvas.put_styled(x, y, &cost, style);
        return;
    }
    canvas.put_styled(VALUE, y, &cost, style);
    // And where, once there is a where. The word is in the value column when there are no minutes to
    // put there, so the street stands on its own rather than being labelled twice.
    if let (Some((street, _)), true) = (worst, jam >= world::BUSY) {
        canvas.put_styled(NOTE, y, clip(street, width - NOTE as u16), style);
    }
}

/// The speedometer in cells, for a pane too short to draw one. Returns the rows it took.
///
/// The same three lines of small print the plate carries, beside the same block digits, in the same
/// order — so the fallback is not a different readout, it is the same one without a plate under it.
fn fallen_speed(canvas: &mut Canvas<'_>, dash: &Dash, width: u16, top: i32) -> i32 {
    let gauge = Gauge::reading((dash.speed * 3.6).round().max(0.0) as u32, dash.class().limit());
    let role = gauge.role();

    let text = gauge.kmh.to_string();
    let wide = i32::from(Canvas::digits_size(&text).0);
    canvas.digits(LABEL, top, &text, role);
    // Beside the figure rather than at the rail's far edge, and clamped so three digits and a limit of
    // 130 still both fit: the small print belongs to the number, not to the column.
    hud::aside(canvas, &gauge, (wide + 2).min(i32::from(width) - 9), top);
    canvas.rule(LABEL, top + 3, width, Role::Dim);
    4
}

/// The manoeuvre in cells, for a pane too small for the card. Returns the rows it took.
///
/// Four rows and a rule, always the same four, whether or not there are lanes to show — the card's
/// first rule is that nothing moves, and a fallback that reflows is a fallback a driver has to read.
fn fallen_next(canvas: &mut Canvas<'_>, dash: &Dash, width: u16, top: i32) -> i32 {
    let Some(route) = &dash.route else { return 0 };
    let next = route.next_maneuver(dash.travelled);
    let away = (next.at - dash.travelled).max(0.0);
    let role = if away < Card::URGENT { Role::Warn } else { Role::Accent };

    // Seconds, not metres, on the second column: the distance is already the value beside the arrow,
    // and at 300 m what a driver is actually asking is how long they have to get across.
    let seconds = (away / dash.speed.max(4.0)).round() as u32;
    let when =
        if seconds < 60 { format!("in {seconds} s") } else { format!("in {} min", seconds / 60) };
    canvas.put(LABEL, top, "NEXT", Role::Dim);
    // This block only ever appears on a rail too narrow for the third column proper — a window wide
    // enough for that is wide enough for the card — so it goes by whether the words fit, not by
    // [`NARROW`], which would drop them at every size this is ever drawn at.
    if width >= NOTE as u16 + 9 {
        canvas.put(NOTE, top, &when, Role::Dim);
    }

    canvas.put(LABEL, top + 1, next.turn.arrow(), role);
    canvas.put(2, top + 1, &metres(away), role);
    canvas.put_truncated(LABEL, top + 2, &next.onto, width, Role::Text);

    // The lanes as the row of arrows the card draws as a carriageway, which is as much of that idea as
    // cells can carry. Blank until they matter, like everything else on the card.
    if away < Card::CLOSE && next.lanes.len() > 1 {
        canvas.put(LABEL, top + 3, "LANES", Role::Dim);
        for (lane, &ours) in next.lanes.iter().enumerate() {
            let (glyph, role) = if ours { (next.turn.arrow(), role) } else { ("↑", Role::Dim) };
            canvas.put(VALUE + lane as i32 * 2, top + 3, glyph, role);
        }
    }
    canvas.rule(LABEL, top + 4, width, Role::Dim);
    5
}

/// Where to go, in the rail. Returns the row it starts at, which is the floor for everything above.
///
/// Not a panel over the map: a panel over the map covers the answer to the question the driver asked a
/// minute ago in order to ask them a new one. Here it takes the bottom of the rail, the map stays
/// whole, and the speedometer stays on it — the search used to hide that, which is a strange thing to
/// do to the one number a stopped car is about to need.
fn search(canvas: &mut Canvas<'_>, dash: &Dash, width: u16, span: (i32, i32)) -> i32 {
    let Some(search) = &dash.search else { return span.1 };
    let (top, bottom) = span;
    canvas.rule(LABEL, top, width, Role::Dim);
    canvas.put(LABEL, top + 1, "WHERE TO", Role::Accent);

    // The query with a cursor on it, because a text field with no cursor is a label.
    let shown = format!("/{}▏", search.text);
    let role = if search.text.is_empty() { Role::Dim } else { Role::Text };
    canvas.put(VALUE + 1, top + 1, clip(&shown, width - VALUE as u16 - 1), role);

    // A rule, the query and the count are the three rows that are not the list.
    let rows = (bottom - top - 3).max(0);
    if search.hits.is_empty() {
        let word =
            if search.text.is_empty() { "type a name or a kind" } else { "nothing by that name" };
        canvas.put(LABEL, top + 2, word, Role::Muted);
        return top;
    }

    // Scroll the window rather than the list, so the cursor is always on screen.
    let first = search.cursor.saturating_sub((rows as usize).saturating_sub(1));
    let names = width >= NARROW;
    for (row, &poi) in search.hits.iter().skip(first).take(rows as usize).enumerate() {
        let y = top + 2 + row as i32;
        let poi = &dash.world.pois[poi];
        let chosen = first + row == search.cursor;
        canvas.put(LABEL, y, if chosen { "▸" } else { " " }, Role::Accent);
        let role = if chosen { Role::Text } else { Role::Muted };
        // The distance is the second thing anybody wants and the first thing to go: a truncated name
        // is not a name, and on a narrow rail there is only room for one of the two.
        let room = if names { width - 9 } else { width - 2 };
        canvas.put_truncated(2, y, poi.name, room, role);
        if names {
            let away = distance(poi.at, dash.at);
            canvas.put_right(i32::from(width), y, &metres(away), Role::Dim);
        }
    }

    // How much was searched, quietly, because it is the reason the list is trustworthy rather than
    // anything a driver acts on.
    // Under the last name rather than at the foot of the block: a count that floats a row below the
    // list it counts reads as the start of something else.
    let counted = format!("{} of {} places", search.hits.len(), dash.world.pois.len());
    let listed = search.hits.len().min(rows as usize) as i32;
    if top + 2 + listed < bottom {
        canvas.put(LABEL, top + 2 + listed, &counted, Role::Dim);
    }
    top
}

/// Tier three: true all the time, and therefore never urgent. Anchored to the bottom so that the
/// tiers above it can grow a line without anything down here appearing to move.
fn ambient(canvas: &mut Canvas<'_>, dash: &Dash, width: u16, height: i32) {
    let top = height - i32::from(AMBIENT);
    canvas.rule(LABEL, top, width, Role::Dim);

    // Fuel is the exception that proves the tier: quiet at three quarters of a tank, and the loudest
    // thing on the rail at a tenth of one. It keeps the bar the other two do not get, because a
    // fraction of a tank is a length and a range in kilometres is a number.
    let low = dash.fuel < 0.12;
    let percent = format!("{} %", (dash.fuel * 100.0) as u32);
    canvas.put(LABEL, top + 1, "FUEL", Role::Dim);
    let bar = if low { Role::Danger } else { Role::Accent };
    // Short enough to leave the percentage a column of its own: a bar that runs into a number has
    // turned two readings into one shape.
    canvas.bar(VALUE, top + 1, dash.fuel, (width - VALUE as u16 - 6).min(12), bar);
    canvas.put_right(
        i32::from(width),
        top + 1,
        &percent,
        if low { Role::Danger } else { Role::Muted },
    );

    line(
        canvas,
        top + 2,
        width,
        "RANGE",
        (&format!("{} km", (dash.fuel * 640.0) as u32), Role::Muted),
        "",
    );
    line(canvas, top + 3, width, "TRIP", (&format!("{:.1} km", dash.trip), Role::Muted), "");
}

/// The keys, with the key itself in the colour of a thing you can press and the word in the colour of
/// a thing you cannot. A footer of uniform grey is a paragraph; this is a row of buttons.
fn footer(canvas: &mut Canvas<'_>, dash: &Dash) {
    let width = canvas.screen_area().width;
    let keys: &[(&str, &str)] = if dash.search.is_some() {
        &[("↑↓", "choose"), ("ENTER", "go"), ("ESC", "back")]
    } else {
        &[
            ("s", "search"),
            ("n", "north"),
            ("p", "pause"),
            ("+/-", "zoom"),
            ("r", "next stop"),
            ("q", "quit"),
            // Last, because this list is dropped from the end and the transport is the only thing in
            // it whose absence from the footer costs a driver nothing they were about to do.
            ("m ‹ ›", "music"),
        ]
    };

    // The status holds the right edge and the keys give way to it, which is the whole reason this is
    // one widget: the room has to come out of the legend before it is filled, and a status drawn
    // afterwards can only overwrite whatever got there first.
    let role = if dash.paused { Role::Warn } else { Role::Muted };
    let hints = keys
        .iter()
        .fold(Hints::new().spacing(2), |hints, (key, word)| hints.key(*key, *word))
        .emphasise_keys()
        .role(Role::Dim)
        .trailing(&dash.status, role);
    hints.render(&mut canvas.shifted(0, 0, width, 1));
}

/// Metres under a kilometre, kilometres above it, the way a navigator says it.
fn metres(value: f32) -> String {
    let (number, unit) = hud::figure(value);
    format!("{number} {unit}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plate may cover the map, which is ground, but not the car, which is the subject.
    ///
    /// Both conditions have to be tested, not just the first: a speedometer that always steps aside and
    /// one that never does are equally wrong, and only one of them is visible in a screenshot.
    #[test]
    fn the_speedometer_gives_the_car_its_cells_back() {
        let dash = Dash::new();
        // Map panes, as `plan` cuts them: three columns for the rule and the rail, three rows for the
        // strip and the footer. 72x24, 100x30, 129x41.
        let panes = [(43, 21), (62, 27), (78, 38)];

        for (width, height) in panes {
            let pane = Rect::new(0, 2, width, height);
            let Some(gauge) = gauge(&dash, pane) else { continue };
            let car = (f32::from(width) * view::CAR.0, f32::from(height) * view::CAR.1);
            assert!(
                f32::from(gauge.area.x) > car.0 || f32::from(gauge.area.y) > car.1,
                "the {width}x{height} pane puts its speedometer on the car"
            );
        }

        assert!(gauge(&dash, Rect::new(0, 2, 43, 21)).is_none(), "that plate lands on the car");
        assert!(gauge(&dash, Rect::new(0, 2, 62, 27)).is_some(), "a 100x30 window fits both");
    }
}

//! The map, drawn as pixels.
//!
//! Same world, same camera, same frame budget as the braille version — but a pixel pane has a few
//! hundred times the positions of a cell pane, and this is what that buys: roads with casings so
//! crossings read as crossings, a river with a shoreline, footprints, a route that curves, and a
//! manoeuvre card drawn at a size a driver can take in without reading.

use crate::hud::{self, Card, Gauge};
use crate::paint::{Band, Rgb};
use crate::route::Route;
use crate::skin::Palette;
use crate::view::{Labels, Projector};
use crate::world::{self, Class, World, distance};

/// How wide a class of way is drawn, never thinner than the eye can follow.
fn width(class: Class, scale: f32) -> f32 {
    let floor = match class {
        Class::Residential => 2.0,
        Class::Primary => 4.5,
        Class::Motorway => 7.0,
    };
    (class.width() * scale).max(floor)
}

#[allow(clippy::too_many_arguments)]
pub fn render(
    band: &mut Band<'_>,
    world: &World,
    route: Option<&Route>,
    view: &Projector,
    heading: f32,
    labels: &Labels,
    card: Option<&Card>,
    gauge: Option<&Gauge>,
    map: &Palette,
) {
    band.fill_all(map.ground);
    let scale = view.scale();
    let here = view.middle();
    let reach = view.width / scale;

    // Water, with a shoreline under it: a band of flat colour reads as a hole until it has an edge.
    let river: Vec<(f32, f32)> = world.river.iter().map(|&point| view.project(point)).collect();
    band.stroke(&river, crate::world::RIVER * scale + 4.0, map.shore);
    band.stroke(&river, crate::world::RIVER * scale, map.water);

    for &(x, y, w, h) in &world.parks {
        band.area(&quad(view, x, y, w, h), map.park);
    }

    // Footprints are the texture that tells you the map is of somewhere, so they are worth the
    // pixels — but only the ones that could land on the pane.
    for &(x, y, w, h) in &world.buildings {
        if distance((x, y), here) < reach {
            band.area(&quad(view, x, y, w, h), map.building);
        }
    }

    // Casings for everything before fills for everything, or every junction gets a seam through it.
    for class in [Class::Residential, Class::Primary] {
        for (line, _) in streets(world, view, class) {
            band.stroke(&line, width(class, scale) + 2.5, map.casing);
        }
        for (line, jam) in streets(world, view, class) {
            band.stroke(&line, width(class, scale), stalled(class, jam, map));
        }
    }

    let motorway: Vec<(f32, f32)> = world.motorway.iter().map(|&p| view.project(p)).collect();
    let wide = width(Class::Motorway, scale);
    band.road(&motorway, wide, map.motorway, map.casing);
    if wide > 7.0 {
        band.dashes(&motorway, (wide * 0.08).max(1.0), map.casing, wide * 1.6, wide * 1.6);
    }

    if let Some(route) = route {
        let line: Vec<(f32, f32)> = route.points.iter().map(|&p| view.project(p)).collect();
        let wide = (18.0 * scale).max(6.0);
        band.stroke(&line, wide + 4.0, map.route_casing);
        band.stroke(&line, wide, map.route);

        // Then the traffic, over the top and only where there is any. A route that is moving stays
        // the one colour it started as, so amber and red are events rather than a legend to learn.
        for (index, leg) in route.legs.iter().enumerate() {
            let jam = leg.jam();
            if jam < world::BUSY {
                continue;
            }
            let until = route.legs.get(index + 1).map_or(line.len(), |next| next.from + 1);
            let stretch = &line[leg.from.min(line.len())..until.min(line.len())];
            let colour = if jam < world::STOPPED { map.busy } else { map.stopped };
            band.stroke(stretch, wide, colour);
        }

        if let Some(&end) = line.last() {
            band.disc(end, wide * 0.9, map.route);
            band.disc(end, wide * 0.45, map.ground);
        }
    }

    car(band, view, heading, map);
    plates(band, view, labels, map);
    band.fade_right(((view.width * 0.02) as u32).max(6), map.ground);

    // Last, so nothing in the map is ever drawn across the two blocks that have to stay readable.
    if let Some(card) = card {
        hud::draw(band, card, view.cell(), map);
    }
    if let Some(gauge) = gauge {
        hud::gauge(band, gauge, view.cell(), map);
    }
}

/// A dark plate under every cell a label will occupy.
fn plates(band: &mut Band<'_>, view: &Projector, labels: &Labels, map: &Palette) {
    let (cw, ch) = view.cell();
    for label in labels.placed() {
        let wide = label.text.chars().count() as f32;
        let (x, y) = (f32::from(label.col) * cw - 1.0, f32::from(label.row) * ch);
        let (w, h) = (wide * cw + 2.0, ch);
        band.wash(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)], map.label, 0.62);
    }
}

/// The car: a chevron at the anchor, pointing the way the map says it is going.
fn car(band: &mut Band<'_>, view: &Projector, heading: f32, map: &Palette) {
    let at = view.anchor();
    // Take the screen direction through the projection rather than from the heading directly: in
    // heading-up it comes back as straight up, and in north-up it comes back turned, for free.
    let world_at = view.unproject(at);
    let ahead =
        view.project((world_at.0 + heading.cos() * 50.0, world_at.1 + heading.sin() * 50.0));
    let (dx, dy) = (ahead.0 - at.0, ahead.1 - at.1);
    let length = dx.hypot(dy).max(0.0001);
    let size = (view.height * 0.025).clamp(7.0, 40.0);
    let (fx, fy) = (dx / length, dy / length);
    let (sx, sy) = (-fy, fx);
    let nose = (at.0 + fx * size, at.1 + fy * size);
    let tail = (at.0 - fx * size * 0.65, at.1 - fy * size * 0.65);
    let left =
        (at.0 + sx * size * 0.72 - fx * size * 0.45, at.1 + sy * size * 0.72 - fy * size * 0.45);
    let right =
        (at.0 - sx * size * 0.72 - fx * size * 0.45, at.1 - sy * size * 0.72 - fy * size * 0.45);

    band.disc(at, size * 1.15, map.route_casing);
    band.shape(&[nose, left, tail, right], map.car);
}

fn quad(view: &Projector, x: f32, y: f32, w: f32, h: f32) -> Vec<(f32, f32)> {
    [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
        .iter()
        .map(|&corner| view.project(corner))
        .collect()
}

fn colour(class: Class, map: &Palette) -> Rgb {
    match class {
        Class::Residential => map.residential,
        Class::Primary => map.primary,
        Class::Motorway => map.motorway,
    }
}

/// Every street of one class, projected, as two-point lines.
fn streets(world: &World, view: &Projector, class: Class) -> Vec<([(f32, f32); 2], f32)> {
    let mut lines = Vec::new();
    for (streets, down) in [(&world.down, true), (&world.across, false)] {
        for street in streets.iter().filter(|street| street.class == class) {
            let ends = view.street_ends(world, street, down);
            lines.push(([view.project(ends[0]), view.project(ends[1])], street.jam()));
        }
    }
    lines
}

/// A street's own colour, unless it is a boulevard that has stopped moving.
///
/// Red on the map itself and not only on the route, because a router costed by time avoids the jams
/// by construction: leave the traffic on the route alone and the one colour that means "do not go
/// down there" is the one colour a driver never sees. This is why it is worth showing.
///
/// Only red, and only boulevards. Amber is the motorway's colour here, so an amber boulevard would
/// read as a change of road class rather than as a delay, and a residential street never gets bad
/// enough to be worth a colour at all — see [`crate::world::jam`]. On the route, where a stretch is
/// being driven rather than avoided, both colours still apply: that is a fact about this journey.
fn stalled(class: Class, jam: f32, map: &Palette) -> Rgb {
    match class == Class::Primary && jam >= world::STOPPED {
        true => map.stopped,
        false => colour(class, map),
    }
}

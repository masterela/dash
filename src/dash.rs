//! What the dashboard knows: where the car is, where it is going, and what the driver typed.
//!
//! All of it is plain data stepped by elapsed time. Nothing here draws, and nothing here blocks —
//! the loop hands it a `dt` and asks for the next frame's truth.

use conui::{KeyCode, KeyEvent, Modifiers};

use crate::cabin::{Player, Weather};
use crate::route::{Leg, Route};
use crate::world::{Class, World, distance};

/// Kilometres a full tank is good for.
const RANGE: f32 = 640.0;

/// Metres a second through a junction: 25 km/h, which is a turn taken rather than a stop.
const CORNER: f32 = 6.9;

/// Metres a second, a second, of slowing down. Comfortable rather than quick — a quarter of what
/// tyres can do, and about what a driver does when nothing has surprised them.
const BRAKE: f32 = 2.5;

/// And of speeding up, which is the slower of the two in every car ever built.
const ACCEL: f32 = 2.0;

pub struct Search {
    pub text: String,
    pub hits: Vec<usize>,
    pub cursor: usize,
}

pub struct Dash {
    pub world: World,
    pub route: Option<Route>,
    pub travelled: f32,
    /// Metres per second, eased towards what the street allows.
    pub speed: f32,
    pub at: (f32, f32),
    pub heading: f32,
    pub north_up: bool,
    pub metres_across: f32,
    pub paused: bool,
    pub fuel: f32,
    /// Kilometres on this trip.
    pub trip: f32,
    /// Seconds since the engine started.
    pub clock: f32,
    pub search: Option<Search>,
    /// The cabin, which is everything the drive does not depend on. See [`crate::cabin`].
    pub weather: Weather,
    pub player: Player,
    pub status: String,
    /// Seconds left of the arrival message, before the next destination is picked.
    pub settling: f32,
    pub quit: bool,
}

impl Dash {
    pub fn new() -> Dash {
        let world = World::generate();
        // Start in the middle of town. Starting at a corner means half the map is the world's edge,
        // which is the one thing a real map never shows you.
        let start = world.node(world.down.len() / 3, world.across.len() / 2);
        let mut dash = Dash {
            at: world.position(start),
            heading: 0.0,
            world,
            route: None,
            travelled: 0.0,
            speed: 0.0,
            north_up: false,
            metres_across: 1500.0,
            paused: false,
            fuel: 0.72,
            trip: 0.0,
            clock: 0.0,
            search: None,
            weather: Weather::new(),
            player: Player::new(),
            status: String::new(),
            settling: 0.0,
            quit: false,
        };
        // Bastille rather than the nearest stop. The first route a driver is shown has to have turns
        // in it: the straight shot to Gare de Lyon happens to be one leg end to end, so it exercises
        // the arrival sign and nothing else — no lane bank, no left, no right.
        dash.drive_to(6);
        dash
    }

    /// Route from wherever the car is to a point of interest, keeping the trip counter running.
    pub fn drive_to(&mut self, poi: usize) {
        let poi = &self.world.pois[poi];
        let (from, to) = (self.world.nearest_node(self.at), self.world.nearest_node(poi.at));
        if from == to {
            self.status = format!("already at {}", poi.name);
            return;
        }
        match Route::find(&self.world, from, to, poi.name) {
            Some(route) => {
                let (at, heading) = route.locate(0.0);
                self.at = at;
                self.heading = heading;
                self.travelled = 0.0;
                self.status = format!("routing to {}", poi.name);
                self.route = Some(route);
            }
            None => self.status = format!("no way through to {}", poi.name),
        }
    }

    /// Advance `dt` seconds of driving.
    pub fn tick(&mut self, dt: f32) {
        self.clock += dt;
        // The cabin keeps running while the car is stopped: a paused drive is a paused drive, and
        // nobody has ever parked and had the radio stop with the wheels.
        self.weather.tick(dt);
        self.player.tick(dt);
        if self.paused || self.route.is_none() {
            self.speed -= self.speed.min(dt * 6.0);
            return;
        }

        let route = self.route.as_ref().expect("checked just above");
        let next = route.next_maneuver(self.travelled);
        let to_turn = next.at - self.travelled;

        // Lift off before a turn and before arriving: a speed readout that ignores the corner
        // ahead is the one detail that makes a mock dashboard feel like a mock.
        //
        // How early depends on how fast, which is the whole of what makes it look driven: the
        // distance is the one that brings this speed down to a corner speed at a rate a passenger
        // would not comment on, so eighty on a boulevard starts easing off a long way out and
        // thirty in a side street barely lifts at all. A fixed distance gives a car that either
        // dawdles into every junction or arrives at it still doing the limit.
        //
        // And the traffic is in the pace, not just in the arrival time: a jam nobody has to drive
        // through is an estimate rather than a jam, and the speedometer is where one is felt.
        let allowed = route.leg(self.travelled).map_or(Class::Residential.speed(), Leg::pace);
        let lift = (self.speed * self.speed - CORNER * CORNER) / (2.0 * BRAKE);
        let target = if to_turn < lift.max(30.0) { allowed.min(CORNER) } else { allowed };
        // A rate in metres a second a second rather than a fraction of the gap closed per frame: the
        // exponential ease this used to be crossed fifteen metres a second in about a third of one,
        // which is a needle that jumps rather than falls, and it made the braking distance above a
        // number with nothing behind it. Slowing down is allowed to be firmer than speeding up.
        let rate = if target < self.speed { BRAKE } else { ACCEL };
        self.speed += (target - self.speed).clamp(-rate * dt, rate * dt);

        let step = self.speed * dt;
        self.travelled += step;
        self.trip += step / 1000.0;
        self.fuel = (self.fuel - step / 1000.0 / RANGE).max(0.0);

        let (at, heading) = route.locate(self.travelled);
        self.at = at;
        // Turn the view towards the new heading rather than snapping, or the map flicks sideways
        // at every junction.
        let mut delta = heading - self.heading;
        while delta > std::f32::consts::PI {
            delta -= std::f32::consts::TAU;
        }
        while delta < -std::f32::consts::PI {
            delta += std::f32::consts::TAU;
        }
        self.heading += delta * (dt * 4.0).min(1.0);

        if self.travelled >= route.length() {
            self.status = format!("arrived at {}", route.destination);
            self.settling += dt;
            self.speed = 0.0;
            if self.settling > 4.0 {
                self.settling = 0.0;
                let next = self.next_destination();
                self.drive_to(next);
            }
        }
    }

    /// The nearest point of interest that is not the one just reached, so the demo keeps driving.
    fn next_destination(&self) -> usize {
        let here = self.at;
        self.world
            .pois
            .iter()
            .enumerate()
            .filter(|(_, poi)| distance(poi.at, here) > 400.0)
            .min_by(|(ai, a), (bi, b)| {
                // Favour a different third of the list each time, or the car would shuttle between
                // the same two nearest stops for ever.
                let turn = self.clock as usize / 11;
                let bias = |index: usize, at: (f32, f32)| {
                    distance(at, here) * if (index + turn) % 3 == 0 { 0.45 } else { 1.0 }
                };
                bias(*ai, a.at).total_cmp(&bias(*bi, b.at))
            })
            .map_or(0, |(index, _)| index)
    }

    pub fn eta(&self) -> f32 {
        self.route.as_ref().map_or(0.0, |route| route.remaining_time(self.travelled))
    }

    pub fn remaining(&self) -> f32 {
        self.route.as_ref().map_or(0.0, |route| (route.length() - self.travelled).max(0.0))
    }

    /// How much longer the rest of the route will take than an empty city would. `1.0` is clear.
    pub fn flow(&self) -> f32 {
        self.route.as_ref().map_or(1.0, |route| route.flow(self.travelled))
    }

    /// The worst traffic still ahead and the street it is on, which is what the rail says out loud.
    pub fn named_worst(&self) -> Option<(&'static str, f32)> {
        self.route.as_ref()?.named_worst(self.travelled)
    }

    /// The road this route mostly runs along from here, which is how a driver names a journey.
    pub fn via(&self) -> Option<&'static str> {
        self.route.as_ref()?.via(self.travelled)
    }

    /// Minutes the traffic ahead is adding, or nothing if it is adding none worth a line.
    ///
    /// Derived from the whole of what is left rather than from its worst leg, because minutes lost are
    /// a total — and rounded, because a driver cannot act on seconds.
    pub fn lost(&self) -> Option<u32> {
        let flow = self.flow();
        let lost = (self.eta() * (1.0 - 1.0 / flow.max(0.01)) / 60.0).round();
        (lost >= 1.0).then_some(lost as u32)
    }

    pub fn class(&self) -> Class {
        self.route.as_ref().map_or(Class::Residential, |route| route.street(self.travelled).1)
    }

    pub fn street(&self) -> &'static str {
        self.route.as_ref().map_or("", |route| route.street(self.travelled).0)
    }

    pub fn key(&mut self, key: KeyEvent) {
        if self.search.is_some() {
            self.search_key(key);
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Escape => self.quit = true,
            KeyCode::Char('c') if key.modifiers.contains(Modifiers::CTRL) => self.quit = true,
            KeyCode::Char('s') | KeyCode::Char('/') => {
                self.search =
                    Some(Search { text: String::new(), hits: self.matches(""), cursor: 0 });
            }
            KeyCode::Char('n') => {
                self.north_up = !self.north_up;
                self.status = if self.north_up { "north up".into() } else { "heading up".into() };
            }
            KeyCode::Char('p') | KeyCode::Char(' ') => {
                self.paused = !self.paused;
                self.status = if self.paused { "paused".into() } else { "driving".into() };
            }
            KeyCode::Char('+') | KeyCode::Char('=') | KeyCode::Up => {
                self.metres_across = (self.metres_across / 1.35).max(180.0);
            }
            KeyCode::Char('-') | KeyCode::Char('_') | KeyCode::Down => {
                self.metres_across = (self.metres_across * 1.35).min(5200.0);
            }
            KeyCode::Char('r') => {
                let next = self.next_destination();
                self.drive_to(next);
            }
            // The transport, on the three keys that are already shaped like it.
            KeyCode::Char('<') | KeyCode::Char(',') => self.player.skip(-1),
            KeyCode::Char('>') | KeyCode::Char('.') => self.player.skip(1),
            KeyCode::Char('m') => {
                self.player.toggle();
                let track = self.player.track();
                self.status = if self.player.playing {
                    format!("{} · {}", track.title, track.artist)
                } else {
                    "paused the music".into()
                };
            }
            _ => {}
        }
    }

    fn search_key(&mut self, key: KeyEvent) {
        let Some(search) = self.search.as_ref() else {
            return;
        };
        match key.code {
            KeyCode::Escape => self.search = None,
            KeyCode::Backspace => {
                let mut text = search.text.clone();
                text.pop();
                self.refine(text);
            }
            KeyCode::Char(character) => {
                let text = format!("{}{character}", search.text);
                self.refine(text);
            }
            KeyCode::Up => {
                if let Some(search) = self.search.as_mut() {
                    search.cursor = search.cursor.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some(search) = self.search.as_mut() {
                    search.cursor = (search.cursor + 1).min(search.hits.len().saturating_sub(1));
                }
            }
            KeyCode::Enter => {
                let chosen = search.hits.get(search.cursor).copied();
                self.search = None;
                if let Some(poi) = chosen {
                    self.drive_to(poi);
                }
            }
            _ => {}
        }
    }

    /// A new query, and the list it produces, from the top.
    fn refine(&mut self, text: String) {
        let hits = self.matches(&text);
        self.search = Some(Search { text, hits, cursor: 0 });
    }

    /// Points of interest matching `query`, nearest first, by name or by kind.
    pub fn matches(&self, query: &str) -> Vec<usize> {
        let needle = fold(query);
        let mut hits: Vec<usize> = self
            .world
            .pois
            .iter()
            .enumerate()
            .filter(|(_, poi)| {
                needle.is_empty()
                    || fold(poi.name).contains(&needle)
                    || fold(poi.kind).contains(&needle)
            })
            .map(|(index, _)| index)
            .collect();
        let here = self.at;
        hits.sort_by(|&a, &b| {
            distance(self.world.pois[a].at, here).total_cmp(&distance(self.world.pois[b].at, here))
        });
        hits
    }
}

/// Lower-case, and accents flattened, so `cafe` finds `Café des Sports`.
fn fold(text: &str) -> String {
    text.chars()
        .flat_map(|character| character.to_lowercase())
        .map(|character| match character {
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'à' | 'â' | 'ä' => 'a',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'û' | 'ù' | 'ü' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

/// Wall-clock time as a driver reads it: minutes from the start of the day plus the time to go.
pub fn clock_text(minutes_of_day: f32) -> String {
    let minutes = minutes_of_day.rem_euclid(1440.0) as u32;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The drive has to have both ends of the speedometer in it, or the readout is a static picture:
    /// a clear boulevard gets up to eighty, a junction is taken at a quarter of that, and the change
    /// between the two is at a rate a passenger would not comment on.
    #[test]
    fn the_car_gets_up_to_speed_and_slows_for_the_corners() {
        let mut dash = Dash::new();
        let (mut fastest, mut slowest, mut jerk) = (0.0f32, f32::MAX, 0.0f32);
        let mut was = 0.0;
        for _ in 0..60 * 120 {
            dash.tick(1.0 / 60.0);
            let kmh = dash.speed * 3.6;
            fastest = fastest.max(kmh);
            // Only once under way: the first seconds and the arrival are both legitimately slow.
            if dash.travelled > 200.0 && dash.remaining() > 200.0 {
                slowest = slowest.min(kmh);
            }
            jerk = jerk.max((dash.speed - was).abs() * 60.0);
            was = dash.speed;
        }
        assert!(fastest > 70.0, "the car never got going: {fastest:.0} km/h");
        assert!(slowest < 35.0, "the car never slowed for a corner: {slowest:.0} km/h");
        assert!(jerk <= BRAKE + 0.01, "that is not braking, it is a step change: {jerk:.1} m/s²");
    }

    /// Three states, and the boundaries between them are the whole readout: amber before the limit is
    /// broken rather than after, and red at one over rather than at four.
    #[test]
    fn the_speedometer_goes_amber_before_it_goes_red() {
        use crate::hud::{Gauge, Watch};

        assert_eq!(Gauge::reading(60, 70).watch(), Watch::Clear);
        assert_eq!(Gauge::reading(68, 70).watch(), Watch::Clear, "two per cent of 70 is 1.4 km/h");
        assert_eq!(Gauge::reading(69, 70).watch(), Watch::Close);
        assert_eq!(Gauge::reading(70, 70).watch(), Watch::Close, "at the limit is not over it");
        assert_eq!(Gauge::reading(71, 70).watch(), Watch::Over);
        assert_eq!(Gauge::reading(71, 70).over_by(), None, "one over is the readout's own error");
        assert_eq!(Gauge::reading(78, 70).over_by(), Some(8), "eight over is a driver's decision");
    }
}

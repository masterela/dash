//! A* over the street graph, and the turn list that falls out of the path.
//!
//! Cost is travel time rather than distance, which is why a route will happily run an extra block
//! to reach a boulevard — the same reason a real router does it. The heuristic is straight-line
//! distance at the fastest class in the graph, so it never overestimates and the path is optimal.

use std::collections::BinaryHeap;

use crate::world::{Class, World, distance};

pub struct Route {
    /// Junction positions, start to destination.
    pub points: Vec<(f32, f32)>,
    /// Cumulative distance to each point, so a position along the route is one lookup.
    pub along: Vec<f32>,
    pub legs: Vec<Leg>,
    pub maneuvers: Vec<Maneuver>,
    pub destination: String,
}

/// One stretch of one street, between two turns.
pub struct Leg {
    pub street: &'static str,
    pub class: Class,
    /// Index into `points` where this leg starts.
    pub from: usize,
}

impl Leg {
    /// How badly this leg is running. See [`crate::world::jam`].
    pub fn jam(&self) -> f32 {
        crate::world::jam(self.street, self.class)
    }

    /// Metres a second this leg actually allows, traffic included.
    pub fn pace(&self) -> f32 {
        self.class.speed() / self.jam()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Turn {
    Left,
    Right,
    Straight,
    Arrive,
}

impl Turn {
    pub fn arrow(self) -> &'static str {
        match self {
            Turn::Left => "↰",
            Turn::Right => "↱",
            Turn::Straight => "↑",
            Turn::Arrive => "◆",
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Turn::Left => "turn left onto",
            Turn::Right => "turn right onto",
            Turn::Straight => "continue on",
            Turn::Arrive => "arrive at",
        }
    }
}

pub struct Maneuver {
    pub turn: Turn,
    /// Distance from the start of the route, in metres.
    pub at: f32,
    /// The street being joined, or the destination for the last one.
    pub onto: String,
    /// Which lanes to be in, innermost first. `true` means it goes the way we do.
    pub lanes: Vec<bool>,
}

#[derive(PartialEq)]
struct Step(f32, usize);

impl Eq for Step {}

impl Ord for Step {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // A min-heap out of a max-heap, and NaN-free because every cost comes from a finite add.
        other.0.total_cmp(&self.0)
    }
}

impl PartialOrd for Step {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Route {
    /// The quickest way from one junction to another, or `None` if the graph does not connect them.
    pub fn find(world: &World, from: usize, to: usize, destination: &str) -> Option<Route> {
        let count = world.down.len() * world.across.len();
        let mut best = vec![f32::INFINITY; count];
        let mut came = vec![usize::MAX; count];
        let mut queue = BinaryHeap::new();
        let target = world.position(to);
        let fastest = Class::Motorway.speed();

        best[from] = 0.0;
        queue.push(Step(0.0, from));

        while let Some(Step(_, node)) = queue.pop() {
            if node == to {
                break;
            }
            for (next, street) in world.neighbours(node) {
                // Costed at what the street is actually doing, not at what its signs say, which is
                // the whole difference between a router with traffic and one without.
                let hop = distance(world.position(node), world.position(next)) / street.pace();
                let cost = best[node] + hop;
                if cost < best[next] {
                    best[next] = cost;
                    came[next] = node;
                    let guess = distance(world.position(next), target) / fastest;
                    queue.push(Step(cost + guess, next));
                }
            }
        }

        if best[to].is_infinite() {
            return None;
        }

        let mut nodes = vec![to];
        while let Some(&previous) = came.get(*nodes.last().expect("a path has an end")) {
            if previous == usize::MAX {
                break;
            }
            nodes.push(previous);
        }
        nodes.reverse();

        Some(Route::describe(world, &nodes, destination))
    }

    fn describe(world: &World, nodes: &[usize], destination: &str) -> Route {
        let points: Vec<(f32, f32)> = nodes.iter().map(|&node| world.position(node)).collect();

        let mut along = Vec::with_capacity(points.len());
        let mut total = 0.0;
        for (index, point) in points.iter().enumerate() {
            if index > 0 {
                total += distance(points[index - 1], *point);
            }
            along.push(total);
        }

        // Which street each hop runs along, taken from the pair of nodes rather than guessed.
        let streets: Vec<(&'static str, Class)> = nodes
            .windows(2)
            .map(|pair| {
                let (ai, aj) = world.node_at(pair[0]);
                let (bi, _) = world.node_at(pair[1]);
                if ai == bi {
                    (world.down[ai].name, world.down[ai].class)
                } else {
                    (world.across[aj].name, world.across[aj].class)
                }
            })
            .collect();

        let mut legs = Vec::new();
        let mut maneuvers = Vec::new();
        for (hop, &(street, class)) in streets.iter().enumerate() {
            if legs.last().is_none_or(|leg: &Leg| leg.street != street) {
                legs.push(Leg { street, class, from: hop });
                if hop > 0 {
                    let turn = bearing_change(points[hop - 1], points[hop], points[hop + 1]);
                    maneuvers.push(Maneuver {
                        turn,
                        at: along[hop],
                        onto: street.to_string(),
                        lanes: lanes_for(turn, class),
                    });
                }
            }
        }
        maneuvers.push(Maneuver {
            turn: Turn::Arrive,
            at: total,
            onto: destination.to_string(),
            lanes: Vec::new(),
        });

        Route { points, along, legs, maneuvers, destination: destination.to_string() }
    }

    pub fn length(&self) -> f32 {
        self.along.last().copied().unwrap_or(0.0)
    }

    /// Seconds still to drive, at the speed each remaining street is actually doing.
    ///
    /// The same sum A* minimised, which is why the arrival time a driver is shown agrees with the
    /// reason the route was chosen.
    pub fn remaining_time(&self, travelled: f32) -> f32 {
        self.remaining_times(travelled).0
    }

    /// How much longer the rest of the route will take than an empty city would allow.
    ///
    /// `1.0` is clear. Expressed as a ratio rather than as minutes lost because a ratio is the thing
    /// that can be given a colour: a driver wants to know whether it is bad, not by how many seconds.
    pub fn flow(&self, travelled: f32) -> f32 {
        let (real, free) = self.remaining_times(travelled);
        if free < 1.0 { 1.0 } else { real / free }
    }

    /// Every manoeuvre still to come, in order, including the arrival.
    ///
    /// The same window [`Self::next_maneuver`] uses, so that the first of these is always the one being
    /// driven towards: anything else and a list that skips the current turn would skip two of them for
    /// the few metres the two disagreed over.
    pub fn to_come(&self, travelled: f32) -> impl Iterator<Item = &Maneuver> {
        self.maneuvers.iter().filter(move |turn| turn.at > travelled - 8.0)
    }

    /// The road a driver would name if asked which way this goes.
    ///
    /// The street with the most of the drive still on it rather than the next one: "via" answers
    /// "which way", and nobody answers that with the side street they are about to leave.
    pub fn via(&self, travelled: f32) -> Option<&'static str> {
        self.ahead(travelled).into_iter().max_by(|a, b| a.2.total_cmp(&b.2)).map(|(name, ..)| name)
    }

    /// The worst jam still to be driven through, and the street it is on.
    ///
    /// The worst and not the average, which is what [`Self::flow`] gives: a route that is clear for ten
    /// kilometres and stopped for the last two averages out to "steady", and the two stopped kilometres
    /// are the entire news. It is graded on the same scale as [`crate::world::jam`], so the colour the
    /// rail gives it is the colour the map drew that stretch in — the map shows where, the rail says
    /// that there is a where.
    ///
    /// And it comes with the name, because a driver told "stopped" has a fact while a driver told
    /// "stopped, Bd Voltaire" has a decision. Whether they know a way round it is theirs to know.
    pub fn named_worst(&self, travelled: f32) -> Option<(&'static str, f32)> {
        self.ahead(travelled)
            .into_iter()
            .map(|(name, class, _)| (name, crate::world::jam(name, class)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Every street still ahead, with the metres left on each — one entry per street, not per hop,
    /// because a route that crosses back onto a boulevard is still one road as far as a driver is
    /// concerned.
    fn ahead(&self, travelled: f32) -> Vec<(&'static str, Class, f32)> {
        let mut rest: Vec<(&'static str, Class, f32)> = Vec::new();
        for hop in 0..self.points.len().saturating_sub(1) {
            let (from, to) = (self.along[hop], self.along[hop + 1]);
            let left = to - travelled.clamp(from, to);
            if left <= 0.0 {
                continue;
            }
            let Some(leg) = self.legs.iter().rev().find(|leg| leg.from <= hop) else { continue };
            match rest.iter_mut().find(|(name, ..)| *name == leg.street) {
                Some(entry) => entry.2 += left,
                None => rest.push((leg.street, leg.class, left)),
            }
        }
        rest
    }

    /// Seconds left with the traffic, and seconds left without it, from one walk of the hops.
    fn remaining_times(&self, travelled: f32) -> (f32, f32) {
        let (mut real, mut free) = (0.0, 0.0);
        for hop in 0..self.points.len().saturating_sub(1) {
            let (from, to) = (self.along[hop], self.along[hop + 1]);
            let left = to - travelled.clamp(from, to);
            if left > 0.0 {
                match self.legs.iter().rev().find(|leg| leg.from <= hop) {
                    Some(leg) => {
                        real += left / leg.pace();
                        free += left / leg.class.speed();
                    }
                    None => {
                        real += left / Class::Residential.speed();
                        free += left / Class::Residential.speed();
                    }
                }
            }
        }
        (real, free)
    }

    /// The leg being driven, for anything that needs more of it than its name and class.
    pub fn leg(&self, travelled: f32) -> Option<&Leg> {
        let hop = self.along.iter().rposition(|&reached| reached <= travelled).unwrap_or(0);
        self.legs.iter().rev().find(|leg| leg.from <= hop)
    }

    /// Where a car that has driven `travelled` metres is, and which way it is pointing.
    pub fn locate(&self, travelled: f32) -> ((f32, f32), f32) {
        let hop = self
            .along
            .iter()
            .rposition(|&reached| reached <= travelled)
            .unwrap_or(0)
            .min(self.points.len().saturating_sub(2));
        let (a, b) = (self.points[hop], self.points[hop + 1]);
        let span = (self.along[hop + 1] - self.along[hop]).max(0.001);
        let t = ((travelled - self.along[hop]) / span).clamp(0.0, 1.0);
        let at = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        (at, (b.1 - a.1).atan2(b.0 - a.0))
    }

    /// The street being driven at `travelled` metres, and its class.
    pub fn street(&self, travelled: f32) -> (&'static str, Class) {
        let hop = self.along.iter().rposition(|&reached| reached <= travelled).unwrap_or(0);
        let leg = self.legs.iter().rev().find(|leg| leg.from <= hop);
        leg.map_or(("", Class::Residential), |leg| (leg.street, leg.class))
    }

    /// The next thing the driver has to do.
    pub fn next_maneuver(&self, travelled: f32) -> &Maneuver {
        self.maneuvers
            .iter()
            .find(|maneuver| maneuver.at > travelled - 8.0)
            .unwrap_or_else(|| self.maneuvers.last().expect("a route arrives somewhere"))
    }

    pub fn after_next(&self, travelled: f32) -> Option<&Maneuver> {
        let mut upcoming = self.maneuvers.iter().filter(|maneuver| maneuver.at > travelled - 8.0);
        upcoming.next();
        upcoming.next()
    }
}

/// Left, right or straight on, from the cross product of the two bearings.
fn bearing_change(before: (f32, f32), at: (f32, f32), after: (f32, f32)) -> Turn {
    let (ax, ay) = (at.0 - before.0, at.1 - before.1);
    let (bx, by) = (after.0 - at.0, after.1 - at.1);
    let cross = ax * by - ay * bx;
    let scale = (ax * ax + ay * ay).sqrt() * (bx * bx + by * by).sqrt();
    // Screen y runs downwards, so a positive cross product turns right on screen.
    match cross / scale.max(0.001) {
        turn if turn > 0.25 => Turn::Right,
        turn if turn < -0.25 => Turn::Left,
        _ => Turn::Straight,
    }
}

/// A plausible lane layout: wider streets have more of them, and a turn uses the near side.
fn lanes_for(turn: Turn, class: Class) -> Vec<bool> {
    let count = match class {
        Class::Residential => 2,
        Class::Primary => 3,
        Class::Motorway => 4,
    };
    (0..count)
        .map(|lane| match turn {
            Turn::Left => lane == 0,
            Turn::Right => lane + 1 == count,
            _ => true,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Street;

    /// A route across the city has to turn, has to end in an arrival, and has to be in order.
    #[test]
    fn a_route_across_town_reads_as_directions() {
        let world = World::generate();
        let route = Route::find(&world, world.node(0, 0), world.node(7, 6), "Gare de Lyon")
            .expect("the grid connects every junction to every other");

        assert!(route.points.len() > 8, "{} junctions is not a crossing", route.points.len());
        assert!(route.maneuvers.len() > 1, "a crossing of the grid involves a turn");
        assert_eq!(route.maneuvers.last().map(|last| last.turn), Some(Turn::Arrive));
        assert!(
            route.maneuvers.windows(2).all(|pair| pair[0].at <= pair[1].at),
            "turns must be given in the order they are driven"
        );
        assert!(route.along.windows(2).all(|pair| pair[0] < pair[1]));
    }

    /// Time to go only ever falls, and reaches zero exactly at the destination.
    #[test]
    fn the_time_left_runs_down_to_nothing() {
        let world = World::generate();
        let route = Route::find(&world, world.node(1, 1), world.node(5, 5), "Parking Bastille")
            .expect("connected");

        let mut last = f32::INFINITY;
        let mut travelled = 0.0;
        while travelled < route.length() {
            let left = route.remaining_time(travelled);
            assert!(left <= last + 0.01, "time to go went up at {travelled} m");
            last = left;
            travelled += 25.0;
        }
        assert!(route.remaining_time(route.length()) < 0.01);
    }

    /// Costing by time is the whole reason a router exists; a slower way must never be chosen.
    ///
    /// The baseline is the same journey crawled through the side streets at the pace they are really
    /// doing, because the router costs with the traffic and so must the thing it has to beat. On a
    /// grid every path that does not double back is the same length, so the distance cancels out and
    /// what is left being compared is the choice of classes, which is the choice under test.
    #[test]
    fn the_quick_way_beats_the_short_way() {
        let world = World::generate();
        let route = Route::find(&world, world.node(0, 0), world.node(8, 7), "far corner")
            .expect("connected");

        let side: Vec<f32> = world
            .down
            .iter()
            .chain(&world.across)
            .filter(|street| street.class == Class::Residential)
            .map(Street::pace)
            .collect();
        let pace = side.iter().sum::<f32>() / side.len() as f32;
        let crawl = route.length() / pace;
        let time = route.remaining_time(0.0);
        assert!(time < crawl, "{time:.0} s is no better than {crawl:.0} s down the side streets");
    }

    /// What the rail says about the traffic has to be what the map draws, and it has to stop saying it
    /// once the jam has been driven through — otherwise a warning outlives the thing it warned about.
    #[test]
    fn the_worst_jam_ahead_is_the_one_the_rail_reports() {
        let world = World::generate();
        let route = Route::find(&world, world.node(0, 0), world.node(8, 7), "far corner")
            .expect("connected");

        let jam = |travelled: f32| route.named_worst(travelled).map_or(1.0, |(_, jam)| jam);
        let worst = route.legs.iter().map(Leg::jam).fold(1.0, f32::max);
        assert_eq!(jam(0.0), worst, "at the start, everything is still ahead");
        // The average cannot see a single bad stretch, which is exactly why the colour comes from this.
        assert!(jam(0.0) >= route.flow(0.0));
        // The street it names has to be the street that badly off, or the rail sends them past it.
        let (street, named) = route.named_worst(0.0).expect("a route has legs");
        assert_eq!(named, worst, "{street} is not the worst of it");

        // And it never rises as the drive goes on: a leg driven is a leg that can no longer be news.
        let mut last = jam(0.0);
        let mut travelled = 0.0;
        while travelled < route.length() {
            let now = jam(travelled);
            assert!(now <= last + 0.001, "the traffic got worse behind us at {travelled} m");
            last = now;
            travelled += 50.0;
        }
    }

    /// "via" has to name the road the drive is mostly on, which is not usually the one under the wheels.
    #[test]
    fn the_road_a_journey_is_named_after_is_the_long_one() {
        let world = World::generate();
        let route = Route::find(&world, world.node(0, 0), world.node(8, 7), "far corner")
            .expect("connected");

        let via = route.via(0.0).expect("a route has legs");
        let longest = route
            .legs
            .iter()
            .map(|leg| {
                let until = route.legs.iter().find(|next| next.from > leg.from);
                let to = until.map_or(route.length(), |next| route.along[next.from]);
                (leg.street, to - route.along[leg.from])
            })
            .fold(("", 0.0), |best, leg| if leg.1 > best.1 { leg } else { best });
        assert_eq!(via, longest.0, "the journey is named after a street it barely touches");
    }
}

//! A city, generated once, in metres.
//!
//! Stand-in for an OSM extract: the shapes a renderer has to deal with (named ways of several
//! classes, water, green, building footprints, points of interest) and a routable graph over them.
//! Streets are straight lines with a per-street slope rather than a perfect chessboard, so every
//! intersection is one line-line solve and the graph needs no tolerance fudging.

/// How fast traffic moves on a class of way, in metres per second, and what it is drawn as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Residential,
    Primary,
    Motorway,
}

impl Class {
    /// What traffic actually does here on a clear run, which is not what the sign says.
    ///
    /// A little over the limit on every class, because that is what a city drives like and because a
    /// speedometer that can never go amber or red is a speedometer with two colours nobody has seen.
    /// The three states are worth more than the flattery of a car that always obeys: what is being
    /// demonstrated is a readout, and a readout is judged on the reading it gives when something is
    /// wrong. The jam factor divides this, so most streets come out under their limit anyway.
    pub fn speed(self) -> f32 {
        match self {
            Class::Residential => 15.0,
            Class::Primary => 22.2,
            Class::Motorway => 33.3,
        }
    }

    /// Metres of carriageway, which is also how wide it gets drawn.
    pub fn width(self) -> f32 {
        match self {
            Class::Residential => 7.0,
            Class::Primary => 14.0,
            Class::Motorway => 22.0,
        }
    }

    pub fn limit(self) -> u32 {
        match self {
            Class::Residential => 50,
            Class::Primary => 70,
            Class::Motorway => 110,
        }
    }
}

/// How badly a street is running, as a multiplier on the time it takes to drive.
///
/// `1.0` is free-flowing; the worst a class can get is a crawl. There is no live feed behind this
/// and in a generated city there could not be, so it comes from a hash of the street's own name:
/// stable for a whole drive, identical on every run, and different for every street — which are the
/// three properties that let a route be coloured by it and an arrival time be derived from it.
///
/// Squaring the share is the one thing here taken from life: most streets are nearly clear and a few
/// are genuinely bad, rather than the whole city being mildly slow.
pub fn jam(name: &str, class: Class) -> f32 {
    let mut hash = 0x811c_9dc5u32;
    for &byte in name.as_bytes() {
        hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
    }
    let share = (hash >> 8) as f32 / (1u32 << 24) as f32;
    // Bigger roads carry more of the city, so they are where it stops moving.
    let worst = match class {
        Class::Residential => 1.35,
        Class::Primary => 2.30,
        Class::Motorway => 2.70,
    };
    1.0 + share * share * (worst - 1.0)
}

/// Where a delay stops being one and starts being worth a colour.
///
/// Named once, because the route on the map and the word in the rail have to agree about red.
pub const BUSY: f32 = 1.25;
pub const STOPPED: f32 = 1.65;

/// A straight way. Vertical ones are `x = offset + slope * y`, horizontal the other way round.
pub struct Street {
    pub name: &'static str,
    pub class: Class,
    pub offset: f32,
    pub slope: f32,
}

impl Street {
    /// How badly this street is running. See [`jam`].
    pub fn jam(&self) -> f32 {
        jam(self.name, self.class)
    }

    /// Metres a second actually achievable here, which is the speed a router should cost with.
    pub fn pace(&self) -> f32 {
        self.class.speed() / self.jam()
    }
}

pub struct Poi {
    pub name: &'static str,
    pub kind: &'static str,
    pub at: (f32, f32),
}

pub struct World {
    pub width: f32,
    pub height: f32,
    /// Vertical streets, west to east.
    pub down: Vec<Street>,
    /// Horizontal streets, north to south.
    pub across: Vec<Street>,
    /// Scenery only, and deliberately so: a motorway you cannot turn onto from a side street is
    /// the honest shape of one, and it keeps the routing graph to the grid.
    pub motorway: Vec<(f32, f32)>,
    pub river: Vec<(f32, f32)>,
    pub parks: Vec<(f32, f32, f32, f32)>,
    pub buildings: Vec<(f32, f32, f32, f32)>,
    pub pois: Vec<Poi>,
}

/// Vertical streets, west to east. Roughly one boulevard every third street, which is about the
/// ratio a real city has and the reason a rendered map has a visible hierarchy at all.
const DOWN_NAMES: [(&str, Class); 18] = [
    ("Rue du Bac", Class::Residential),
    ("Bd Saint-Michel", Class::Primary),
    ("Rue de Sèvres", Class::Residential),
    ("Rue de Rennes", Class::Primary),
    ("Rue Saint-Honoré", Class::Residential),
    ("Av. de l'Opéra", Class::Primary),
    ("Rue Montorgueil", Class::Residential),
    ("Bd Sébastopol", Class::Primary),
    ("Rue du Temple", Class::Residential),
    ("Rue Oberkampf", Class::Residential),
    ("Bd Magenta", Class::Primary),
    ("Rue de la Roquette", Class::Residential),
    ("Rue de Charonne", Class::Residential),
    ("Av. Ledru-Rollin", Class::Residential),
    ("Bd Voltaire", Class::Primary),
    ("Rue Crozatier", Class::Residential),
    ("Av. Daumesnil", Class::Primary),
    ("Rue de Reuilly", Class::Residential),
];

const ACROSS_NAMES: [(&str, Class); 14] = [
    ("Rue Lafayette", Class::Residential),
    ("Bd Haussmann", Class::Primary),
    ("Rue Saint-Lazare", Class::Residential),
    ("Rue de Provence", Class::Residential),
    ("Rue de Rivoli", Class::Primary),
    ("Quai de la Mégisserie", Class::Residential),
    ("Quai d'Orsay", Class::Residential),
    ("Bd Saint-Germain", Class::Primary),
    ("Rue de Vaugirard", Class::Residential),
    ("Av. Montaigne", Class::Residential),
    ("Bd de Grenelle", Class::Primary),
    ("Rue de Tolbiac", Class::Residential),
    ("Av. d'Italie", Class::Primary),
    ("Rue de Charenton", Class::Residential),
];

const POI_NAMES: [(&str, &str); 16] = [
    ("Pharmacie du Marché", "pharmacy"),
    ("Pharmacie Centrale", "pharmacy"),
    ("Pharmacie de la Gare", "pharmacy"),
    ("Café des Sports", "cafe"),
    ("Boulangerie Julien", "bakery"),
    ("Parking Concorde", "parking"),
    ("Parking Bastille", "parking"),
    ("Hôpital Saint-Louis", "hospital"),
    ("Musée d'Orsay", "museum"),
    ("Gare de Lyon", "station"),
    ("Total Access", "fuel"),
    ("Station Avia", "fuel"),
    ("Carrefour City", "shop"),
    ("Marché d'Aligre", "market"),
    ("Piscine Pontoise", "pool"),
    ("Bibliothèque Arsenal", "library"),
];

/// How wide the river is, in metres. Wide enough that a bridge is a landmark.
pub const RIVER: f32 = 190.0;

/// Deterministic noise, so the city is the same on every run and on every machine.
fn wobble(seed: u32) -> f32 {
    let mut value = seed.wrapping_mul(2_654_435_761).wrapping_add(12_345);
    value ^= value >> 15;
    value = value.wrapping_mul(2_246_822_519);
    value ^= value >> 13;
    (value % 2000) as f32 / 1000.0 - 1.0
}

impl World {
    pub fn generate() -> World {
        let (width, height) = (4200.0, 3000.0);

        let down = DOWN_NAMES
            .iter()
            .enumerate()
            .map(|(index, (name, class))| Street {
                name,
                class: *class,
                offset: 150.0 + index as f32 * 230.0 + wobble(index as u32 * 7) * 45.0,
                slope: wobble(index as u32 * 13) * 0.10,
            })
            .collect();

        let across = ACROSS_NAMES
            .iter()
            .enumerate()
            .map(|(index, (name, class))| Street {
                name,
                class: *class,
                offset: 140.0 + index as f32 * 208.0 + wobble(index as u32 * 11 + 3) * 40.0,
                slope: wobble(index as u32 * 17 + 5) * 0.08,
            })
            .collect();

        let river: Vec<(f32, f32)> = (0..=28)
            .map(|step| {
                let x = step as f32 / 28.0 * width;
                (x, 1850.0 + (x / 620.0).sin() * 210.0 + x * 0.045)
            })
            .collect();

        let motorway = vec![
            (-200.0, 3100.0),
            (900.0, 2250.0),
            (2100.0, 1250.0),
            (2900.0, 520.0),
            (3400.0, -150.0),
        ];

        let parks = vec![(1500.0, 620.0, 620.0, 430.0), (3050.0, 2200.0, 700.0, 380.0)];

        let mut buildings = Vec::new();
        // Enough footprints that a block looks built up rather than sprinkled. They are drawn under
        // the roads, so one straddling a street costs nothing.
        for index in 0..460u32 {
            let x = 120.0 + (wobble(index * 3 + 1) * 0.5 + 0.5) * (width - 400.0);
            let y = 120.0 + (wobble(index * 5 + 2) * 0.5 + 0.5) * (height - 400.0);
            let w = 45.0 + (wobble(index * 7 + 4) * 0.5 + 0.5) * 115.0;
            let h = 40.0 + (wobble(index * 11 + 6) * 0.5 + 0.5) * 85.0;
            // Nothing is built in the river. Left in, it reads as a smear rather than as water.
            let middle = (x + w * 0.5, y + h * 0.5);
            let wet = river
                .windows(2)
                .map(|pair| point_to_segment(middle, pair[0], pair[1]))
                .fold(f32::INFINITY, f32::min);
            if wet > RIVER * 0.5 + w.max(h) * 0.5 {
                buildings.push((x, y, w, h));
            }
        }

        let world = World {
            width,
            height,
            down,
            across,
            motorway,
            river,
            parks,
            buildings,
            pois: Vec::new(),
        };

        // Every POI sits on a junction, so "drive to it" means "drive to a node" and the demo
        // never has to fake the last hundred metres.
        let junctions = world.junctions();
        let pois = POI_NAMES
            .iter()
            .enumerate()
            .map(|(index, (name, kind))| {
                let pick =
                    (wobble(index as u32 * 23 + 9) * 0.5 + 0.5) * (junctions.len() - 1) as f32;
                Poi { name, kind, at: junctions[pick as usize] }
            })
            .collect();

        World { pois, ..world }
    }

    /// Where vertical street `i` crosses horizontal street `j`.
    pub fn junction(&self, i: usize, j: usize) -> (f32, f32) {
        let (down, across) = (&self.down[i], &self.across[j]);
        // x = a + s*y and y = b + t*x, solved for the pair.
        let y = (across.offset + across.slope * down.offset) / (1.0 - across.slope * down.slope);
        let x = down.offset + down.slope * y;
        (x, y)
    }

    pub fn junctions(&self) -> Vec<(f32, f32)> {
        (0..self.down.len())
            .flat_map(|i| (0..self.across.len()).map(move |j| (i, j)))
            .map(|(i, j)| self.junction(i, j))
            .collect()
    }

    pub fn node(&self, i: usize, j: usize) -> usize {
        i * self.across.len() + j
    }

    pub fn node_at(&self, node: usize) -> (usize, usize) {
        (node / self.across.len(), node % self.across.len())
    }

    pub fn position(&self, node: usize) -> (f32, f32) {
        let (i, j) = self.node_at(node);
        self.junction(i, j)
    }

    /// The four neighbours of a junction, with the street each one runs along.
    pub fn neighbours(&self, node: usize) -> Vec<(usize, &Street)> {
        let (i, j) = self.node_at(node);
        let mut out = Vec::with_capacity(4);
        if j > 0 {
            out.push((self.node(i, j - 1), &self.down[i]));
        }
        if j + 1 < self.across.len() {
            out.push((self.node(i, j + 1), &self.down[i]));
        }
        if i > 0 {
            out.push((self.node(i - 1, j), &self.across[j]));
        }
        if i + 1 < self.down.len() {
            out.push((self.node(i + 1, j), &self.across[j]));
        }
        out
    }

    /// Distance from a point to the middle of the river, which is what decides what is water.
    ///
    /// Both renderers ask this, and so does the generator when it declines to put a building in the
    /// water, so the shoreline is in one place rather than three.
    pub fn river_distance(&self, at: (f32, f32)) -> f32 {
        self.river
            .windows(2)
            .map(|pair| point_to_segment(at, pair[0], pair[1]))
            .fold(f32::INFINITY, f32::min)
    }

    pub fn nearest_node(&self, at: (f32, f32)) -> usize {
        (0..self.down.len() * self.across.len())
            .min_by(|&a, &b| {
                let (pa, pb) = (self.position(a), self.position(b));
                distance(pa, at).total_cmp(&distance(pb, at))
            })
            .unwrap_or(0)
    }
}

pub fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

pub fn point_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    if length < 0.001 {
        return distance(p, a);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length).clamp(0.0, 1.0);
    distance(p, (a.0 + dx * t, a.1 + dy * t))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The traffic has to be stable, bounded, and — most of all — uneven: a city where every street
    /// is equally slow says nothing, and a route coloured from it would be one flat colour.
    #[test]
    fn the_traffic_is_the_same_every_run_and_bad_in_only_a_few_places() {
        assert_eq!(
            jam("Bd Saint-Germain", Class::Primary),
            jam("Bd Saint-Germain", Class::Primary)
        );
        assert!(jam("Rue du Bac", Class::Residential) != jam("Rue de Rivoli", Class::Residential));

        let world = World::generate();
        let all: Vec<f32> = world.down.iter().chain(&world.across).map(Street::pace).collect();
        for (street, pace) in world.down.iter().chain(&world.across).zip(&all) {
            assert!(*pace > 0.0 && *pace <= street.class.speed(), "{} paces wrong", street.name);
        }
        // Most of it moving, some of it not, which is what makes amber and red mean anything.
        let stopped =
            world.down.iter().chain(&world.across).filter(|street| street.jam() >= BUSY).count();
        assert!(stopped > 0, "a city with no jam in it has nothing to colour");
        assert!(stopped * 2 < all.len(), "a city that is all jam has no clear to contrast with");
    }
}

//! The two things on a dashboard that are not about driving: the sky, and what is playing.
//!
//! Neither has a source here — there is no radio in a terminal and no weather station in a generated
//! city — so both are stand-ins, and they are stand-ins of the same kind as the traffic: a stable
//! number that everything downstream of it treats as real. The temperature drifts the way an
//! afternoon does, the track advances in real seconds, and the controls actually control something.
//! Swapping either for a feed is a change to this file and to nothing else.

/// What the sky is doing. Enough to pick a glyph and a word, which is all a dash ever says about it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sky {
    Clear,
    Cloud,
    Rain,
}

impl Sky {
    /// One character, because the weather is worth a glyph and not a sentence.
    pub fn glyph(self) -> &'static str {
        match self {
            Sky::Clear => "☀",
            Sky::Cloud => "☁",
            Sky::Rain => "☂",
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Sky::Clear => "clear",
            Sky::Cloud => "cloudy",
            Sky::Rain => "rain",
        }
    }
}

pub struct Weather {
    pub sky: Sky,
    /// Degrees Celsius, outside.
    pub outside: f32,
    /// Seconds since the engine started, which is all the forecast there is.
    elapsed: f32,
}

impl Weather {
    /// An evening in a city the router thinks is Paris.
    pub fn new() -> Weather {
        Weather { sky: Sky::Clear, outside: 21.0, elapsed: 0.0 }
    }

    /// Cools off the way an evening does, a tenth of a degree a minute, and no further than a night.
    ///
    /// The sky turns on a fixed round of its own, slowly enough that it is never what changed while a
    /// driver was looking at it — a weather readout that flickers is one nobody believes twice.
    pub fn tick(&mut self, dt: f32) {
        self.elapsed += dt;
        self.outside = (self.outside - dt / 600.0).max(14.0);
        const TURN: [Sky; 6] =
            [Sky::Clear, Sky::Clear, Sky::Cloud, Sky::Clear, Sky::Cloud, Sky::Rain];
        self.sky = TURN[(self.elapsed / 90.0) as usize % TURN.len()];
    }

    pub fn degrees(&self) -> i32 {
        self.outside.round() as i32
    }
}

pub struct Track {
    pub title: &'static str,
    pub artist: &'static str,
    /// Seconds.
    pub length: f32,
}

/// What is playing, in the only terms a dash shows: a title, an artist, and how far in it is.
pub struct Player {
    pub tracks: Vec<Track>,
    pub index: usize,
    pub playing: bool,
    /// Seconds into the current track.
    pub elapsed: f32,
}

impl Player {
    pub fn new() -> Player {
        Player {
            tracks: vec![
                Track { title: "Nocturne in E♭", artist: "Chopin", length: 271.0 },
                Track { title: "So What", artist: "Miles Davis", length: 545.0 },
                Track { title: "Stratus", artist: "Billy Cobham", length: 578.0 },
                Track { title: "Teardrop", artist: "Massive Attack", length: 330.0 },
                Track { title: "Rondo alla Turca", artist: "Mozart", length: 218.0 },
            ],
            index: 0,
            playing: true,
            elapsed: 34.0,
        }
    }

    pub fn track(&self) -> &Track {
        &self.tracks[self.index]
    }

    /// Runs the clock, and rolls into the next track at the end of this one.
    pub fn tick(&mut self, dt: f32) {
        if !self.playing {
            return;
        }
        self.elapsed += dt;
        if self.elapsed >= self.track().length {
            self.skip(1);
        }
    }

    /// How far through, nought to one, which is the form a bar wants.
    pub fn through(&self) -> f32 {
        (self.elapsed / self.track().length.max(1.0)).clamp(0.0, 1.0)
    }

    pub fn toggle(&mut self) {
        self.playing = !self.playing;
    }

    /// Forwards or back. Back inside the first few seconds means the previous track, and later means
    /// the start of this one, because that is what every other player a driver has used does.
    pub fn skip(&mut self, by: i32) {
        if by < 0 && self.elapsed > 4.0 {
            self.elapsed = 0.0;
            return;
        }
        let count = self.tracks.len() as i32;
        self.index = (self.index as i32 + by).rem_euclid(count) as usize;
        self.elapsed = 0.0;
    }

    /// `1:32`, the way a player says it.
    pub fn at(&self) -> String {
        clock(self.elapsed)
    }

    pub fn of(&self) -> String {
        clock(self.track().length)
    }
}

fn clock(seconds: f32) -> String {
    let whole = seconds.max(0.0) as u32;
    format!("{}:{:02}", whole / 60, whole % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The controls have to control it: a transport that is only a picture of a transport is furniture.
    #[test]
    fn the_transport_keys_move_the_music() {
        let mut player = Player::new();
        let first = player.track().title;

        player.skip(1);
        assert_ne!(player.track().title, first, "next stayed on the same track");
        assert_eq!(player.elapsed, 0.0, "a new track starts at the start");

        // Back inside the first few seconds is the previous track; later it is this one, from the top.
        player.elapsed = 30.0;
        player.skip(-1);
        assert_eq!(player.track().title, player.tracks[1].title, "that should have rewound");
        player.skip(-1);
        assert_eq!(player.track().title, first, "and that should have gone back a track");

        // The clock only runs while it is playing, and rolls over at the end of a track.
        player.toggle();
        player.tick(10.0);
        assert_eq!(player.elapsed, 0.0, "a paused player is not playing");
        player.toggle();
        player.elapsed = player.track().length - 1.0;
        player.tick(2.0);
        assert_eq!(player.index, 1, "the end of a track is the start of the next one");
    }
}

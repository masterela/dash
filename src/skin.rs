//! Two skins, and one reason for each colour in them.
//!
//! A car dashboard is read in two lighting conditions that have nothing in common: a night drive,
//! where anything bright is a reflection in the windscreen, and full sun, where a dark screen is a
//! mirror. So there are two grounds and everything else follows from which one is underneath.
//!
//! The hues barely change between them. What changes is which end of the scale carries the ink: on
//! the dark skin the roads are the light thing on a dark ground, and on the light skin the ground is
//! the light thing and the roads are defined by their casings — which is exactly how printed maps
//! have always done it.

use conui::{Color, Theme};

use crate::paint::Rgb;

/// The map's own colours, in pixels.
pub struct Palette {
    pub ground: Rgb,
    pub water: Rgb,
    pub shore: Rgb,
    pub park: Rgb,
    pub building: Rgb,
    pub casing: Rgb,
    pub residential: Rgb,
    pub primary: Rgb,
    pub motorway: Rgb,
    /// The route, and what it turns into where the traffic on it has stopped moving.
    ///
    /// Blue rather than green, and deliberately: green on a road means "this is moving", so a green
    /// route and a green traffic reading are two different claims in one colour. Blue says "this is
    /// yours" and leaves the whole green-amber-red scale free to mean the one thing it always means.
    pub route: Rgb,
    pub route_casing: Rgb,
    /// Traffic, on the route and in the rail. The only green-amber-red on screen.
    pub clear: Rgb,
    pub busy: Rgb,
    pub stopped: Rgb,
    pub car: Rgb,
    /// Behind a street name, so the label has contrast wherever it lands.
    pub label: Rgb,
    /// The manoeuvre card, and how much of the map shows through it.
    ///
    /// It needs an outline as well as a fill: a card whose fill is close enough to the ground to
    /// keep the map legible through it is, by that same token, too close to have an edge.
    pub plate: Rgb,
    pub plate_edge: Rgb,
    pub plate_alpha: f32,
    /// An empty bar, an unused lane — present, but not information.
    pub track: Rgb,
    /// The card's edge and its countdown while the manoeuvre is still far off.
    pub calm: Rgb,
    /// The same, once it is close enough to act on.
    ///
    /// Louder than `calm` and the same hue as it, never amber: amber and red are the traffic's, and a
    /// card that borrows one of them says "there is a jam" to anybody not reading the whole screen.
    pub press: Rgb,
}

/// The same areas in braille mode, where they are cell backgrounds rather than pixels — so they
/// have to be dark enough for a glyph to show on, which the map's own colours are not.
pub struct Blocks {
    pub water: Color,
    pub park: Color,
    pub building: Color,
}

pub struct Skin {
    pub theme: Theme,
    pub map: Palette,
    pub blocks: Blocks,
}

/// A map colour as a cell colour, for the two readings that appear in both.
pub const fn ink(colour: Rgb) -> Color {
    Color::Rgb(colour.0, colour.1, colour.2)
}

impl Skin {
    pub const DARK: Skin = Skin {
        // `Theme::LAYA` with the accent turned from mint to the same blue the route is drawn in, so
        // that everything the dash says about *your* journey is one colour and traffic owns green.
        theme: Theme { accent: Color::hex("#62b7f5"), ..Theme::LAYA },
        map: Palette {
            ground: Rgb::hex("#090f13"),
            water: Rgb::hex("#14323d"),
            shore: Rgb::hex("#1d4b5b"),
            park: Rgb::hex("#1a3a2f"),
            building: Rgb::hex("#16252b"),
            casing: Rgb::hex("#050a0d"),
            residential: Rgb::hex("#3c555f"),
            primary: Rgb::hex("#8ea2a7"),
            motorway: Rgb::hex("#f0c987"),
            // The same hue and saturation the mint had, rotated round to blue: #62f5b5 → #62b7f5.
            route: Rgb::hex("#62b7f5"),
            route_casing: Rgb::hex("#0c2a3a"),
            clear: Rgb::hex("#62f5b5"),
            busy: Rgb::hex("#ffce73"),
            stopped: Rgb::hex("#ff5a68"),
            car: Rgb::hex("#eafff6"),
            label: Rgb::hex("#090f13"),
            plate: Rgb::hex("#0c1519"),
            plate_edge: Rgb::hex("#28383f"),
            plate_alpha: 0.92,
            track: Rgb::hex("#3a5159"),
            calm: Rgb::hex("#62b7f5"),
            press: Rgb::hex("#b8e4ff"),
        },
        blocks: Blocks {
            water: Color::hex("#102a35"),
            park: Color::hex("#16302b"),
            building: Color::hex("#18272d"),
        },
    };

    /// Paper, for sunlight. The accent has to be darkened to survive on it: a mint that glows on
    /// black is invisible on white, and the same is true of every colour that was chosen for a
    /// dark ground.
    pub const LIGHT: Skin = Skin {
        theme: Theme {
            background: Color::hex("#eef1ec"),
            text: Color::hex("#16201c"),
            muted: Color::hex("#5d6f68"),
            dim: Color::hex("#bcc6bf"),
            accent: Color::hex("#0a4d7d"),
            warn: Color::hex("#a2600f"),
            danger: Color::hex("#bc2f24"),
            info: Color::hex("#1a6883"),
            surface: Color::hex("#dde4de"),
        },
        map: Palette {
            ground: Rgb::hex("#eae9e2"),
            water: Rgb::hex("#a8cddc"),
            shore: Rgb::hex("#7fb0c4"),
            park: Rgb::hex("#cbe2c6"),
            building: Rgb::hex("#dcdbd3"),
            // On paper a road is its outline. Take the casing away and the whole map dissolves.
            casing: Rgb::hex("#b3b8ad"),
            residential: Rgb::hex("#ffffff"),
            primary: Rgb::hex("#fff4d2"),
            motorway: Rgb::hex("#f6b35c"),
            route: Rgb::hex("#0e629b"),
            route_casing: Rgb::hex("#073a58"),
            clear: Rgb::hex("#0a7d51"),
            busy: Rgb::hex("#c07407"),
            stopped: Rgb::hex("#c02b20"),
            car: Rgb::hex("#ffffff"),
            label: Rgb::hex("#eae9e2"),
            plate: Rgb::hex("#ffffff"),
            plate_edge: Rgb::hex("#b4beb6"),
            plate_alpha: 0.94,
            track: Rgb::hex("#c8d1c7"),
            calm: Rgb::hex("#0a4d7d"),
            // On paper louder means darker, so the press is the calm blue driven further down.
            press: Rgb::hex("#062f52"),
        },
        blocks: Blocks {
            water: Color::hex("#cfe3ec"),
            park: Color::hex("#d8e8d4"),
            building: Color::hex("#e2e1d9"),
        },
    };

    pub fn pick(light: bool) -> &'static Skin {
        if light { &Skin::LIGHT } else { &Skin::DARK }
    }
}

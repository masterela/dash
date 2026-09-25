//! Putting real pixels on a terminal, via the kitty graphics protocol.
//!
//! By default the pixels travel through a temp file rather than the escape sequence itself. A frame
//! of this map is a few megabytes, and base64 down a pty at twenty frames a second is the one part
//! of this that would actually be slow; handing over a path costs eighty bytes. The cost is that it
//! only works locally, which for a dashboard bolted to a car is not a cost — and that the terminal
//! has to agree to open the file, which is why [`Medium::Inline`] exists as the fallback that is
//! guaranteed to work anywhere the protocol does at all.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::paint::{Rgb, Surface};

/// Image and placement id. Reusing them means each frame replaces the last rather than stacking.
const ID: u32 = 8071;

/// Most base64 bytes allowed in one escape sequence. The protocol's own limit.
const CHUNK: usize = 4096;

/// How the pixels get from here to the terminal.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Medium {
    /// A path in a temp file. Cheap, and what a local terminal should prefer.
    File,
    /// Base64 in the escape sequence, split into chunks. Works over ssh and through a multiplexer.
    Inline,
}

pub struct Graphics {
    id: u32,
    depth: i32,
    medium: Medium,
    paths: [PathBuf; 2],
    slot: usize,
    shown: bool,
}

impl Graphics {
    /// Whether this terminal understands a picture.
    ///
    /// Asking would be better than guessing, but the reply arrives in the input stream that the
    /// event loop owns, and a wrong guess here costs a flag rather than a broken screen.
    pub fn available() -> bool {
        if let Ok(forced) = std::env::var("DASH_PIXELS") {
            return forced != "0";
        }
        let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
        let term = std::env::var("TERM").unwrap_or_default();
        std::env::var("KITTY_WINDOW_ID").is_ok()
            || std::env::var("WEZTERM_PANE").is_ok()
            || term.contains("kitty")
            || matches!(program.as_str(), "ghostty" | "WezTerm")
    }

    /// One terminal cell in pixels, straight from the tty, or `None` if it will not say.
    ///
    /// Worth asking rather than guessing: on a high-density display a cell is nearer 16x34 than
    /// 8x17, and rendering at half the real size means the terminal scales the map up and every
    /// edge this renderer took care over comes back blurred.
    ///
    /// This used to be a raw `ioctl` with a request constant per operating system, written here
    /// because conui read the two pixel fields of `winsize` and threw them away. It asks the library
    /// now, which is where the question belonged: a terminal's own dimensions are not a dashboard's
    /// business, and the app that needed them had ended up with the only `unsafe` in it.
    pub fn cell_size() -> Option<(u16, u16)> {
        conui::cell_pixels()
    }

    pub fn new(medium: Medium) -> Graphics {
        Graphics::with(ID, -1, medium)
    }

    fn with(id: u32, depth: i32, medium: Medium) -> Graphics {
        // kitty only opens a temp file whose path contains this, so that a stray escape sequence
        // cannot talk a terminal into reading an arbitrary file. Ghostty keeps the same rule.
        let base = std::env::temp_dir();
        let name = |slot: char| format!("tty-graphics-protocol-dash-{}-{id}-{slot}.rgb", pid());
        Graphics {
            id,
            depth,
            medium,
            paths: [base.join(name('a')), base.join(name('b'))],
            slot: 0,
            shown: false,
        }
    }

    /// Place `surface` over the cell rectangle at `(col, row)`, scaled to fill it.
    ///
    /// `z=-1` is the whole trick: it puts the image above the cell backgrounds but below the text,
    /// so the rail and the street names stay terminal glyphs drawn on top of a photograph.
    pub fn show(
        &mut self,
        surface: &Surface,
        col: u16,
        row: u16,
        cols: u16,
        rows: u16,
    ) -> io::Result<()> {
        let mut out = Vec::with_capacity(1 << 12);
        // CUP is 1-based, and the image lands wherever the cursor is.
        write!(out, "\x1b[{};{}H", row + 1, col + 1)?;
        let keys = format!(
            "a=T,f=24,s={},v={},i={},p={},c={cols},r={rows},z={},C=1,q=2",
            surface.width, surface.height, self.id, self.id, self.depth,
        );

        match self.medium {
            Medium::File => {
                // Two paths in rotation: the terminal is still reading the last one when this one
                // is written.
                self.slot = 1 - self.slot;
                let path = &self.paths[self.slot];
                fs::write(path, &surface.pixels)?;
                let payload = base64(path.to_string_lossy().as_bytes());
                write!(out, "\x1b_G{keys},t=t;{payload}\x1b\\")?;
            }
            Medium::Inline => {
                let payload = base64(&surface.pixels);
                let bytes = payload.as_bytes();
                let last = bytes.len().div_ceil(CHUNK).saturating_sub(1);
                for (index, chunk) in bytes.chunks(CHUNK).enumerate() {
                    let more = u8::from(index < last);
                    // All the control data goes in the first chunk; the rest say only "more".
                    if index == 0 {
                        write!(out, "\x1b_G{keys},t=d,m={more};")?;
                    } else {
                        write!(out, "\x1b_Gm={more};")?;
                    }
                    out.extend_from_slice(chunk);
                    out.extend_from_slice(b"\x1b\\");
                }
            }
        }

        let mut stdout = io::stdout().lock();
        stdout.write_all(&out)?;
        stdout.flush()?;
        self.shown = true;
        Ok(())
    }

    /// Take the picture back off the screen and clean up after it.
    pub fn clear(&mut self) -> io::Result<()> {
        if self.shown {
            let mut stdout = io::stdout().lock();
            write!(stdout, "\x1b_Ga=d,d=I,i={},q=2\x1b\\", self.id)?;
            stdout.flush()?;
            self.shown = false;
        }
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
        Ok(())
    }
}

/// Three ways of showing the same patch, so one run says which of them this terminal does.
///
/// A picture that does not turn up has exactly three interesting causes: the terminal refused the
/// temp file, it draws `z<0` under the cell background rather than over it, or it does not speak the
/// protocol at all. Each patch removes one of them.
pub fn probe() -> io::Result<()> {
    let (cw, ch) = Graphics::cell_size().unwrap_or((8, 17));
    let (cols, rows) = (24u16, 4u16);
    let (width, height) = (u32::from(cols) * u32::from(cw), u32::from(rows) * u32::from(ch));

    let patches = [
        ("A  temp file, z=-1  — what the dash does", Medium::File, -1, Rgb::hex("#2f7f55")),
        ("B  temp file, z=1   — above the text", Medium::File, 1, Rgb::hex("#b07028")),
        ("C  inline,    z=1   — no file involved", Medium::Inline, 1, Rgb::hex("#2b5f9f")),
    ];

    let mut out = io::stdout().lock();
    write!(out, "\x1b[2J\x1b[H")?;
    writeln!(out, "kitty graphics probe · cell {cw}x{ch} · patch {width}x{height} px\r")?;
    writeln!(out, "Each patch should be a coloured block with a pale disc in it.\r")?;
    out.flush()?;

    let mut all = Vec::new();
    for (index, (label, medium, depth, colour)) in patches.into_iter().enumerate() {
        let top = 4 + index as u16 * (rows + 2);
        // A dark, explicitly coloured cell background under the patch — the thing suspected of
        // hiding the image — with text on it, so it is obvious what is drawn over what.
        write!(out, "\x1b[{};1H{label}", top)?;
        for line in 0..rows {
            write!(out, "\x1b[{};1H\x1b[48;2;20;24;28m", top + 1 + line)?;
            write!(out, "{:width$}", "cell background", width = usize::from(cols))?;
            write!(out, "\x1b[0m")?;
        }
        out.flush()?;

        let mut surface = Surface::new(width, height, colour);
        surface.paint(1, |band| {
            band.disc(
                (width as f32 * 0.5, height as f32 * 0.5),
                height as f32 * 0.35,
                Rgb::hex("#eafff6"),
            );
        });
        let mut graphics = Graphics::with(9000 + index as u32, depth, medium);
        graphics.show(&surface, 0, top + 1, cols, rows)?;
        all.push(graphics);
    }

    let bottom = 4 + 3 * (rows + 2);
    write!(out, "\x1b[{bottom};1H")?;
    writeln!(out, "A visible  → the dash should work; if it is not, the bug is elsewhere.\r")?;
    writeln!(out, "A missing, B visible → z=-1 is drawn under cell backgrounds.\r")?;
    writeln!(out, "A and B missing, C visible → the temp file was refused.\r")?;
    writeln!(out, "None visible → this terminal does not speak the protocol.\r")?;
    writeln!(out, "\nENTER to clear.\r")?;
    out.flush()?;

    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    for graphics in &mut all {
        graphics.clear()?;
    }
    Ok(())
}

fn pid() -> u32 {
    std::process::id()
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for group in data.chunks(3) {
        let bits = group
            .iter()
            .enumerate()
            .fold(0u32, |bits, (index, &byte)| bits | (u32::from(byte) << (16 - index * 8)));
        for index in 0..4 {
            if index <= group.len() {
                out.push(char::from(ALPHABET[(bits >> (18 - index * 6)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Graphics, Medium, base64};

    #[test]
    fn base64_matches_the_examples_from_the_rfc() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(b"/tmp/dash-1-a.rgb"), "L3RtcC9kYXNoLTEtYS5yZ2I=");
    }

    #[test]
    fn the_temp_paths_carry_the_marker_the_terminal_insists_on() {
        let graphics = Graphics::new(Medium::File);
        for path in &graphics.paths {
            let path = path.to_string_lossy();
            assert!(path.contains("tty-graphics-protocol"), "{path} would be refused");
        }
    }
}

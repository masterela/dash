//! A PNG writer, so a frame can be looked at as a file instead of only on a screen.
//!
//! Stored deflate blocks: no compression at all, which is the right trade for something only ever
//! used to check a render. The live path does not go through here.

use crate::paint::Surface;

pub fn encode(surface: &Surface) -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

    let mut header = Vec::new();
    header.extend_from_slice(&surface.width.to_be_bytes());
    header.extend_from_slice(&surface.height.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]); // 8 bits, truecolour, no interlace.
    chunk(&mut out, b"IHDR", &header);

    // Each scanline is prefixed by its filter type, and none of them are filtered.
    let mut raw = Vec::with_capacity(surface.pixels.len() + surface.height as usize);
    for row in surface.pixels.chunks_exact((surface.width * 3) as usize) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    chunk(&mut out, b"IDAT", &zlib(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = crc32(0xffff_ffff, kind);
    crc = crc32(crc, data);
    out.extend_from_slice(&(crc ^ 0xffff_ffff).to_be_bytes());
}

fn zlib(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    for (index, block) in raw.chunks(0xffff).enumerate() {
        let last = u8::from((index + 1) * 0xffff >= raw.len());
        let length = block.len() as u16;
        out.push(last);
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&(!length).to_le_bytes());
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let (mut low, mut high) = (1u32, 0u32);
    for &byte in data {
        low = (low + u32::from(byte)) % 65521;
        high = (high + low) % 65521;
    }
    (high << 16) | low
}

fn crc32(start: u32, data: &[u8]) -> u32 {
    let mut crc = start;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            // The reflected CRC-32 polynomial, computed rather than tabulated: this runs once
            // per saved file, not once per frame.
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    crc
}

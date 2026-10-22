//! The BSP header and lump directory, including Valve's LZMA lump wrapper.

use std::borrow::Cow;

use super::BspError;

pub const IDENT: u32 = u32::from_le_bytes(*b"VBSP");
pub const HEADER_LUMPS: usize = 64;
pub const HEADER_SIZE: usize = 8 + HEADER_LUMPS * 16 + 4;
pub const LZMA_IDENT: u32 = u32::from_le_bytes(*b"LZMA");

/// The lumps this crate reads, by their index in the directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LumpId {
    Entities = 0,
    Planes = 1,
    Vertexes = 3,
    Nodes = 5,
    Faces = 7,
    Leafs = 10,
    Edges = 12,
    SurfEdges = 13,
    Models = 14,
    LeafBrushes = 17,
    Brushes = 18,
    BrushSides = 19,
    DispInfo = 26,
    DispVerts = 33,
}

#[derive(Debug)]
pub struct Lumps<'a> {
    version: u32,
    bytes: &'a [u8],
}

impl<'a> Lumps<'a> {
    /// Checks the header; individual lumps are bounds-checked when read.
    pub fn read(bytes: &'a [u8]) -> Result<Self, BspError> {
        if bytes.len() < HEADER_SIZE {
            return Err(BspError::TooShort(bytes.len()));
        }
        let ident = u32_at(bytes, 0);
        if ident != IDENT {
            return Err(BspError::BadIdent(ident));
        }
        let version = u32_at(bytes, 4);
        if !(19..=21).contains(&version) {
            return Err(BspError::UnsupportedVersion(version));
        }
        Ok(Self { version, bytes })
    }

    /// The lump's contents, decompressed if the map stores it compressed.
    pub fn get(&self, lump: LumpId) -> Result<Cow<'a, [u8]>, BspError> {
        let entry = 8 + lump as usize * 16;
        let offset = u32_at(self.bytes, entry) as usize;
        let length = u32_at(self.bytes, entry + 4) as usize;
        let uncompressed = u32_at(self.bytes, entry + 12) as usize;
        if length == 0 {
            return Ok(Cow::Borrowed(&[]));
        }
        let data = offset
            .checked_add(length)
            .and_then(|end| self.bytes.get(offset..end))
            .ok_or(BspError::LumpOutOfBounds { lump })?;
        if uncompressed == 0 {
            return Ok(Cow::Borrowed(data));
        }
        decompress(lump, data, uncompressed).map(Cow::Owned)
    }
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("caller checked the length"))
}

/// Decodes Valve's `lzma_header_t` (ident, actual size, compressed size, five
/// property bytes) followed by a raw LZMA stream.
fn decompress(lump: LumpId, data: &[u8], expected: usize) -> Result<Vec<u8>, BspError> {
    let fail = |reason: String| BspError::Lzma { lump, reason };
    if data.len() < 17 || u32_at(data, 0) != LZMA_IDENT {
        return Err(fail("missing the LZMA header".to_string()));
    }
    let actual = u32_at(data, 4) as usize;
    if actual != expected {
        return Err(fail(format!("header says {actual} bytes, lump table says {expected}")));
    }
    let mut stream = Vec::with_capacity(data.len() - 12);
    stream.extend_from_slice(&data[12..17]);
    stream.extend_from_slice(&data[17..]);
    let options = lzma_rs::decompress::Options {
        unpacked_size: lzma_rs::decompress::UnpackedSize::UseProvided(Some(actual as u64)),
        ..Default::default()
    };
    let mut out = Vec::with_capacity(actual);
    lzma_rs::lzma_decompress_with_options(&mut &stream[..], &mut out, &options).map_err(|e| fail(e.to_string()))?;
    if out.len() != actual {
        return Err(fail(format!("decompressed to {} bytes, expected {actual}", out.len())));
    }
    Ok(out)
}

impl Lumps<'_> {
    pub fn version(&self) -> u32 {
        self.version
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A header-only BSP whose lumps follow it in the given order.
    pub(crate) fn bsp_with(version: u32, lumps: &[(LumpId, Vec<u8>, u32)]) -> Vec<u8> {
        let mut out = vec![0u8; HEADER_SIZE];
        out[0..4].copy_from_slice(&IDENT.to_le_bytes());
        out[4..8].copy_from_slice(&version.to_le_bytes());
        for (lump, data, uncompressed) in lumps {
            let entry = 8 + *lump as usize * 16;
            let offset = out.len() as u32;
            out[entry..entry + 4].copy_from_slice(&offset.to_le_bytes());
            out[entry + 4..entry + 8].copy_from_slice(&(data.len() as u32).to_le_bytes());
            out[entry + 12..entry + 16].copy_from_slice(&uncompressed.to_le_bytes());
            out.extend_from_slice(data);
        }
        out
    }

    /// `data` in Valve's lzma_header_t wrapper around a raw LZMA stream.
    pub(crate) fn valve_lzma(data: &[u8]) -> Vec<u8> {
        let mut alone = Vec::new();
        lzma_rs::lzma_compress(&mut &data[..], &mut alone).expect("compresses");
        // .lzma "alone" files are 5 property bytes, a u64 size, then the stream.
        let (properties, stream) = (&alone[0..5], &alone[13..]);
        let mut out = Vec::new();
        out.extend_from_slice(&LZMA_IDENT.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(stream.len() as u32).to_le_bytes());
        out.extend_from_slice(properties);
        out.extend_from_slice(stream);
        out
    }

    #[test]
    fn a_short_file_is_refused() {
        assert_eq!(Lumps::read(&[0; 10]).err(), Some(BspError::TooShort(10)));
    }

    #[test]
    fn a_wrong_ident_is_refused() {
        let mut bytes = bsp_with(20, &[]);
        bytes[0] = b'X';
        assert!(matches!(Lumps::read(&bytes), Err(BspError::BadIdent(_))));
    }

    #[test]
    fn an_unsupported_version_is_refused() {
        assert_eq!(Lumps::read(&bsp_with(17, &[])).err(), Some(BspError::UnsupportedVersion(17)));
    }

    #[test]
    fn a_plain_lump_reads_back_as_written() {
        let bytes = bsp_with(20, &[(LumpId::Planes, vec![1, 2, 3, 4], 0)]);
        let lumps = Lumps::read(&bytes).expect("valid header");
        assert_eq!(lumps.get(LumpId::Planes).expect("in bounds").as_ref(), &[1, 2, 3, 4]);
        assert!(lumps.get(LumpId::Brushes).expect("empty is fine").is_empty());
    }

    #[test]
    fn a_compressed_lump_is_decompressed() {
        let original: Vec<u8> = (0..200u8).cycle().take(5000).collect();
        let bytes = bsp_with(21, &[(LumpId::Entities, valve_lzma(&original), original.len() as u32)]);
        let lumps = Lumps::read(&bytes).expect("valid header");
        assert_eq!(lumps.get(LumpId::Entities).expect("decompresses").as_ref(), &original[..]);
    }

    #[test]
    fn a_lump_past_the_end_is_refused() {
        let mut bytes = bsp_with(20, &[(LumpId::Planes, vec![0; 20], 0)]);
        bytes.truncate(bytes.len() - 1);
        let lumps = Lumps::read(&bytes).expect("header is intact");
        assert_eq!(lumps.get(LumpId::Planes).err(), Some(BspError::LumpOutOfBounds { lump: LumpId::Planes }));
    }
}

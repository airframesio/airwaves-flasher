//! Local image inspection for the file picker: size, compression (by magic
//! bytes, extension as a hint), and — for xz — the exact uncompressed size read
//! from the stream index footer (accurate progress + capacity check).

use crate::error::{FlashError, Result};
use crate::model::{Compression, LocalImage};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Sniff the compression format from the first bytes (authoritative), falling
/// back to the filename only when the magic is absent.
pub fn sniff_compression(head: &[u8], name: &str) -> Compression {
    if head.len() >= 6 && head[..6] == [0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00] {
        Compression::Xz
    } else if head.len() >= 2 && head[..2] == [0x1F, 0x8B] {
        Compression::Gz
    } else if head.len() >= 4 && head[..4] == [0x28, 0xB5, 0x2F, 0xFD] {
        Compression::Zst
    } else if head.len() >= 4 && head[..4] == [0x50, 0x4B, 0x03, 0x04] {
        Compression::Zip
    } else {
        // raw images have no reliable magic — fall back to the name, default raw
        match Compression::from_name(name) {
            Compression::None => Compression::None,
            // if name says compressed but magic disagrees, trust raw
            _ => Compression::None,
        }
    }
}

pub fn inspect_image(path: &str) -> Result<LocalImage> {
    let p = Path::new(path);
    let mut f = File::open(p).map_err(|e| FlashError::Image(e.to_string()))?;
    let size = f.metadata().map_err(|e| FlashError::Image(e.to_string()))?.len();

    let mut head = [0u8; 8];
    let n = f.read(&mut head).unwrap_or(0);
    let name = p
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());
    let compression = sniff_compression(&head[..n], &name);

    let extract_size = match compression {
        Compression::None => Some(size),
        Compression::Xz => xz_uncompressed_size(&mut f).ok(),
        _ => None,
    };

    Ok(LocalImage {
        path: path.to_string(),
        name,
        size,
        compression,
        extract_size,
    })
}

/// Parse the (single-stream) xz index footer to get the total uncompressed
/// size without decompressing. Returns Err if the layout is unexpected.
fn xz_uncompressed_size(f: &mut File) -> Result<u64> {
    let len = f.metadata().map_err(|e| FlashError::Image(e.to_string()))?.len();
    if len < 12 {
        return Err(FlashError::Image("xz too short".into()));
    }
    // stream footer: CRC32(4) | backward_size(4 LE) | stream_flags(2) | "YZ"(2)
    let mut footer = [0u8; 12];
    f.seek(SeekFrom::Start(len - 12)).map_err(io)?;
    f.read_exact(&mut footer).map_err(io)?;
    if footer[10] != 0x59 || footer[11] != 0x5A {
        return Err(FlashError::Image("not an xz footer".into()));
    }
    let backward_size = u32::from_le_bytes([footer[4], footer[5], footer[6], footer[7]]);
    let index_size = (backward_size as u64 + 1) * 4;
    if index_size + 12 > len {
        return Err(FlashError::Image("bad xz index size".into()));
    }
    let index_start = len - 12 - index_size;
    let mut index = vec![0u8; index_size as usize];
    f.seek(SeekFrom::Start(index_start)).map_err(io)?;
    f.read_exact(&mut index).map_err(io)?;

    // index: 0x00 indicator | varint(count) | count*(varint unpadded, varint uncompressed) | padding | CRC32
    let mut pos = 0usize;
    if index.first() != Some(&0x00) {
        return Err(FlashError::Image("bad xz index indicator".into()));
    }
    pos += 1;
    let count = read_varint(&index, &mut pos)?;
    let mut total = 0u64;
    for _ in 0..count {
        let _unpadded = read_varint(&index, &mut pos)?;
        let uncompressed = read_varint(&index, &mut pos)?;
        total = total.saturating_add(uncompressed);
    }
    Ok(total)
}

/// LZMA/xz little-endian base-128 variable-length integer.
fn read_varint(buf: &[u8], pos: &mut usize) -> Result<u64> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    loop {
        let byte = *buf
            .get(*pos)
            .ok_or_else(|| FlashError::Image("xz varint overrun".into()))?;
        *pos += 1;
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 63 {
            return Err(FlashError::Image("xz varint too long".into()));
        }
    }
    Ok(result)
}

fn io(e: std::io::Error) -> FlashError {
    FlashError::Image(e.to_string())
}

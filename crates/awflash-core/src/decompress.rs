//! Streaming, on-the-fly decompression. We peek the first bytes, decide the
//! format by magic (extension is only a hint), then return a `Read` that yields
//! raw image bytes. Zip is handled separately (needs Read+Seek) in `flash.rs`.

use crate::inspect::sniff_compression;
use crate::model::Compression;
use flate2::read::MultiGzDecoder;
use liblzma::read::XzDecoder;
use std::io::{self, Cursor, Read};

/// A boxed reader yielding decompressed image bytes.
pub type ImageReader = Box<dyn Read + Send>;

fn read_head<R: Read>(reader: &mut R, n: usize) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; n];
    let mut filled = 0;
    while filled < n {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(k) => filled += k,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    buf.truncate(filled);
    Ok(buf)
}

/// Wrap a streaming source in the right decoder, sniffing the format from the
/// leading bytes. `hint` (from the catalog/inspect) is used only if the magic
/// is absent. Never used for zip (caller routes zip to a file path).
pub fn open_stream(mut reader: ImageReader, hint: Compression) -> io::Result<ImageReader> {
    let head = read_head(&mut reader, 6)?;
    let mut comp = sniff_compression(&head, "");
    if comp == Compression::None && hint != Compression::Zip {
        comp = hint;
    }
    let combined = Cursor::new(head).chain(reader);
    Ok(match comp {
        Compression::Xz => Box::new(XzDecoder::new(combined)),
        Compression::Gz => Box::new(MultiGzDecoder::new(combined)), // multistream-safe
        Compression::Zst => Box::new(zstd::stream::read::Decoder::new(combined)?),
        Compression::Zip => Box::new(combined), // unreachable in practice
        Compression::None => Box::new(combined),
    })
}

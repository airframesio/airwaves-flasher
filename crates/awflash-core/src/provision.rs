//! Provision the AWCFG config partition on a just-written card.
//!
//! Current Airwaves OS images do NOT ship an AWCFG partition (it's created by
//! the on-device installer). For direct-boot boards we create it here: add a
//! FAT partition at the tail of the device, build a FAT filesystem in memory
//! with `airwaves-install.json`, and write it — all via sector-aligned writes
//! on the raw device, so no mount/partition-reread is needed. On first boot the
//! device's `airwaves-preconfig` finds the `AWCFG` label and applies the config.

use crate::customize::build_install_json;
use crate::error::{FlashError, Result};
use crate::model::Customization;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

const SECTOR: u64 = 512;
/// Size of the AWCFG partition we create (FAT, label AWCFG). Sits at the tail;
/// the OS grows root into the remaining free space on first boot.
const AWCFG_BYTES: u64 = 64 * 1024 * 1024;
/// Microsoft Basic Data partition type GUID (mixed-endian, FAT-mountable).
const MS_BASIC_DATA: [u8; 16] = [
    0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44, 0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7,
];
const README: &str = "Airwaves OS configuration partition (AWCFG)\n\n\
Edit airwaves-install.json on any computer (Windows/macOS/Linux) to pre-configure\n\
this device. It is applied automatically on first boot. To re-apply on an\n\
already-set-up device, also create an empty file named apply.conf.\n";

/// Create (or rewrite) the AWCFG partition and write the first-boot config.
/// Returns Ok(false) if the partition scheme is unsupported / there's no room.
pub fn add_awcfg(dev: &mut File, device_size_bytes: u64, c: &Customization) -> Result<bool> {
    if !c.enabled {
        return Ok(false);
    }
    let json = build_install_json(c);

    let mut sec0 = [0u8; 512];
    dev.seek(SeekFrom::Start(0)).map_err(io)?;
    dev.read_exact(&mut sec0).map_err(io)?;
    if sec0[510] != 0x55 || sec0[511] != 0xAA {
        return Ok(false); // no recognizable partition table
    }
    let mut sec1 = [0u8; 512];
    dev.seek(SeekFrom::Start(SECTOR)).map_err(io)?;
    dev.read_exact(&mut sec1).map_err(io)?;

    if &sec1[0..8] == b"EFI PART" {
        add_awcfg_gpt(dev, device_size_bytes, &json)
    } else if !(0..4).any(|i| sec0[446 + i * 16 + 4] == 0xEE) {
        add_awcfg_mbr(dev, &mut sec0, device_size_bytes, &json)
    } else {
        Ok(false)
    }
}

/// Build a FAT filesystem of `size` bytes in memory containing the config files.
fn build_fat(size_bytes: u64, json: &str) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; size_bytes as usize];
    {
        let mut cur = std::io::Cursor::new(&mut buf[..]);
        fatfs::format_volume(
            &mut cur,
            fatfs::FormatVolumeOptions::new().volume_label(*b"AWCFG      "),
        )
        .map_err(|e| FlashError::Customize(format!("format AWCFG: {e}")))?;
    }
    {
        let cur = std::io::Cursor::new(&mut buf[..]);
        let fs = fatfs::FileSystem::new(cur, fatfs::FsOptions::new())
            .map_err(|e| FlashError::Customize(format!("open AWCFG fs: {e}")))?;
        {
            let root = fs.root_dir();
            write_file(&root, "airwaves-install.json", json.as_bytes())?;
            write_file(&root, "README.txt", README.as_bytes())?;
        }
        fs.unmount().ok();
    }
    Ok(buf)
}

fn write_file<T: fatfs::ReadWriteSeek>(
    root: &fatfs::Dir<T>,
    name: &str,
    data: &[u8],
) -> Result<()> {
    let mut f = root
        .create_file(name)
        .map_err(|e| FlashError::Customize(format!("create {name}: {e}")))?;
    f.truncate().ok();
    f.write_all(data)
        .map_err(|e| FlashError::Customize(format!("write {name}: {e}")))?;
    f.flush()
        .map_err(|e| FlashError::Customize(format!("flush {name}: {e}")))?;
    Ok(())
}

fn add_awcfg_gpt(dev: &mut File, device_size: u64, json: &str) -> Result<bool> {
    let total = device_size / SECTOR;
    if total < 100 {
        return Ok(false);
    }
    let dev_last = total - 1;

    // ---- read primary header + entries -------------------------------------
    let mut hdr = [0u8; 512];
    dev.seek(SeekFrom::Start(SECTOR)).map_err(io)?;
    dev.read_exact(&mut hdr).map_err(io)?;
    let hsize = u32(&hdr, 12) as usize;
    let old_alt = u64v(&hdr, 32);
    let first_usable = u64v(&hdr, 40);
    let entry_lba = u64v(&hdr, 72);
    let num = u32(&hdr, 80);
    let esize = u32(&hdr, 84);
    if !(36..=512).contains(&esize) || num == 0 || num > 256 {
        return Ok(false);
    }
    let arr_bytes = (num * esize) as usize;
    let arr_sectors = (arr_bytes as u64).div_ceil(SECTOR);
    let mut arr = vec![0u8; arr_bytes];
    dev.seek(SeekFrom::Start(entry_lba * SECTOR)).map_err(io)?;
    dev.read_exact(&mut arr).map_err(io)?;

    // existing AWCFG (by partition name) → just rewrite its FAT, no GPT change
    if let Some(i) = (0..num as usize).find(|&i| entry_name(&arr, i * esize as usize) == "AWCFG") {
        let off = i * esize as usize;
        let first = u64v(&arr, off + 32);
        let last = u64v(&arr, off + 40);
        let region = (last - first + 1) * SECTOR;
        let fat = build_fat(region.min(256 * 1024 * 1024), json)?;
        dev.seek(SeekFrom::Start(first * SECTOR)).map_err(io)?;
        dev.write_all(&fat).map_err(io)?;
        return Ok(true);
    }

    // find a free entry slot + the end of the last used partition
    let mut slot = None;
    let mut max_end = first_usable;
    for i in 0..num as usize {
        let off = i * esize as usize;
        if arr[off..off + 16].iter().all(|&b| b == 0) {
            slot.get_or_insert(i);
        } else {
            max_end = max_end.max(u64v(&arr, off + 40) + 1);
        }
    }
    let Some(slot) = slot else { return Ok(false) };

    // ---- geometry: expand usable space, place AWCFG at the tail ------------
    let new_last_usable = dev_last - 1 - arr_sectors; // backup header + backup array
    let awcfg_sectors = AWCFG_BYTES / SECTOR;
    let mut first = (new_last_usable + 1).saturating_sub(awcfg_sectors);
    first -= first % 2048; // 1 MiB align
    if first <= max_end || first >= new_last_usable {
        return Ok(false); // no room
    }
    let last = first + awcfg_sectors - 1;

    // build + write the FAT image into the region
    let fat = build_fat(awcfg_sectors * SECTOR, json)?;
    dev.seek(SeekFrom::Start(first * SECTOR)).map_err(io)?;
    dev.write_all(&fat).map_err(io)?;

    // ---- fill the partition entry ------------------------------------------
    let off = slot * esize as usize;
    arr[off..off + 16].copy_from_slice(&MS_BASIC_DATA);
    let mut uniq = MS_BASIC_DATA;
    uniq[15] ^= (slot as u8).wrapping_add(1);
    uniq[0] ^= 0x5A;
    arr[off + 16..off + 32].copy_from_slice(&uniq);
    put_u64(&mut arr, off + 32, first);
    put_u64(&mut arr, off + 40, last);
    put_u64(&mut arr, off + 48, 0); // attributes
    set_entry_name(&mut arr, off, "AWCFG");
    let arr_crc = crc32fast::hash(&arr);

    // ---- rewrite primary header --------------------------------------------
    put_u64(&mut hdr, 32, dev_last); // alternate_lba → device end
    put_u64(&mut hdr, 48, new_last_usable);
    put_u32(&mut hdr, 88, arr_crc);
    reseal_header(&mut hdr, hsize);
    dev.seek(SeekFrom::Start(SECTOR)).map_err(io)?;
    dev.write_all(&hdr).map_err(io)?;
    dev.seek(SeekFrom::Start(entry_lba * SECTOR)).map_err(io)?;
    dev.write_all(&arr).map_err(io)?;

    // ---- write the backup GPT at the device end ----------------------------
    let backup_arr_lba = dev_last - arr_sectors;
    dev.seek(SeekFrom::Start(backup_arr_lba * SECTOR)).map_err(io)?;
    dev.write_all(&arr).map_err(io)?;
    let mut bhdr = hdr;
    put_u64(&mut bhdr, 24, dev_last); // my_lba
    put_u64(&mut bhdr, 32, 1); // alternate_lba
    put_u64(&mut bhdr, 72, backup_arr_lba); // partition_entry_lba
    reseal_header(&mut bhdr, hsize);
    dev.seek(SeekFrom::Start(dev_last * SECTOR)).map_err(io)?;
    dev.write_all(&bhdr).map_err(io)?;

    // zero the old (now stale) backup header sector to avoid confusing tools
    if old_alt > new_last_usable && old_alt < dev_last {
        dev.seek(SeekFrom::Start(old_alt * SECTOR)).map_err(io)?;
        dev.write_all(&[0u8; 512]).map_err(io)?;
    }
    Ok(true)
}

fn add_awcfg_mbr(dev: &mut File, mbr: &mut [u8; 512], device_size: u64, json: &str) -> Result<bool> {
    let total = device_size / SECTOR;
    let dev_last = total - 1;
    // find a free primary slot + the end of used partitions
    let mut slot = None;
    let mut max_end: u64 = 2048;
    for i in 0..4 {
        let base = 446 + i * 16;
        let ptype = mbr[base + 4];
        let start = u32(mbr, base + 8) as u64;
        let count = u32(mbr, base + 12) as u64;
        if ptype == 0 || count == 0 {
            slot.get_or_insert(i);
        } else {
            max_end = max_end.max(start + count);
        }
    }
    let Some(slot) = slot else { return Ok(false) };

    let awcfg_sectors = AWCFG_BYTES / SECTOR;
    let mut first = (dev_last + 1).saturating_sub(awcfg_sectors);
    first -= first % 2048;
    // MBR LBA fields are 32-bit
    if first <= max_end || first + awcfg_sectors > u32::MAX as u64 {
        return Ok(false);
    }

    let fat = build_fat(awcfg_sectors * SECTOR, json)?;
    dev.seek(SeekFrom::Start(first * SECTOR)).map_err(io)?;
    dev.write_all(&fat).map_err(io)?;

    let base = 446 + slot * 16;
    mbr[base] = 0x00; // not bootable
    mbr[base + 1..base + 4].copy_from_slice(&[0xFE, 0xFF, 0xFF]); // CHS first (LBA-only)
    mbr[base + 4] = 0x0C; // FAT32 LBA
    mbr[base + 5..base + 8].copy_from_slice(&[0xFE, 0xFF, 0xFF]); // CHS last
    put_u32(mbr, base + 8, first as u32);
    put_u32(mbr, base + 12, awcfg_sectors as u32);
    dev.seek(SeekFrom::Start(0)).map_err(io)?;
    dev.write_all(mbr).map_err(io)?;
    Ok(true)
}

// ---- helpers ---------------------------------------------------------------
fn reseal_header(hdr: &mut [u8; 512], hsize: usize) {
    put_u32(hdr, 16, 0);
    let crc = crc32fast::hash(&hdr[..hsize.min(512)]);
    put_u32(hdr, 16, crc);
}
fn entry_name(arr: &[u8], off: usize) -> String {
    let mut s = String::new();
    let mut i = off + 56;
    while i + 1 < off + 128 {
        let c = u16::from_le_bytes([arr[i], arr[i + 1]]);
        if c == 0 {
            break;
        }
        if let Some(ch) = char::from_u32(c as u32) {
            s.push(ch);
        }
        i += 2;
    }
    s
}
fn set_entry_name(arr: &mut [u8], off: usize, name: &str) {
    for b in arr[off + 56..off + 128].iter_mut() {
        *b = 0;
    }
    let mut i = off + 56;
    for u in name.encode_utf16() {
        if i + 1 >= off + 128 {
            break;
        }
        arr[i..i + 2].copy_from_slice(&u.to_le_bytes());
        i += 2;
    }
}
fn u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn u64v(b: &[u8], o: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    u64::from_le_bytes(a)
}
fn put_u32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
fn put_u64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}
fn io(e: std::io::Error) -> FlashError {
    FlashError::Customize(e.to_string())
}

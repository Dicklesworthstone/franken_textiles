//! A ZIP archive in memory (deflated entries, no extra fields): the page's "download everything".

/// CRC-32 (IEEE).
fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (n, t) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 == 1 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *t = c;
    }
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c = table[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

/// MS-DOS time and date of `YYYY-MM-DDTHH:MM` (1980-01-01 00:00 when it does not parse).
fn dos_time(stamp: &str) -> (u16, u16) {
    let num = |a: usize, b: usize| stamp.get(a..b).and_then(|s| s.parse::<u16>().ok());
    match (num(0, 4), num(5, 7), num(8, 10), num(11, 13), num(14, 16)) {
        (Some(y), Some(mo), Some(d), Some(h), Some(mi)) if y >= 1980 => {
            ((h << 11) | (mi << 5), ((y - 1980) << 9) | (mo << 5) | d)
        }
        _ => (0, (1 << 5) | 1),
    }
}

fn u16le(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, v: usize) {
    out.extend_from_slice(&u32::try_from(v).unwrap_or(u32::MAX).to_le_bytes());
}

/// The archive of `files` (`/`-separated names, in the given order), stamped `stamp`.
#[must_use]
pub fn zip(files: &[(String, Vec<u8>)], stamp: &str) -> Vec<u8> {
    let (time, date) = dos_time(stamp);
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let crc = crc32(data);
        let packed = miniz_oxide::deflate::compress_to_vec(data, 6);
        let offset = out.len();
        for (header, sig) in [(&mut out, 0x0403_4b50usize), (&mut central, 0x0201_4b50)] {
            u32le(header, sig);
            if sig == 0x0201_4b50 {
                u16le(header, 20); // made by: MS-DOS, spec 2.0
            }
            u16le(header, 20); // needed: deflate
            u16le(header, 0x0800); // UTF-8 names
            u16le(header, 8);
            u16le(header, time);
            u16le(header, date);
            u32le(header, crc as usize);
            u32le(header, packed.len());
            u32le(header, data.len());
            u16le(header, u16::try_from(name.len()).unwrap_or(u16::MAX));
            u16le(header, 0); // extra
            if sig == 0x0201_4b50 {
                u16le(header, 0); // comment
                u16le(header, 0); // disk
                u16le(header, 0); // internal attributes
                u32le(header, 0); // external attributes
                u32le(header, offset);
            }
            header.extend_from_slice(name.as_bytes());
        }
        out.extend_from_slice(&packed);
    }
    let (cd_offset, cd_len) = (out.len(), central.len());
    out.extend_from_slice(&central);
    u32le(&mut out, 0x0605_4b50);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    u16le(&mut out, u16::try_from(files.len()).unwrap_or(u16::MAX));
    u16le(&mut out, u16::try_from(files.len()).unwrap_or(u16::MAX));
    u32le(&mut out, cd_len);
    u32le(&mut out, cd_offset);
    u16le(&mut out, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_and_layout() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        let z = zip(
            &[("a.txt".into(), b"hello hello hello".to_vec())],
            "2026-10-13T12:00",
        );
        assert_eq!(&z[..4], &[0x50, 0x4b, 0x03, 0x04]);
        assert_eq!(&z[z.len() - 22..z.len() - 18], &[0x50, 0x4b, 0x05, 0x06]);
        let (t, d) = dos_time("2026-10-13T12:00");
        assert_eq!((t >> 11, (t >> 5) & 63), (12, 0));
        assert_eq!((1980 + (d >> 9), (d >> 5) & 15, d & 31), (2026, 10, 13));
    }
}

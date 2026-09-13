use anyhow::{bail, ensure, Context, Result};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    X64,
    Arm64,
}

#[derive(Debug)]
pub struct Image {
    pub arch: Arch,
    pub ranges: Vec<Range<usize>>,
}

fn arch(machine: u64) -> Result<Arch> {
    match machine {
        0x8664 | 0x3e | 0x01000007 => Ok(Arch::X64),
        0xaa64 | 0xb7 | 0x0100000c => Ok(Arch::Arm64),
        _ => bail!("Unsupported architecture: {machine:#x}"),
    }
}

fn number(data: &[u8], offset: usize, width: usize, big: bool) -> Result<usize> {
    let end = offset.checked_add(width).context("Header overflow")?;
    let bytes = data
        .get(offset..end)
        .context("Truncated executable header")?;
    let mut result = 0u64;
    for i in 0..width {
        let byte = bytes[if big { i } else { width - 1 - i }];
        result = (result << 8) | u64::from(byte);
    }
    usize::try_from(result).context("Executable offset is too large")
}

fn ranges(mut ranges: Vec<Range<usize>>, length: usize) -> Result<Vec<Range<usize>>> {
    for range in &ranges {
        ensure!(
            range.start <= range.end && range.end <= length,
            "Invalid executable range"
        );
    }
    ranges.retain(|r| !r.is_empty());
    ranges.sort_by_key(|r| r.start);
    let mut result: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        if let Some(last) = result.last_mut() {
            if range.start <= last.end {
                last.end = last.end.max(range.end);
                continue;
            }
        }
        result.push(range);
    }
    ensure!(!result.is_empty(), "No executable ranges");
    Ok(result)
}

fn range(start: usize, size: usize) -> Result<Range<usize>> {
    Ok(start..start.checked_add(size).context("Range overflow")?)
}

fn elf(data: &[u8]) -> Result<Image> {
    ensure!(data.get(4) == Some(&2), "Only 64-bit ELF is supported");
    let big = match data.get(5) {
        Some(1) => false,
        Some(2) => true,
        _ => bail!("Invalid ELF byte order"),
    };
    let n = |off, width| number(data, off, width, big);
    let arch = arch(n(18, 2)? as u64)?;
    let offset = n(32, 8)?;
    let size = n(54, 2)?;
    let count = n(56, 2)?;
    ensure!(size >= 56 && count <= 65535, "Invalid ELF program table");
    let mut result = Vec::new();
    for i in 0..count {
        let p = offset
            .checked_add(i * size)
            .context("Program table overflow")?;
        ensure!(
            p <= data.len().saturating_sub(56),
            "Truncated ELF program table"
        );
        if n(p, 4)? == 1 && n(p + 4, 4)? & 1 != 0 {
            result.push(range(n(p + 8, 8)?, n(p + 32, 8)?)?);
        }
    }
    Ok(Image {
        arch,
        ranges: ranges(result, data.len())?,
    })
}

fn pe(data: &[u8]) -> Result<Image> {
    let n = |off, width| number(data, off, width, false);
    let p = n(0x3c, 4)?;
    ensure!(
        data.get(p..p.saturating_add(4)) == Some(b"PE\0\0"),
        "Invalid PE signature"
    );
    ensure!(p <= data.len().saturating_sub(24), "Truncated PE header");
    let arch = arch(n(p + 4, 2)? as u64)?;
    let count = n(p + 6, 2)?;
    let table = p
        .checked_add(24 + n(p + 20, 2)?)
        .context("PE table overflow")?;
    let mut result = Vec::new();
    for i in 0..count {
        let p = table.checked_add(i * 40).context("PE section overflow")?;
        ensure!(p <= data.len().saturating_sub(40), "Truncated PE section");
        if n(p + 36, 4)? & 0x20000000 != 0 {
            result.push(range(n(p + 20, 4)?, n(p + 16, 4)?)?);
        }
    }
    Ok(Image {
        arch,
        ranges: ranges(result, data.len())?,
    })
}

fn macho(data: &[u8], base: usize) -> Result<Image> {
    let big = match data.get(..4) {
        Some(b"\xcf\xfa\xed\xfe") => false,
        Some(b"\xfe\xed\xfa\xcf") => true,
        _ => bail!("Unsupported Mach-O slice"),
    };
    let n = |off, width| number(data, off, width, big);
    let arch = arch(n(4, 4)? as u64)?;
    let count = n(16, 4)?;
    let commands_end = 32usize
        .checked_add(n(20, 4)?)
        .context("Mach-O command overflow")?;
    ensure!(
        commands_end <= data.len() && count <= data.len() / 8,
        "Invalid Mach-O command table"
    );
    let mut p = 32usize;
    let mut result = Vec::new();
    for _ in 0..count {
        let command = n(p, 4)?;
        let size = n(p + 4, 4)?;
        ensure!(
            size >= 8 && p.checked_add(size).is_some_and(|end| end <= commands_end),
            "Invalid Mach-O command"
        );
        if command == 0x19 {
            ensure!(size >= 72, "Invalid Mach-O segment");
            let count = n(p + 64, 4)?;
            ensure!(count <= (size - 72) / 80, "Invalid Mach-O sections");
            for i in 0..count {
                let s = p + 72 + i * 80;
                let name = &data[s..s + 16];
                let segment = &data[s + 16..s + 32];
                let flags = n(s + 64, 4)?;
                if (name.starts_with(b"__text\0") && segment.starts_with(b"__TEXT\0"))
                    || flags & 0x80000400 != 0
                {
                    result.push(range(n(s + 48, 4)?, n(s + 40, 8)?)?);
                }
            }
        }
        p += size;
    }
    let mut normalized = ranges(result, data.len())?;
    for r in &mut normalized {
        r.start += base;
        r.end += base;
    }
    Ok(Image {
        arch,
        ranges: normalized,
    })
}

pub fn executable_images(data: &[u8]) -> Result<Vec<Image>> {
    if data.starts_with(b"\x7fELF") {
        return Ok(vec![elf(data)?]);
    }
    if data.starts_with(b"MZ") {
        return Ok(vec![pe(data)?]);
    }
    if data.starts_with(b"\xcf\xfa\xed\xfe") || data.starts_with(b"\xfe\xed\xfa\xcf") {
        return Ok(vec![macho(data, 0)?]);
    }
    let (big, wide) = match data.get(..4) {
        Some(b"\xca\xfe\xba\xbe") => (true, false),
        Some(b"\xbe\xba\xfe\xca") => (false, false),
        Some(b"\xca\xfe\xba\xbf") => (true, true),
        Some(b"\xbf\xba\xfe\xca") => (false, true),
        _ => bail!("Unsupported executable format"),
    };
    let count = number(data, 4, 4, big)?;
    ensure!(
        count > 0 && count <= 64,
        "Invalid universal Mach-O slice count"
    );
    let mut images = Vec::new();
    let mut occupied: Vec<Range<usize>> = Vec::new();
    for i in 0..count {
        let entry = 8 + i * if wide { 32 } else { 20 };
        let width = if wide { 8 } else { 4 };
        let start = number(data, entry + 8, width, big)?;
        let size = number(data, entry + 8 + width, width, big)?;
        let slice = range(start, size)?;
        ensure!(
            start >= 8 + count * if wide { 32 } else { 20 },
            "Slice overlaps Mach-O header"
        );
        ensure!(
            !occupied
                .iter()
                .any(|r| slice.start < r.end && r.start < slice.end),
            "Overlapping Mach-O slices"
        );
        let image = macho(
            data.get(slice.clone()).context("Truncated Mach-O slice")?,
            start,
        )?;
        ensure!(
            image.arch == arch(number(data, entry, 4, big)? as u64)?,
            "Mach-O architecture mismatch"
        );
        images.push(image);
        occupied.push(slice);
    }
    Ok(images)
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn elf_fixture(code: &[u8], machine: u16) -> Vec<u8> {
        let mut b = vec![0u8; 512 + code.len()];
        b[..6].copy_from_slice(b"\x7fELF\x02\x01");
        b[18..20].copy_from_slice(&machine.to_le_bytes());
        b[32..40].copy_from_slice(&64u64.to_le_bytes());
        b[54..56].copy_from_slice(&56u16.to_le_bytes());
        b[56..58].copy_from_slice(&1u16.to_le_bytes());
        b[64..68].copy_from_slice(&1u32.to_le_bytes());
        b[68..72].copy_from_slice(&1u32.to_le_bytes());
        b[72..80].copy_from_slice(&512u64.to_le_bytes());
        b[96..104].copy_from_slice(&(code.len() as u64).to_le_bytes());
        b[512..].copy_from_slice(code);
        b
    }
    #[test]
    fn elf_bounds() {
        let mut b = elf_fixture(&[1, 2, 3], 0x3e);
        assert_eq!(executable_images(&b).unwrap()[0].ranges, vec![512..515]);
        b.truncate(514);
        assert!(executable_images(&b).is_err());
    }
    #[test]
    fn malformed_headers_do_not_panic() {
        for size in 0..128 {
            let b = vec![255; size];
            assert!(executable_images(&b).is_err());
        }
    }
}

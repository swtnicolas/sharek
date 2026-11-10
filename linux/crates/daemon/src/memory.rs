pub fn readonly_executable_ranges(maps: &str, executable: &str) -> Vec<(usize, usize)> {
    maps.lines()
        .filter(|line| line.ends_with(executable))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (range, permissions) = (fields.next()?, fields.next()?);
            if permissions.contains('w') {
                return None;
            }
            let (start, end) = range.split_once('-')?;
            Some((usize::from_str_radix(start, 16).ok()?, usize::from_str_radix(end, 16).ok()?))
        })
        .collect()
}

pub fn release() {
    unsafe {
        libc::malloc_trim(0);
    }
    let (Ok(executable), Ok(maps)) = (std::fs::read_link("/proc/self/exe"), std::fs::read_to_string("/proc/self/maps")) else { return };
    let Some(executable) = executable.to_str() else { return };
    for (start, end) in readonly_executable_ranges(&maps, executable) {
        unsafe {
            libc::madvise(start as *mut libc::c_void, end - start, libc::MADV_PAGEOUT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAPS: &str = "\
5652a0000000-5652a0080000 r--p 00000000 00:21 123 /home/me/.local/bin/sharek
5652a0080000-5652a0300000 r-xp 00080000 00:21 123 /home/me/.local/bin/sharek
5652a0300000-5652a0320000 r--p 00300000 00:21 123 /home/me/.local/bin/sharek
5652a0320000-5652a0322000 rw-p 00320000 00:21 123 /home/me/.local/bin/sharek
7f0000000000-7f0000100000 r-xp 00000000 00:21 456 /usr/lib/libc.so.6
7f0000200000-7f0000300000 rw-p 00000000 00:00 0
";

    #[test]
    fn only_the_executables_readonly_segments_are_released() {
        assert_eq!(
            readonly_executable_ranges(MAPS, "/home/me/.local/bin/sharek"),
            vec![(0x5652a0000000, 0x5652a0080000), (0x5652a0080000, 0x5652a0300000), (0x5652a0300000, 0x5652a0320000)]
        );
    }
}

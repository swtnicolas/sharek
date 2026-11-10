use std::fmt;
use std::ops::{Deref, DerefMut};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::ptr::NonNull;
use std::slice;
use std::sync::{Mutex, OnceLock, PoisonError};

const DISK_THRESHOLD: usize = 1 << 20;
const DISK_RESERVE: u64 = 1 << 30;

static DIRECTORY: OnceLock<Option<OwnedFd>> = OnceLock::new();
static LIVE: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());

pub struct Bytes(Store);

enum Store {
    Heap(Vec<u8>),
    File(Mapping),
}

struct Mapping {
    pointer: NonNull<u8>,
    length: usize,
}

unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

fn directory() -> Option<&'static OwnedFd> {
    DIRECTORY
        .get_or_init(|| {
            let path = crate::paths::cache_dir();
            std::fs::create_dir_all(&path).ok()?;
            let directory = OwnedFd::from(std::fs::File::open(&path).ok()?);
            let mut volume: libc::statfs = unsafe { std::mem::zeroed() };
            if unsafe { libc::fstatfs(directory.as_raw_fd(), &mut volume) } != 0 {
                return None;
            }
            (volume.f_type != libc::TMPFS_MAGIC).then_some(directory)
        })
        .as_ref()
}

impl Mapping {
    fn new(length: usize) -> Option<Mapping> {
        let directory = directory()?.as_raw_fd();
        let mut volume: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstatvfs(directory, &mut volume) } != 0 {
            return None;
        }
        let free = (volume.f_bavail as u64).saturating_mul(volume.f_frsize as u64);
        if free < length as u64 + DISK_RESERVE {
            return None;
        }
        let file = unsafe { libc::openat(directory, c".".as_ptr(), libc::O_TMPFILE | libc::O_RDWR | libc::O_CLOEXEC, 0o600 as libc::c_uint) };
        if file < 0 {
            return None;
        }
        let file = unsafe { OwnedFd::from_raw_fd(file) };
        if unsafe { libc::ftruncate(file.as_raw_fd(), length as libc::off_t) } != 0 {
            return None;
        }
        let address = unsafe { libc::mmap(std::ptr::null_mut(), length, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, file.as_raw_fd(), 0) };
        if address == libc::MAP_FAILED {
            return None;
        }
        let pointer = NonNull::new(address.cast::<u8>())?;
        LIVE.lock().unwrap_or_else(PoisonError::into_inner).push((address as usize, length));
        Some(Mapping { pointer, length })
    }

    fn holding(data: &[u8]) -> Option<Mapping> {
        let mut mapping = Mapping::new(data.len())?;
        mapping.as_mut_slice().copy_from_slice(data);
        Some(mapping)
    }

    fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.pointer.as_ptr(), self.length) }
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.pointer.as_ptr(), self.length) }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        let address = self.pointer.as_ptr() as usize;
        LIVE.lock().unwrap_or_else(PoisonError::into_inner).retain(|entry| entry.0 != address);
        unsafe {
            libc::munmap(self.pointer.as_ptr().cast(), self.length);
        }
    }
}

impl Bytes {
    pub fn on_disk(self) -> Bytes {
        match self.0 {
            Store::Heap(data) if data.len() >= DISK_THRESHOLD => match Mapping::holding(&data) {
                Some(mapping) => Bytes(Store::File(mapping)),
                None => Bytes(Store::Heap(data)),
            },
            store => Bytes(store),
        }
    }
}

pub fn page_out() {
    for &(address, length) in LIVE.lock().unwrap_or_else(PoisonError::into_inner).iter() {
        unsafe {
            libc::madvise(address as *mut libc::c_void, length, libc::MADV_PAGEOUT);
        }
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(data: Vec<u8>) -> Bytes {
        Bytes(Store::Heap(data))
    }
}

impl Deref for Bytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match &self.0 {
            Store::Heap(data) => data,
            Store::File(mapping) => mapping.as_slice(),
        }
    }
}

impl DerefMut for Bytes {
    fn deref_mut(&mut self) -> &mut [u8] {
        match &mut self.0 {
            Store::Heap(data) => data,
            Store::File(mapping) => mapping.as_mut_slice(),
        }
    }
}

impl Clone for Bytes {
    fn clone(&self) -> Bytes {
        match &self.0 {
            Store::Heap(data) => Bytes(Store::Heap(data.clone())),
            Store::File(mapping) => match Mapping::holding(mapping.as_slice()) {
                Some(copy) => Bytes(Store::File(copy)),
                None => Bytes(Store::Heap(mapping.as_slice().to_vec())),
            },
        }
    }
}

impl PartialEq for Bytes {
    fn eq(&self, other: &Bytes) -> bool {
        **self == **other
    }
}

impl Eq for Bytes {}

impl fmt::Debug for Bytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterned(length: usize) -> Vec<u8> {
        (0..length).map(|index| (index % 251) as u8).collect()
    }

    fn on_disk(bytes: &Bytes) -> bool {
        matches!(bytes.0, Store::File(_))
    }

    fn mapped_as_file(bytes: &Bytes) -> bool {
        let start = format!("{:x}-", bytes.as_ptr() as usize);
        let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
        maps.lines().any(|line| line.starts_with(&start) && line.contains(" rw-s ") && line.ends_with("(deleted)"))
    }

    #[test]
    fn a_buffer_under_the_threshold_stays_on_the_heap() {
        let bytes = Bytes::from(patterned(DISK_THRESHOLD - 1)).on_disk();
        assert!(!on_disk(&bytes));
        assert_eq!(*bytes, *patterned(DISK_THRESHOLD - 1));
    }

    #[test]
    fn a_large_buffer_becomes_a_shared_mapping_of_an_unnamed_file_with_the_same_bytes() {
        let bytes = Bytes::from(patterned(DISK_THRESHOLD + 5)).on_disk();
        assert_eq!(on_disk(&bytes), directory().is_some());
        assert_eq!(mapped_as_file(&bytes), directory().is_some());
        assert_eq!(*bytes, *patterned(DISK_THRESHOLD + 5));
    }

    #[test]
    fn paging_out_keeps_the_bytes() {
        let bytes = Bytes::from(patterned(2 * DISK_THRESHOLD)).on_disk();
        page_out();
        assert_eq!(*bytes, *patterned(2 * DISK_THRESHOLD));
    }

    #[test]
    fn a_clone_of_a_buffer_on_disk_has_its_own_bytes() {
        let original = Bytes::from(patterned(DISK_THRESHOLD)).on_disk();
        let mut copy = original.clone();
        copy[0] = 200;
        assert_eq!(on_disk(&copy), on_disk(&original));
        assert_eq!(original[0], 0);
        assert_eq!(copy[1..], original[1..]);
    }

    #[test]
    fn a_dropped_buffer_is_unmapped_and_forgotten() {
        let bytes = Bytes::from(patterned(DISK_THRESHOLD)).on_disk();
        let address = bytes.as_ptr() as usize;
        let tracked = |address: usize| LIVE.lock().unwrap().iter().any(|entry| entry.0 == address);
        assert_eq!(tracked(address), directory().is_some());
        drop(bytes);
        assert!(!tracked(address));
    }
}

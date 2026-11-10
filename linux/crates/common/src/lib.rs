pub mod history;
pub mod image;
pub mod ipc;
pub mod mapped;
pub mod ocr;
pub mod paths;
pub mod portal;
pub mod region_label;
pub mod settings;
pub mod swirl;
pub mod text;
pub mod time;

use std::io::Write;
use std::path::Path;

pub fn write_atomically(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension("tmp");
    let mut file = std::fs::File::create(&temporary)?;
    file.write_all(data)?;
    file.sync_data()?;
    std::fs::rename(&temporary, path)
}

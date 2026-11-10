use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::path::PathBuf;

pub const FD_VARIABLE: &str = "SHAREK_IPC_FD";
pub const HISTORY_TITLE: &str = "Historial";
const HEADER_LIMIT: u32 = 1 << 20;
const PAYLOAD_LIMIT: u64 = 1 << 31;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ToEditor {
    Open { id: u64, width: u32, height: u32, scale: f64, created: i64, saved: Option<PathBuf> },
    OpenMedia { id: u64, path: PathBuf, gif: bool, width: f64, height: f64, created: i64, saved_automatically: bool },
    ShowHistory,
    ClipboardOwner { id: Option<u64> },
    PasteImage { scale: f64 },
    ExitAccepted,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ToDaemon {
    Copy { id: u64 },
    Changed { id: u64 },
    Discard { id: u64 },
    Release { id: u64 },
    CopyFile { id: u64, path: PathBuf, gif: bool },
    CopyFiles { paths: Vec<PathBuf> },
    CopyText { text: String },
    Pin { width: u32, height: u32, scale: f64 },
    Toast { title: String, detail: Option<String> },
    PasteRequest,
    Activate { history: bool },
    Exiting { received: u64 },
}

pub fn send<W: Write, M: Serialize>(writer: &mut W, message: &M, payload: &[u8]) -> io::Result<()> {
    let header = serde_json::to_vec(message).map_err(io::Error::other)?;
    writer.write_all(&(header.len() as u32).to_le_bytes())?;
    writer.write_all(&header)?;
    writer.write_all(&(payload.len() as u64).to_le_bytes())?;
    writer.write_all(payload)?;
    writer.flush()
}

pub fn receive<R: Read, M: DeserializeOwned>(reader: &mut R) -> io::Result<(M, Vec<u8>)> {
    let mut length = [0u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length);
    if length > HEADER_LIMIT {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "header too large"));
    }
    let mut header = vec![0u8; length as usize];
    reader.read_exact(&mut header)?;
    let mut size = [0u8; 8];
    reader.read_exact(&mut size)?;
    let size = u64::from_le_bytes(size);
    if size > PAYLOAD_LIMIT {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "payload too large"));
    }
    let mut payload = vec![0u8; size as usize];
    reader.read_exact(&mut payload)?;
    let message = serde_json::from_slice(&header).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok((message, payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip_with_their_payload() {
        let mut buffer = Vec::new();
        send(&mut buffer, &ToDaemon::Pin { width: 2, height: 1, scale: 1.5 }, &[1, 2, 3, 4, 5, 6]).unwrap();
        send(&mut buffer, &ToDaemon::Exiting { received: 3 }, &[]).unwrap();
        let mut reader = buffer.as_slice();
        let (first, payload): (ToDaemon, Vec<u8>) = receive(&mut reader).unwrap();
        assert!(matches!(first, ToDaemon::Pin { width: 2, height: 1, .. }));
        assert_eq!(payload, vec![1, 2, 3, 4, 5, 6]);
        let (second, payload): (ToDaemon, Vec<u8>) = receive(&mut reader).unwrap();
        assert!(matches!(second, ToDaemon::Exiting { received: 3 }) && payload.is_empty());
        assert!(receive::<_, ToDaemon>(&mut reader).is_err());
    }
}

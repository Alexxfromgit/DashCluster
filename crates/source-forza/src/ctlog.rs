//! `.ctlog` — a minimal timestamped raw-packet log, so sessions can be recorded on the
//! gaming PC and replayed anywhere (tests, demos, development without the game).
//!
//! Layout: `b"CTLOG\x01"`, u16 LE length + UTF-8 source tag (e.g. "forza"), then records
//! of `u64 LE microseconds since start, u16 LE length, payload`.

use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

const MAGIC: &[u8; 6] = b"CTLOG\x01";

pub struct Writer {
    out: BufWriter<std::fs::File>,
    start: Instant,
}

impl Writer {
    pub fn create(path: &Path, source: &str) -> std::io::Result<Writer> {
        let mut out = BufWriter::new(std::fs::File::create(path)?);
        out.write_all(MAGIC)?;
        out.write_all(&(source.len() as u16).to_le_bytes())?;
        out.write_all(source.as_bytes())?;
        Ok(Writer { out, start: Instant::now() })
    }

    pub fn write(&mut self, payload: &[u8]) -> std::io::Result<()> {
        self.write_at(self.start.elapsed(), payload)
    }

    pub fn write_at(&mut self, t: Duration, payload: &[u8]) -> std::io::Result<()> {
        self.out.write_all(&(t.as_micros() as u64).to_le_bytes())?;
        self.out.write_all(&(payload.len() as u16).to_le_bytes())?;
        self.out.write_all(payload)
    }

    pub fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

#[derive(Debug, Clone)]
pub struct Record {
    pub t: Duration,
    pub payload: Vec<u8>,
}

/// Reads a whole log: (source tag, records).
pub fn read(path: &Path) -> anyhow::Result<(String, Vec<Record>)> {
    let mut r = BufReader::new(std::fs::File::open(path)?);
    let mut magic = [0u8; 6];
    r.read_exact(&mut magic)?;
    anyhow::ensure!(&magic == MAGIC, "{} is not a .ctlog file", path.display());
    let mut len = [0u8; 2];
    r.read_exact(&mut len)?;
    let mut tag = vec![0u8; u16::from_le_bytes(len) as usize];
    r.read_exact(&mut tag)?;
    let mut records = Vec::new();
    loop {
        let mut t = [0u8; 8];
        match r.read_exact(&mut t) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        r.read_exact(&mut len)?;
        let mut payload = vec![0u8; u16::from_le_bytes(len) as usize];
        if r.read_exact(&mut payload).is_err() {
            break; // truncated last record (e.g. app killed mid-write)
        }
        records.push(Record { t: Duration::from_micros(u64::from_le_bytes(t)), payload });
    }
    Ok((String::from_utf8_lossy(&tag).into_owned(), records))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read() {
        let dir = std::env::temp_dir().join(format!("ctlog-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.ctlog");
        let mut w = Writer::create(&p, "forza").unwrap();
        w.write_at(Duration::from_millis(5), b"hello").unwrap();
        w.write_at(Duration::from_millis(21), &[1, 2, 3]).unwrap();
        w.flush().unwrap();
        drop(w);
        let (tag, recs) = read(&p).unwrap();
        assert_eq!(tag, "forza");
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[1].t, Duration::from_millis(21));
        assert_eq!(recs[0].payload, b"hello");
        std::fs::remove_dir_all(dir).ok();
    }
}

//! `.ctlog` — a minimal timestamped raw-packet log, so sessions can be recorded on the
//! gaming PC and replayed anywhere (tests, demos, development without the game).
//!
//! Layout: `b"CTLOG\x01"`, u16 LE length + UTF-8 source tag (e.g. "forza"), then records
//! of `u64 LE microseconds since start, u16 LE length, payload`.

use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAGIC: &[u8; 6] = b"CTLOG\x01";

pub struct Writer {
    out: BufWriter<std::fs::File>,
    start: Instant,
}

/// A recorder shared by every run of a source, so restarting the source (switching to the
/// demo car and back, changing settings) keeps appending to the same recording.
pub type SharedWriter = Arc<Mutex<Writer>>;

impl Writer {
    pub fn create(path: &Path, source: &str) -> std::io::Result<Writer> {
        let mut out = BufWriter::new(std::fs::File::create(path)?);
        out.write_all(MAGIC)?;
        out.write_all(&(source.len() as u16).to_le_bytes())?;
        out.write_all(source.as_bytes())?;
        Ok(Writer { out, start: Instant::now() })
    }

    pub fn shared(path: &Path, source: &str) -> std::io::Result<SharedWriter> {
        Ok(Arc::new(Mutex::new(Writer::create(path, source)?)))
    }

    pub fn write(&mut self, payload: &[u8]) -> std::io::Result<()> {
        self.write_at(self.start.elapsed(), payload)
    }

    pub fn write_at(&mut self, t: Duration, payload: &[u8]) -> std::io::Result<()> {
        let len = u16::try_from(payload.len()).map_err(|_| std::io::Error::other("packet too large to record"))?;
        self.out.write_all(&(t.as_micros() as u64).to_le_bytes())?;
        self.out.write_all(&len.to_le_bytes())?;
        self.out.write_all(payload)
    }

    pub fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        let _ = self.out.flush();
    }
}

#[derive(Debug, Clone)]
pub struct Record {
    pub t: Duration,
    pub payload: Vec<u8>,
}

/// Streams the records of a log without loading it into memory.
pub struct Reader {
    r: BufReader<std::fs::File>,
    pub tag: String,
}

impl Reader {
    pub fn open(path: &Path) -> anyhow::Result<Reader> {
        let mut r = BufReader::new(std::fs::File::open(path)?);
        let mut magic = [0u8; 6];
        r.read_exact(&mut magic).map_err(|_| anyhow::anyhow!("{} is not a .ctlog file", path.display()))?;
        anyhow::ensure!(&magic == MAGIC, "{} is not a .ctlog file", path.display());
        let mut len = [0u8; 2];
        r.read_exact(&mut len)?;
        let mut tag = vec![0u8; u16::from_le_bytes(len) as usize];
        r.read_exact(&mut tag)?;
        Ok(Reader { r, tag: String::from_utf8_lossy(&tag).into_owned() })
    }

    /// The next record; `None` at the end, including a record cut off mid-write (app killed
    /// while recording).
    pub fn next_record(&mut self) -> std::io::Result<Option<Record>> {
        let mut t = [0u8; 8];
        let mut len = [0u8; 2];
        for buf in [&mut t[..], &mut len[..]] {
            match self.r.read_exact(buf) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(e) => return Err(e),
            }
        }
        let mut payload = vec![0u8; u16::from_le_bytes(len) as usize];
        match self.r.read_exact(&mut payload) {
            Ok(()) => Ok(Some(Record { t: Duration::from_micros(u64::from_le_bytes(t)), payload })),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Reads a whole log: (source tag, records). Fine for tests and short recordings; replay
/// streams with [`Reader`].
pub fn read(path: &Path) -> anyhow::Result<(String, Vec<Record>)> {
    let mut r = Reader::open(path)?;
    let mut records = Vec::new();
    while let Some(rec) = r.next_record()? {
        records.push(rec);
    }
    Ok((r.tag, records))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ctlog-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("a.ctlog")
    }

    #[test]
    fn write_then_read() {
        let p = temp("rw");
        let mut w = Writer::create(&p, "forza").unwrap();
        w.write_at(Duration::from_millis(5), b"hello").unwrap();
        w.write_at(Duration::from_millis(21), &[1, 2, 3]).unwrap();
        drop(w);
        let (tag, recs) = read(&p).unwrap();
        assert_eq!(tag, "forza");
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[1].t, Duration::from_millis(21));
        assert_eq!(recs[0].payload, b"hello");
    }

    #[test]
    fn truncated_files_keep_the_complete_records() {
        let p = temp("trunc");
        let mut w = Writer::create(&p, "forza").unwrap();
        w.write_at(Duration::from_millis(1), b"one").unwrap();
        w.write_at(Duration::from_millis(2), b"two").unwrap();
        drop(w);
        let full = std::fs::read(&p).unwrap();
        // Cut after the second record's timestamp, inside its length, and inside its payload.
        let second = full.len() - (8 + 2 + 3);
        for cut in [second + 8, second + 9, second + 11] {
            std::fs::write(&p, &full[..cut]).unwrap();
            let (_, recs) = read(&p).unwrap();
            assert_eq!(recs.len(), 1, "cut at {cut}");
        }
    }
}

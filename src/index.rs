use std::ffi::OsString;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

const MAGIC: &[u8; 8] = b"KEEPIDX2";

pub struct Entry {
    pub path: PathBuf,
    pub size: u64,
    pub inode: Inode,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Inode {
    pub device: u64,
    pub number: u64,
}

pub struct Index {
    pub host: String,
    pub roots: Vec<PathBuf>,
    pub entries: Vec<Entry>,
    pub unreadable: Vec<String>,
}

impl Index {
    pub fn write(&self, path: &Path) -> Result<()> {
        let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
        let mut writer = BufWriter::new(zstd::Encoder::new(file, 9)?.auto_finish());
        writer.write_all(MAGIC)?;
        write_bytes(&mut writer, self.host.as_bytes())?;
        write_count(&mut writer, self.roots.len())?;
        for root in &self.roots {
            write_bytes(&mut writer, root.as_os_str().as_bytes())?;
        }
        write_count(&mut writer, self.entries.len())?;
        for entry in &self.entries {
            writer.write_all(&entry.size.to_le_bytes())?;
            writer.write_all(&entry.inode.device.to_le_bytes())?;
            writer.write_all(&entry.inode.number.to_le_bytes())?;
            write_bytes(&mut writer, entry.path.as_os_str().as_bytes())?;
        }
        write_count(&mut writer, self.unreadable.len())?;
        for message in &self.unreadable {
            write_bytes(&mut writer, message.as_bytes())?;
        }
        writer.flush()?;
        Ok(())
    }

    pub fn read(path: &Path) -> Result<Index> {
        let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut reader = BufReader::new(zstd::Decoder::new(file)?);
        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic)?;
        ensure!(&magic == MAGIC, "{} is not a keep index", path.display());
        let host = String::from_utf8(read_bytes(&mut reader)?)?;
        let roots = (0..read_count(&mut reader)?)
            .map(|_| read_path(&mut reader))
            .collect::<Result<Vec<_>>>()?;
        let entries = (0..read_count(&mut reader)?)
            .map(|_| {
                let size = read_u64(&mut reader)?;
                let inode = Inode {
                    device: read_u64(&mut reader)?,
                    number: read_u64(&mut reader)?,
                };
                Ok(Entry {
                    size,
                    inode,
                    path: read_path(&mut reader)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let unreadable = (0..read_count(&mut reader)?)
            .map(|_| Ok(String::from_utf8(read_bytes(&mut reader)?)?))
            .collect::<Result<Vec<_>>>()?;
        Ok(Index {
            host,
            roots,
            entries,
            unreadable,
        })
    }
}

fn write_count(writer: &mut impl Write, count: usize) -> Result<()> {
    writer.write_all(&(count as u64).to_le_bytes())?;
    Ok(())
}

fn write_bytes(writer: &mut impl Write, bytes: &[u8]) -> Result<()> {
    write_count(writer, bytes.len())?;
    writer.write_all(bytes)?;
    Ok(())
}

fn read_u64(reader: &mut impl Read) -> Result<u64> {
    let mut value = [0u8; 8];
    reader.read_exact(&mut value)?;
    Ok(u64::from_le_bytes(value))
}

fn read_count(reader: &mut impl Read) -> Result<usize> {
    Ok(usize::try_from(read_u64(reader)?)?)
}

fn read_bytes(reader: &mut impl Read) -> Result<Vec<u8>> {
    let mut bytes = vec![0u8; read_count(reader)?];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_path(reader: &mut impl Read) -> Result<PathBuf> {
    Ok(PathBuf::from(OsString::from_vec(read_bytes(reader)?)))
}

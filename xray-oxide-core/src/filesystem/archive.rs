use std::{
    fs::File,
    hash::{Hash, Hasher},
    io::{BufReader, Cursor, Read, Seek},
    mem::size_of,
    path::{Path, PathBuf},
};

use byteorder::{LittleEndian, ReadBytesExt};
use ini::Ini;
use local_encoding::{Encoder, Encoding};
use memmap2::{Mmap, MmapOptions};

use crate::{ext::StrExt, filesystem::Filesystem, lzhuf};

pub struct Archive {
    path: PathBuf,
    index: usize,
    header: Ini,
    size: usize,
}

impl Archive {
    pub fn new(path: PathBuf, index: usize) -> anyhow::Result<Archive> {
        let size = File::open(&path)?.metadata()?.len() as usize;

        Ok(Archive {
            path,
            index,
            header: Ini::new(),
            size,
        })
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn header(&self) -> &Ini {
        &self.header
    }

    pub fn set_header(&mut self, header: Ini) {
        self.header = header;
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn open(&self) -> anyhow::Result<BufReader<File>> {
        Ok(BufReader::new(File::open(&self.path)?))
    }

    pub fn map(&self, start: usize, len: Option<usize>) -> anyhow::Result<Mmap> {
        let file = File::open(&self.path)?;

        let mut options = MmapOptions::new();
        options.offset(start as u64);
        if let Some(len) = len {
            options.len(len);
        }

        Ok(unsafe { options.map(&file) }?)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct VirtualFile {
    name: PathBuf,
    archive: Option<usize>,
    size_real: usize,
    size_compressed: usize,
    ptr: usize,
}

impl VirtualFile {
    pub fn new(
        name: PathBuf,
        archive: Option<usize>,
        size_real: usize,
        size_compressed: usize,
        ptr: usize,
    ) -> VirtualFile {
        VirtualFile {
            name,
            archive,
            size_real,
            size_compressed,
            ptr,
        }
    }

    pub fn only_name(name: PathBuf) -> VirtualFile {
        VirtualFile::new(name, None, 0, 0, 0)
    }

    pub fn name(&self) -> &PathBuf {
        &self.name
    }

    pub fn archive(&self) -> Option<usize> {
        self.archive
    }
}

const ARCHIVE_COMPRESS_FLAG: u32 = 1 << 31;
const ARCHIVE_HEADER_CHUNK_ID: u32 = 666;

impl Filesystem {
    pub(crate) fn process_archive<P: AsRef<Path>>(&mut self, path: P) -> anyhow::Result<()> {
        log::trace!("process_archive: {}", path.as_ref().display().to_string());

        let path = std::fs::canonicalize(path)?;

        if self.archives.iter().any(|archive| archive.path() == &path) {
            return Ok(());
        }

        let index = self.archives.len();

        self.archives.push(Archive::new(path, index)?);

        let archive = self.archives.last_mut().unwrap();

        let mut reader = archive.open()?;

        let header = open_chunk(&mut reader, ARCHIVE_HEADER_CHUNK_ID)?;

        let load = if let Some(header) = header {
            let header = String::from_utf8(header)?;
            let header = Ini::load_from_str_noescape(&header)?;

            archive.set_header(header);

            archive
                .header()
                .get_from(Some("header"), "auto_load")
                .map(StrExt::is_bool_true)
                .unwrap_or(false)
        } else {
            true
        };

        if load {
            self.load_archive(index)
        } else {
            Ok(())
        }
    }

    fn load_archive(&mut self, index: usize) -> anyhow::Result<()> {
        log::trace!("load_archive: {}", index);

        let archive = self.archives.get_mut(index).unwrap();

        if archive.header().is_empty() {
            panic!("unsupported");
        }

        let entry_point = archive
            .header()
            .get_from(Some("header"), "entry_point")
            .unwrap();

        let entry_point = if entry_point == "gamedata" {
            todo!();
        } else {
            let (alias, add) = entry_point.split_once('\\').unwrap();
            let alias = PathBuf::from(alias);

            let mut read_path = PathBuf::new();

            if let Some(path) = self.paths.get(&alias) {
                read_path.push(path.path());
            }

            read_path.push(add);

            read_path
        };

        let mut reader = archive.open()?;

        let archive_index = archive.index();

        let chunk = open_chunk(&mut reader, 1)?.unwrap();
        log::trace!("load_archive: opened chunk");
        let buffers = ChunkBuffersIter::new(chunk).collect::<Vec<_>>();
        log::trace!("load_archive: collected buffers");

        buffers
            .into_iter()
            .map(|mut buffer| {
                let buffer_size = buffer.get_ref().len();

                let size_real = buffer.read_u32::<LittleEndian>()?;
                let size_compressed = buffer.read_u32::<LittleEndian>()?;
                let _crc = buffer.read_u32::<LittleEndian>()?;
                let name_length = buffer_size - 4 * size_of::<u32>();

                let mut name = vec![0; name_length];
                buffer.read_exact(name.as_mut_slice())?;
                let name = Encoding::ANSI.to_string(&name)?;

                let ptr = buffer.read_u32::<LittleEndian>()?;

                let mut path = entry_point.clone();
                path.push(name);

                self.register(
                    path,
                    Some(archive_index),
                    size_real as usize,
                    size_compressed as usize,
                    ptr as usize,
                )?;

                Ok(())
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        Ok(())
    }

    pub fn file_from_archive(&self, archive: usize, file: &VirtualFile) -> anyhow::Result<Vec<u8>> {
        let archive = &self.archives[archive];

        let map = archive.map(file.ptr, Some(file.size_compressed))?;

        let mut buffer = vec![0u8; file.size_real];

        if file.size_compressed == file.size_real {
            buffer.copy_from_slice(&map);
        } else {
            lzo1x_1::decompress_to_slice(&map, &mut buffer)?;
        }

        Ok(buffer)
    }

    pub fn string_from_archive(
        &self,
        archive: usize,
        file: &VirtualFile,
    ) -> anyhow::Result<String> {
        let data = self.file_from_archive(archive, file)?;

        Ok(Encoding::ANSI.to_string(&data)?)
    }
}

struct ChunkBuffersIter<T: AsRef<[u8]>> {
    inner: Cursor<T>,
}

impl<T: AsRef<[u8]>> ChunkBuffersIter<T> {
    fn new(inner: T) -> Self {
        ChunkBuffersIter {
            inner: Cursor::new(inner),
        }
    }
}

impl<T: AsRef<[u8]>> Iterator for ChunkBuffersIter<T> {
    type Item = Cursor<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        let buffer_size = self.inner.read_u16::<LittleEndian>().ok()? as usize;
        let mut buffer = vec![0; buffer_size];

        self.inner.read_exact(buffer.as_mut_slice()).ok()?;

        Some(Cursor::new(buffer))
    }
}

fn open_chunk<R: Read + Seek>(
    reader: &mut BufReader<R>,
    id: u32,
) -> anyhow::Result<Option<Vec<u8>>> {
    loop {
        let ty = reader.read_u32::<LittleEndian>()?;
        let size = reader.read_u32::<LittleEndian>()?;

        if (ty & !ARCHIVE_COMPRESS_FLAG) == id {
            let mut source_data = vec![0; size as usize];

            reader.read_exact(source_data.as_mut_slice())?;

            return if (ty & ARCHIVE_COMPRESS_FLAG) > 0 {
                Ok(Some(lzhuf::decompress(Cursor::new(source_data))?))
            } else {
                Ok(Some(source_data))
            };
        } else {
            reader.seek_relative(size as i64)?;
        }
    }
}

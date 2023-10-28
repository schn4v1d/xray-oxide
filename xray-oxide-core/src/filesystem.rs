use std::time::Instant;
use std::{
    collections::{HashMap, HashSet},
    fs::{DirEntry, File},
    hash::{Hash, Hasher},
    io::{BufReader, Cursor, Read, Seek},
    mem::size_of,
    path::{Path, PathBuf},
};

use byteorder::{LittleEndian, ReadBytesExt};
use ini::Ini;
use local_encoding::{Encoder, Encoding};
use thiserror::Error;

use crate::{
    ext::{MetadataExt, StrExt},
    lzhuf,
};

pub struct FSPath {
    path: PathBuf,
    root: PathBuf,
    add: Option<PathBuf>,
    def_ext: Option<String>,
    filter_caption: Option<String>,
    recurse: bool,
    notify: bool,
}

impl FSPath {
    pub fn new(
        root: PathBuf,
        add: Option<PathBuf>,
        def_ext: Option<String>,
        filter_caption: Option<String>,
        recurse: bool,
        notify: bool,
    ) -> FSPath {
        let mut path = root.clone();

        if let Some(add) = &add {
            path.push(add);
        }

        FSPath {
            path,
            root,
            add,
            def_ext,
            filter_caption,
            recurse,
            notify,
        }
    }
}

impl Default for FSPath {
    fn default() -> Self {
        FSPath::new(PathBuf::new(), None, None, None, false, false)
    }
}

#[derive(PartialEq, Eq)]
struct VirtualFile {
    name: String,
    archive: Option<usize>,
}

impl Hash for VirtualFile {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

struct Archive {
    path: PathBuf,
    index: usize,
    header: Ini,
}

impl Archive {
    fn open(&self) -> anyhow::Result<BufReader<File>> {
        Ok(BufReader::new(File::open(&self.path)?))
    }
}

const DEFAULT_FS_LTX: &str = "fsgame.ltx";
const FS_ROOT: &str = "$fs_root$";
const ARCHIVE_COMPRESS_FLAG: u32 = 1 << 31;
const ARCHIVE_HEADER_CHUNK_ID: u32 = 666;

pub struct Filesystem {
    fs_root: PathBuf,
    paths: HashMap<PathBuf, FSPath>,
    files: HashSet<VirtualFile>,
    archives: Vec<Archive>,
}

impl Filesystem {
    pub fn new() -> anyhow::Result<Filesystem> {
        Filesystem::with_fs_ltx(DEFAULT_FS_LTX)
    }

    pub fn with_fs_ltx(fs_path: &str) -> anyhow::Result<Filesystem> {
        let fs_root = Path::new(fs_path);
        let mut fs_root = std::fs::canonicalize(fs_root)?;
        fs_root.pop();

        let mut fs = Filesystem {
            fs_root,
            paths: HashMap::new(),
            files: HashSet::new(),
            archives: Vec::new(),
        };

        fs.initialize(fs_path)?;

        Ok(fs)
    }

    fn initialize<P: AsRef<Path>>(&mut self, fs_path: P) -> anyhow::Result<()> {
        log::debug!("Initializing filesystem");
        let start = Instant::now();

        let fs_path = fs_path.as_ref();

        let fs_ltx = std::fs::read_to_string(fs_path)?;

        for (line_idx, line) in fs_ltx
            .lines()
            .enumerate()
            .map(|(line_idx, line)| (line_idx + 1, line))
        {
            // Skip comment lines
            if line.starts_with(';') {
                continue;
            }

            let (id, values) = line.split_once('=').unwrap();
            let id = PathBuf::from(id.trim());
            let mut values = values.trim().split('|').map(str::trim);

            fn require_value<'a, I, T, F>(
                values: &mut I,
                f: F,
                fs_path: &Path,
                line_idx: usize,
            ) -> anyhow::Result<T>
            where
                I: Iterator<Item = &'a str>,
                F: Fn(&'a str) -> T,
            {
                if let Some(v) = values.next() {
                    Ok(f(v))
                } else {
                    Err(FilesystemError::InvalidFsLtxSyntax {
                        file_name: fs_path.display().to_string(),
                        line: line_idx,
                    }
                    .into())
                }
            }

            let recurse = require_value(&mut values, StrExt::is_bool_true, fs_path, line_idx)?;

            let notify = require_value(&mut values, StrExt::is_bool_true, fs_path, line_idx)?;

            let root = require_value(&mut values, PathBuf::from, fs_path, line_idx)?;

            let add = values.next().map(PathBuf::from);
            let def_ext = values.next().map(str::to_owned);
            let filter_caption = values.next().map(str::to_owned);

            let root_path = self.paths.get(&root);
            let root_path = if root_path.is_none() && root.as_os_str() == FS_ROOT {
                let p = FSPath::new(self.fs_root.clone(), None, None, None, false, false);

                self.paths.insert(PathBuf::from(FS_ROOT), p);

                self.paths.get(&root)
            } else {
                root_path
            };

            let root = root_path.map(|path| path.path.clone()).unwrap_or(root);

            let path = FSPath::new(root, add, def_ext, filter_caption, recurse, notify);

            self.recurse(&path.path, path.recurse)?;

            self.paths.insert(id, path);
        }

        log::debug!(
            "Initialized filesystem in {} seconds",
            start.elapsed().as_secs_f64()
        );

        log::debug!(
            "{} files cached {} archives",
            self.files.len(),
            self.archives.len()
        );

        Ok(())
    }

    fn recurse<P: AsRef<Path>>(&mut self, path: P, recurse: bool) -> anyhow::Result<()> {
        let path = path.as_ref().to_path_buf();
        log::trace!("recurse({})", path.display().to_string());

        let ignore = {
            let mut path = path.clone();
            path.push(".xrignore");
            path
        };

        if ignore.exists() {
            return Ok(());
        }

        if let Ok(dir) = path.read_dir() {
            for entry in dir {
                let entry = entry?;

                if ignore_name(&entry.file_name().into_string().unwrap()) {
                    continue;
                }

                self.process_single(entry, recurse)?;
            }
        } else {
            return Ok(());
        }

        self.register(path, None)?;

        Ok(())
    }

    fn register<P: AsRef<Path>>(&mut self, path: P, archive: Option<usize>) -> anyhow::Result<()> {
        let path = path.as_ref();
        log::trace!("register({}, {archive:?})", path.display().to_string());

        let description = VirtualFile {
            name: path.display().to_string(),
            archive,
        };

        self.files.insert(description);

        let mut archive_id = archive;

        for ancestor in path.ancestors().skip(1) {
            let description = VirtualFile {
                name: ancestor.display().to_string(),
                archive: archive_id,
            };

            if !self.files.insert(description) {
                break;
            }

            archive_id = None;
        }

        Ok(())
    }

    fn process_single(&mut self, entry: DirEntry, recurse: bool) -> anyhow::Result<()> {
        log::trace!("process_single: {}", entry.path().display().to_string());

        let metadata = entry.metadata()?;

        if metadata.is_hidden() {
            return Ok(());
        }

        if metadata.is_dir() {
            if !recurse || entry.file_name() == "." || entry.file_name() == ".." {
                return Ok(());
            }

            self.register(entry.path(), None)?;

            self.recurse(entry.path(), recurse)
        } else {
            let path = entry.path();

            if let Some(extension) = path.extension() {
                let extension = extension.to_str().unwrap();

                if extension.starts_with("db") || extension.starts_with("xdb") {
                    return self.process_archive(path);
                }
            }

            self.register(path, None)
        }
    }

    fn process_archive<P: AsRef<Path>>(&mut self, path: P) -> anyhow::Result<()> {
        log::trace!("process_archive: {}", path.as_ref().display().to_string());

        let path = std::fs::canonicalize(path)?;

        if self.archives.iter().any(|archive| archive.path == path) {
            return Ok(());
        }

        let index = self.archives.len();

        self.archives.push(Archive {
            path,
            index,
            header: Ini::new(),
        });

        let archive = self.archives.last_mut().unwrap();

        let mut reader = archive.open()?;

        let header = open_chunk(&mut reader, ARCHIVE_HEADER_CHUNK_ID)?;

        let load = if let Some(header) = header {
            let header = String::from_utf8(header)?;
            let header = Ini::load_from_str_noescape(&header)?;

            archive.header = header;

            archive
                .header
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

        if archive.header.is_empty() {
            panic!("unsupported");
        }

        let entry_point = archive
            .header
            .get_from(Some("header"), "entry_point")
            .unwrap();

        let entry_point = if entry_point == "gamedata" {
            todo!();
        } else {
            let (alias, add) = entry_point.split_once('\\').unwrap();
            let alias = PathBuf::from(alias);

            let mut read_path = PathBuf::new();

            if let Some(path) = self.paths.get(&alias) {
                read_path.push(&path.path);
            }

            read_path.push(add);

            read_path
        };

        let mut reader = archive.open()?;

        let archive_index = archive.index;

        let chunk = open_chunk(&mut reader, 1)?.unwrap();
        log::trace!("load_archive: opened chunk");
        let buffers = ChunkBuffersIter::new(chunk).collect::<Vec<_>>();
        log::trace!("load_archive: collected buffers");

        buffers
            .into_iter()
            .map(|mut buffer| {
                let buffer_size = buffer.get_ref().len();

                let _size_real = buffer.read_u32::<LittleEndian>()?;
                let _size_compr = buffer.read_u32::<LittleEndian>()?;
                let _crc = buffer.read_u32::<LittleEndian>()?;
                let name_length = buffer_size - 4 * size_of::<u32>();

                let mut name = vec![0; name_length];
                buffer.read_exact(name.as_mut_slice())?;
                let name = Encoding::ANSI.to_string(&name)?;

                let _ptr = buffer.read_u32::<LittleEndian>()?;

                let mut path = entry_point.clone();
                path.push(name);

                self.register(path, Some(archive_index))?;

                Ok(())
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        Ok(())
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

#[derive(Error, Debug)]
pub enum FilesystemError {
    #[error("invalid fs_ltx syntax in {file_name} in line {line}")]
    InvalidFsLtxSyntax { file_name: String, line: usize },
}

fn ignore_name(name: &str) -> bool {
    name == "Thumbs.db" || name == ".svn"
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

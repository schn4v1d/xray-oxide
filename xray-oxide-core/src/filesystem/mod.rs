use std::{
    collections::HashMap,
    fs::DirEntry,
    path::{Path, PathBuf},
    time::Instant,
};

use thiserror::Error;

use crate::ext::{MetadataExt, StrExt};

use archive::{Archive, VirtualFile};
use fs_path::FSPath;

pub mod archive;
pub mod fs_path;

const DEFAULT_FS_LTX: &str = "fsgame.ltx";
const FS_ROOT: &str = "$fs_root$";

pub struct Filesystem {
    fs_root: PathBuf,
    paths: HashMap<PathBuf, FSPath>,
    files: HashMap<PathBuf, VirtualFile>,
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
            files: HashMap::new(),
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

            let root = root_path.map(|path| path.path().clone()).unwrap_or(root);

            let path = FSPath::new(root, add, def_ext, filter_caption, recurse, notify);

            self.recurse(path.path(), path.recurse())?;

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

        self.register(path, None, 0, 0, 0)?;

        Ok(())
    }

    fn register<P: AsRef<Path>>(
        &mut self,
        path: P,
        archive: Option<usize>,
        size_real: usize,
        size_compressed: usize,
        ptr: usize,
    ) -> anyhow::Result<()> {
        let path = path.as_ref();
        log::trace!("register({}, {archive:?})", path.display());

        let description =
            VirtualFile::new(path.to_path_buf(), archive, size_real, size_compressed, ptr);

        self.files.insert(path.to_path_buf(), description);

        let mut archive_id = archive;

        for ancestor in path.ancestors().skip(1) {
            let description = VirtualFile::new(ancestor.to_path_buf(), archive_id, 0, 0, 0);

            if self.files.insert(ancestor.to_path_buf(), description).is_some() {
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

            self.register(entry.path(), None, 0, 0, 0)?;

            self.recurse(entry.path(), recurse)
        } else {
            let path = entry.path();

            if let Some(extension) = path.extension() {
                let extension = extension.to_str().unwrap();

                if extension.starts_with("db") || extension.starts_with("xdb") {
                    return self.process_archive(path);
                }
            }

            let size = entry.metadata()?.len() as usize;

            self.register(path, None, size, size, 0)
        }
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

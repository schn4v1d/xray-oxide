use crate::filesystem::Filesystem;
use std::fs::read_to_string;
use std::path::{Path, PathBuf};
use thiserror::Error;

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

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    pub fn recurse(&self) -> bool {
        self.recurse
    }

    pub fn appended<P: AsRef<Path>>(&self, to_append: P) -> PathBuf {
        let mut path = self.path.clone();
        path.push(to_append);
        path
    }
}

impl Default for FSPath {
    fn default() -> Self {
        FSPath::new(PathBuf::new(), None, None, None, false, false)
    }
}

impl Filesystem {
    pub fn get_path<P: AsRef<Path>>(&self, path: P) -> Option<&FSPath> {
        self.paths.get(path.as_ref())
    }
    pub fn get_path_mut<P: AsRef<Path>>(&mut self, path: P) -> Option<&mut FSPath> {
        self.paths.get_mut(path.as_ref())
    }

    pub fn append_path<P1: AsRef<Path>, P2: AsRef<Path>>(
        &self,
        initial: P1,
        append: P2,
    ) -> Option<PathBuf> {
        self.get_path(initial).map(|p| p.appended(append))
    }

    pub fn read_to_string<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<String> {
        let path = path.as_ref().to_path_buf();

        let file = self
            .files
            .get(&path)
            .ok_or(FilesystemFSPathError::NotFound { path })?;

        Ok(match file.archive() {
            Some(archive) => self.string_from_archive(archive, file)?,
            None => read_to_string(file.name())?,
        })
    }
}

#[derive(Debug, Error)]
pub enum FilesystemFSPathError {
    #[error("File not found {path}")]
    NotFound { path: PathBuf },
}

use std::fs::Metadata;

pub trait StrExt {
    fn is_bool_true(&self) -> bool;
}

impl StrExt for str {
    fn is_bool_true(&self) -> bool {
        self == "on" || self == "yes" || self == "true" || self == "1"
    }
}

pub trait MetadataExt {
    fn is_hidden(&self) -> bool;
}

#[cfg(windows_platform)]
impl MetadataExt for Metadata {
    fn is_hidden(&self) -> bool {
        use std::os::windows::fs::MetadataExt;

        (self.file_attributes() & 2) > 0
    }
}

#[cfg(not(windows_platform))]
impl MetadataExt for Metadata {
    fn is_hidden(&self) -> bool {
        false
    }
}

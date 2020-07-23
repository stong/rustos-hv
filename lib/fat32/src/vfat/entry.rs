use crate::traits;
use alloc::string::String;
use crate::vfat::{Dir, File, Metadata, Cluster, VFatHandle};
use core::fmt;

// You can change this definition if you want
#[derive(Debug)]
pub enum Entry<HANDLE: VFatHandle> {
    File(File<HANDLE>),
    Dir(Dir<HANDLE>)
}

#[derive(Debug)]
pub struct EntryInfo<HANDLE: VFatHandle> {
    pub vfat: HANDLE,
    pub cluster: Cluster,
    pub metadata: Metadata,
    pub name: String
}

impl<HANDLE: VFatHandle> Entry<HANDLE> {
    pub fn entry_info(&self) -> &EntryInfo<HANDLE> {
        match self {
            Entry::File(f) => &f.entry_info,
            Entry::Dir(f) => &f.0
        }
    }
}

impl<HANDLE: VFatHandle> traits::Entry for Entry<HANDLE> {
    type File = File<HANDLE>;
    type Dir = Dir<HANDLE>;
    type Metadata = Metadata;

    fn name(&self) -> &str {
        &self.entry_info().name
    }

    fn metadata(&self) -> &Metadata {
        &self.entry_info().metadata
    }

    fn as_file(&self) -> Option<&File<HANDLE>> {
        match self {
            Entry::File(f) => Some(f),
            Entry::Dir(_) => None
        }
    }

    fn as_dir(&self) -> Option<&Dir<HANDLE>> {
        match self {
            Entry::File(_) => None,
            Entry::Dir(f) => Some(f)
        }
    }

    fn into_file(self) -> Option<File<HANDLE>> {
        match self {
            Entry::File(f) => Some(f),
            Entry::Dir(_) => None
        }
    }

    fn into_dir(self) -> Option<Dir<HANDLE>> {
        match self {
            Entry::File(_) => None,
            Entry::Dir(f) => Some(f)
        }
    }
} 

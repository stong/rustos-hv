use alloc::vec::Vec;
use alloc::string::String;
use shim::const_assert_size;
use shim::ffi::OsStr;
use shim::io;
use shim::ioerr;
use shim::newioerr;

use crate::traits;
use crate::util::VecExt;
use crate::vfat::{Attributes, Date, Metadata, Time, Timestamp};
use crate::vfat::entry::EntryInfo;
use crate::vfat::{Cluster, File, Entry, VFatHandle};

#[derive(Debug)]
pub struct Dir<HANDLE: VFatHandle>(pub EntryInfo<HANDLE>);

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct VFatRegularDirEntry {
    file_name: [u8; 8],
    file_extension: [u8; 3],
    attributes: Attributes,
    reserved_nt: u8,
    creation_time_tenths: u8,
    creation_time: Time,
    creation_date: Date,
    access_date: Date,
    cluster_hi: u16,
    modified_time: Time,
    modified_date: Date,
    cluster_lo: u16,
    filesize: u32
}

const_assert_size!(VFatRegularDirEntry, 32);

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct VFatLfnDirEntry {
    sequence_number: u8,
    name_1: [u8; 10],
    attributes: Attributes, // 0x0F
    lfn_type: u8, // 0x00
    checksum_dos: u8,
    name_2: [u8; 12],
    reserved: u16, // 0x0000
    name_3: [u8; 4]
}

impl VFatLfnDirEntry {
    pub fn full_name(&self) -> impl core::iter::Iterator<Item = u8> + '_ {
        self.name_1.iter().chain(self.name_2.iter()).chain(self.name_3.iter()).copied()
    }

    pub fn sequence_number(&self) -> u8 {
        (self.sequence_number & 0x1F) - 1
    }
}

const_assert_size!(VFatLfnDirEntry, 32);

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct VFatUnknownDirEntry {
    id: u8,
    _padding1: [u8; 10],
    attributes: Attributes,
    _padding2: [u8; 20]
}

const_assert_size!(VFatUnknownDirEntry, 32);

pub union VFatDirEntry {
    unknown: VFatUnknownDirEntry,
    regular: VFatRegularDirEntry,
    long_filename: VFatLfnDirEntry,
}

impl<HANDLE: VFatHandle> Dir<HANDLE> {
    pub fn from(entry_info: EntryInfo<HANDLE>) -> Dir<HANDLE> {
        Dir{0: entry_info}
    }

    pub fn from_root(vfat: HANDLE) -> Dir<HANDLE> {
        let cluster = vfat.lock(|vfat| vfat.rootdir_cluster);
        Dir{0: EntryInfo{
            vfat,
            cluster,
            metadata: Metadata::default(),
            name: String::from("")
        }}
    }
}

pub struct VFatDirIter<HANDLE: VFatHandle> {
    vfat: HANDLE,
    entries: Vec<VFatDirEntry>,
    cur_name: Vec<u8>, // utf-16
    i: usize,
}

impl<HANDLE: VFatHandle> VFatDirIter<HANDLE> {
    fn from(vfat: HANDLE, entries: Vec<VFatDirEntry>) -> VFatDirIter<HANDLE> {
        VFatDirIter{
            vfat,
            entries,
            cur_name: vec![0; 0],
            i: 0,
        }
    }
}

fn parse_ascii_name(yeehaw: &[u8]) -> String {
    let len = yeehaw.iter().copied().scan(false, |terminated, c| {
        *terminated |= c == 0x00 || c == 0x20;
        if *terminated {
            None
        } else {
            Some(c)
        }
    }).count();
    String::from_utf8_lossy(&yeehaw[..len]).into_owned()
}

impl<HANDLE: VFatHandle> Iterator for VFatDirIter<HANDLE> {
    type Item = Entry<HANDLE>;

    fn next(&mut self) -> Option<Entry<HANDLE>> {
        loop {
            let entry = &self.entries[self.i];
            let unknown_entry = unsafe { entry.unknown };
            self.i += 1;
            match unknown_entry.id {
                0xE5 => continue, // Deleted entry
                0x00 => return None, // End of directory
                _ => (),
            }
            if unknown_entry.attributes.lfn() {
                let lfn_entry = unsafe { entry.long_filename };
                let name_index = lfn_entry.sequence_number() as usize * 26;
                if self.cur_name.len() < name_index + 26 {
                    self.cur_name.resize(name_index + 26, 0);
                }
                for (i, c) in lfn_entry.full_name().enumerate() {
                    self.cur_name[name_index + i] = c;
                }
            } else {
                let dir_entry = unsafe { entry.regular };
                let name: String;
                if !self.cur_name.is_empty() { // LFN
                    name = core::char::decode_utf16(self.cur_name.chunks(2).scan(false, |terminated, chars| {
                        let c: u16 = (chars[0] as u16) | ((chars[1] as u16) << 8);
                        *terminated |= c == 0x0000 || c == 0xFFFF;
                        if *terminated {
                            None
                        } else {
                            Some(c)
                        }
                    })).map(|r| r.unwrap_or(core::char::REPLACEMENT_CHARACTER)).collect::<String>();
                } else {
                    let extension = parse_ascii_name(&dir_entry.file_extension);
                    if extension.is_empty() {
                        name = format!("{}", parse_ascii_name(&dir_entry.file_name));
                    } else {
                        name = format!("{}.{}", parse_ascii_name(&dir_entry.file_name), extension);
                    }
                }
                let entry_info = EntryInfo{
                    vfat: self.vfat.clone(),
                    cluster: Cluster::from((dir_entry.cluster_lo as u32) | ((dir_entry.cluster_hi as u32) << 16)),
                    metadata: Metadata {
                        created: Timestamp{
                            date: dir_entry.creation_date,
                            time: dir_entry.creation_time,
                            time_10ms: dir_entry.creation_time_tenths
                        },
                        accessed: dir_entry.creation_date,
                        modified: Timestamp{
                            date: dir_entry.modified_date,
                            time: dir_entry.modified_time,
                            time_10ms: 0
                        },
                        attributes: dir_entry.attributes
                    },
                    name
                };
                self.cur_name.clear();
                if dir_entry.attributes.directory() {
                    return Some(Entry::Dir(Dir::from(entry_info)));
                } else {
                    return Some(Entry::File(File::from(entry_info, dir_entry.filesize as u64)));
                }
            }
        }
    }
}

impl<HANDLE: VFatHandle> traits::Dir for Dir<HANDLE> {
    type Entry = Entry<HANDLE>;

    type Iter = VFatDirIter<HANDLE>;

    fn entries(&self) -> io::Result<VFatDirIter<HANDLE>> {
        let mut yeehaw = vec![0 as u8; self.0.vfat.lock(|vfat| vfat.cluster_size())];
        self.0.vfat.lock(|vfat| vfat.read_chain(self.0.cluster, &mut yeehaw))?;
        Ok(VFatDirIter::from(self.0.vfat.clone(), unsafe { yeehaw.cast() }))
    }

    /// Finds the entry named `name` in `self` and returns it. Comparison is
    /// case-insensitive.
    ///
    /// # Errors
    ///
    /// If no entry with name `name` exists in `self`, an error of `NotFound` is
    /// returned.
    ///
    /// If `name` contains invalid UTF-8 characters, an error of `InvalidInput`
    /// is returned.
    fn find<P: AsRef<OsStr>>(&self, name: P) -> io::Result<Entry<HANDLE>> {
        let name_str = name.as_ref().to_str().ok_or(newioerr!(InvalidInput, "invalid UTF-8"))?;
        for entry in traits::Dir::entries(self)? {
            if traits::Entry::name(&entry).eq_ignore_ascii_case(name_str) {
                return Ok(entry);
            }
        }
        ioerr!(NotFound, "not found")
    }
}

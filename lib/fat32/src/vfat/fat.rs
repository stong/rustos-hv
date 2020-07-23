use crate::vfat::*;
use core::fmt;

#[derive(Debug, PartialEq)]
pub enum Status {
    /// The FAT entry corresponds to an unused (free) cluster.
    Free,
    /// The FAT entry/cluster is reserved.
    Reserved,
    /// The FAT entry corresponds to a valid data cluster. The next cluster in
    /// the chain is `Cluster`.
    Data(Cluster),
    /// The FAT entry corresponds to a bad (disk failed) cluster.
    Bad,
    /// The FAT entry corresponds to a valid data cluster. The corresponding
    /// cluster is the last in its chain.
    Eoc(u32),
}

#[derive(Copy, Clone)]
#[repr(C, packed)]
pub struct FatEntry(pub u32);

impl FatEntry {
    /// Returns the `Status` of the FAT entry `self`.
    pub fn status(&self) -> Status {
        match self.0 & !0xF0000000 {
            0 => Status::Free,
            1 | 0xFFFFFF0..=0xFFFFFF6 => Status::Reserved,
            0xFFFFFF7 => Status::Bad,
            eoc @ 0xFFFFFF8..=0xFFFFFFF => Status::Eoc(eoc),
            cluster => Status::Data(Cluster::from(cluster as u32))
        }
    }
}

impl fmt::Debug for FatEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("FatEntry")
            .field("value", &{ self.0 })
            .field("status", &self.status())
            .finish()
    }
}

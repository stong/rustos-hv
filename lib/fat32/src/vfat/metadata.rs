use core::fmt;

use crate::traits;

/// A date as represented in FAT32 on-disk structures.
#[repr(C, packed)]
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub struct Date(u16);

/// Time as represented in FAT32 on-disk structures.
#[repr(C, packed)]
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub struct Time(u16);

/// File attributes as represented in FAT32 on-disk structures.
#[repr(C, packed)]
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub struct Attributes(u8);

/// A structure containing a date and time.
#[derive(Default, Copy, Clone, Debug, PartialEq, Eq)]
pub struct Timestamp {
    pub date: Date,
    pub time: Time,
    pub time_10ms: u8 // tenths of a second
}

/// Metadata for a directory entry.
#[derive(Default, Debug, Clone)]
pub struct Metadata {
    pub created: Timestamp,
    pub accessed: Date,
    pub modified: Timestamp,
    pub attributes: Attributes,
}

impl Attributes {
    pub fn system(&self) -> bool {
        return self.0 & 0x04 != 0;
    }

    pub fn volume_id(&self) -> bool {
        return self.0 & 0x08 != 0;
    }

    pub fn directory(&self) -> bool {
        return self.0 & 0x10 != 0;
    }

    pub fn archive(&self) -> bool {
        return self.0 & 0x20 != 0;
    }

    pub fn lfn(&self) -> bool {
        return self.0 == 0xF;
    }
}

impl traits::Timestamp for Timestamp {
    fn year(&self) -> usize {
        1980 + ((self.date.0 >> 9) & 0xFF) as usize
    }

    fn month(&self) -> u8 {
        ((self.date.0 >> 5) & 0xF) as u8
    }

    fn day(&self) -> u8 {
        (self.date.0 & 0x1F) as u8
    }

    fn hour(&self) -> u8 {
        ((self.time.0 >> 11) & 0x1F) as u8
    }

    fn minute(&self) -> u8 {
        ((self.time.0 >> 5) & 0x3F) as u8
    }

    fn second(&self) -> u8 {
        (2 * (self.time.0 & 0x1F)) as u8 + self.time_10ms / 100
    }
}

impl traits::Metadata for Metadata {
    type Timestamp = Timestamp;

    fn read_only(&self) -> bool {
        return self.attributes.0 & 0x01 != 0;
    }

    fn hidden(&self) -> bool {
        return self.attributes.0 & 0x02 != 0;
    }

    fn created(&self) -> Self::Timestamp {
        return self.created;
    }

    fn modified(&self) -> Self::Timestamp {
        return self.modified;
    }

    fn accessed(&self) -> Self::Timestamp {
        return Timestamp{date: self.accessed, time: Time{0: 0}, time_10ms: 0};
    }
}

impl fmt::Display for Metadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.attributes.lfn() {
            write!(f, "LFN")?;
        } else {
            if traits::Metadata::read_only(self) {
                write!(f, "R")?;
            }
            if self.attributes.archive() {
                write!(f, "A")?;
            }
            if self.attributes.system() {
                write!(f, "S")?;
            }
            if traits::Metadata::hidden(self) {
                write!(f, "H")?;
            }
        }
        Ok(())
    }
}

use core::fmt;
use fmt::Debug;
use shim::const_assert_size;
use shim::io;
use core::mem;

use crate::traits::BlockDevice;

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct CHS {
    head: u8,
    sec_cyl: u16
}

impl CHS {
    pub fn head(&self) -> u8 {
        self.head
    }

    pub fn cylinder(&self) -> u16 {
        (self.sec_cyl.to_le() >> 6) & 0x3FF
    }

    pub fn sector(&self) -> u16 {
        (self.sec_cyl.to_le()) & 0x3F
    }
}

impl Debug for CHS {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("CHS")
            .field("head", &self.head())
            .field("cylinder", &self.cylinder())
            .field("sector", &self.sector())
            .finish()
    }
}

const_assert_size!(CHS, 3);

#[repr(C, packed)]
pub struct PartitionEntry {
    pub boot_indicator: u8,  // 0x0: 0x80 == bootable, 0x00 = no
    pub start: CHS,          // 0x1:
    pub part_type: u8,       // 0x4: 0xB or 0xC = fat32
    pub end: CHS,            // 0x5:
    pub offset: u32,         // 0x8: offset in sectors from start of disk to start of partition
    pub num_sectors: u32     // 0xC: total sectors in partition
}

impl PartitionEntry {
    pub fn bootable(&self) -> bool {
        return self.boot_indicator == 0x80;
    }
}

impl Debug for PartitionEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("PartitionEntry")
            .field("boot_indicator", &self.boot_indicator)
            .field("start", &self.start)
            .field("part_type", &self.part_type)
            .field("end", &self.end)
            .field("offset", &{self.offset})
            .field("num_sectors", &{self.num_sectors})
            .finish()
    }
}

const_assert_size!(PartitionEntry, 16);

/// The master boot record (MBR).
#[repr(C, packed)]
pub struct MasterBootRecord {
    pub bootstrap: [u8; 436],                 // 0x000: code
    pub disk_id: [u8; 10],                    // 0x1b4: optional disk ID
    pub partition_table: [PartitionEntry; 4], // 0x1be: partition table
    pub magic: u16                            // 0x1fe: 0x55, 0xAA (0xAA55) "Valid bootsector"
}

impl Debug for MasterBootRecord {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MasterBootRecord")
            .field("disk_id", &self.disk_id)
            .field("partition_table", &self.partition_table)
            .field("magic", &{self.magic.to_le()})
            .finish()
    }
}

const_assert_size!(MasterBootRecord, 512);

#[derive(Debug)]
pub enum Error {
    /// There was an I/O error while reading the MBR.
    Io(io::Error),
    /// Partiion `.0` (0-indexed) contains an invalid or unknown boot indicator.
    UnknownBootIndicator(u8),
    /// The MBR magic signature was invalid.
    BadSignature,
}

impl MasterBootRecord {
    /// Reads and returns the master boot record (MBR) from `device`.
    ///
    /// # Errors
    ///
    /// Returns `BadSignature` if the MBR contains an invalid magic signature.
    /// Returns `UnknownBootIndicator(n)` if partition `n` contains an invalid
    /// boot indicator. Returns `Io(err)` if the I/O error `err` occured while
    /// reading the MBR.
    pub fn from<T: BlockDevice>(mut device: T) -> Result<MasterBootRecord, Error> {
        let mut mbr_data: [u8; 512] = [0; 512];
        device.read_sector(0, &mut mbr_data).map_err(Error::Io)?;
        if mbr_data[0x1FE] != 0x55 || mbr_data[0x1FF] != 0xAA {
            return Err(Error::BadSignature);
        }
        let mbr: MasterBootRecord = unsafe { mem::transmute(mbr_data) };
        assert_eq!(mbr.magic.to_le(), 0xAA55);
        for (n, partition) in mbr.partition_table.iter().enumerate() {
            match partition.boot_indicator {
                0x00 | 0x80 => (),
                _ => return Err(Error::UnknownBootIndicator(n as u8))
            }
        }
        Ok(mbr)
    }
}

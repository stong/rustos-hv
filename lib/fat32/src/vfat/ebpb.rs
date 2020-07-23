use core::mem;
use core::fmt;
use shim::const_assert_size;

use crate::traits::BlockDevice;
use crate::vfat::Error;

#[repr(C, packed)]
pub struct BiosParameterBlock {
    pub jmp_short: [u8; 3],         // 0x000
    pub oem_ident: [u8; 8],         // 0x003
    pub bytes_per_sector: u16,      // 0x00B
    pub sectors_per_cluster: u8,    // 0x00D
    pub reserved_sectors: u16,      // 0x00E
    pub num_fats: u8,               // 0x010
    pub max_dir_entries: u16,       // 0x011
    pub total_sectors_16: u16,      // 0x013
    pub fat_id: u8,                 // 0x015
    pub sectors_per_fat_16: u16,    // 0x016
    pub sectors_per_track: u16,     // 0x018
    pub num_heads: u16,             // 0x01A
    pub hidden_sectors: u32,        // 0x01C
    pub total_sectors_32: u32,      // 0x020
    pub sectors_per_fat_32: u32,    // 0x024
    pub flags: u16,                 // 0x028
    pub version: u16,               // 0x02A
    pub root_cluster: u32,          // 0x02C
    pub fsinfo_sector: u16,         // 0x030
    pub backup_boot_sector: u16,    // 0x032
    pub reserved: [u8; 12],         // 0x034
    pub drive_number: u8,           // 0x040
    pub nt_flags: u8,               // 0x041
    pub signature: u8,              // 0x042: should be 0x28 or 0x29
    pub volume_id: u32,             // 0x043
    pub volume_label: [u8; 11],     // 0x047
    pub system_identifier: [u8; 8], // 0x052: "FAT32   "
    pub boot_code: [u8; 420],       // 0x05A
    pub magic: u16,                 // 0x1FE: 0x55, 0xAA (0xAA55) "Valid bootsector"
}

const_assert_size!(BiosParameterBlock, 512);

impl BiosParameterBlock {
    /// Reads the FAT32 extended BIOS parameter block from sector `sector` of
    /// device `device`.
    ///
    /// # Errors
    ///
    /// If the EBPB signature is invalid, returns an error of `BadSignature`.
    pub fn from<T: BlockDevice>(mut device: T, sector: u64) -> Result<BiosParameterBlock, Error> {
        let mut ebpb_data: [u8; 512] = [0; 512];
        device.read_sector(sector, &mut ebpb_data).map_err(Error::Io)?;
        if ebpb_data[0x1FE] != 0x55 || ebpb_data[0x1FF] != 0xAA {
            return Err(Error::BadSignature);
        }
        let ebpb: BiosParameterBlock = unsafe { mem::transmute(ebpb_data) };
        Ok(ebpb)
    }
}

impl fmt::Debug for BiosParameterBlock {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BiosParameterBlock")
            .field("bytes_per_sector", &{self.bytes_per_sector})
            .field("reserved_sectors", &{self.reserved_sectors})
            .field("num_fats", &{self.num_fats})
            .field("sectors_per_track", &{self.sectors_per_track})
            .field("num_heads", &{self.num_heads})
            .field("total_sectors_32", &{self.total_sectors_32})
            .field("sectors_per_fat_32", &{self.sectors_per_fat_32})
            .field("version", &{self.version})
            .field("root_cluster", &{self.root_cluster})
            .field("fsinfo_sector", &{self.fsinfo_sector})
            .field("signature", &{self.signature})
            .field("magic", &{self.magic})
            .finish()
    }
}

use core::fmt::Debug;
use core::marker::PhantomData;

use alloc::vec::Vec;

use shim::io;
use shim::ioerr;
use shim::path;
use shim::path::Path;

use crate::mbr::MasterBootRecord;
use crate::traits::{BlockDevice, FileSystem};
use crate::util::SliceExt;
use crate::vfat::{BiosParameterBlock, CachedPartition, Partition};
use crate::vfat::{Cluster, Dir, Entry, Error, FatEntry, File, Status};

/// A generic trait that handles a critical section as a closure
pub trait VFatHandle: Clone + Debug + Send + Sync {
    fn new(val: VFat<Self>) -> Self;
    fn lock<R>(&self, f: impl FnOnce(&mut VFat<Self>) -> R) -> R;
}

#[derive(Debug)]
pub struct VFat<HANDLE: VFatHandle> {
    // all sectors are in LOGICAL units
    pub phantom: PhantomData<HANDLE>,
    pub device: CachedPartition,
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub sectors_per_fat: u32,
    pub fat_start_sector: u64,
    pub data_start_sector: u64,
    pub rootdir_cluster: Cluster
}

impl<HANDLE: VFatHandle> VFat<HANDLE> {
    pub fn cluster_size(&self) -> usize {
        return self.bytes_per_sector as usize * self.sectors_per_cluster as usize;
    }
}

impl<HANDLE: VFatHandle> VFat<HANDLE> {
    pub fn from<T>(mut device: T) -> Result<HANDLE, Error>
    where
        T: BlockDevice + 'static,
    {
        let mbr = MasterBootRecord::from(&mut device)?;
        let partition = mbr.partition_table.iter().filter(|p| p.part_type == 0xB || p.part_type == 0xC).next().ok_or(Error::NotFound)?;
        let ebpb = BiosParameterBlock::from(&mut device, partition.offset as u64)?;
        let vfat = VFat{
            phantom: PhantomData,
            device: CachedPartition::new(device, Partition{
                start: partition.offset as u64,
                num_sectors: partition.num_sectors as u64,
                sector_size: ebpb.bytes_per_sector as u64
            }),
            bytes_per_sector: ebpb.bytes_per_sector,
            sectors_per_cluster: ebpb.sectors_per_cluster,
            sectors_per_fat: ebpb.sectors_per_fat_32,
            fat_start_sector: ebpb.reserved_sectors as u64,
            data_start_sector: ebpb.reserved_sectors as u64 + ebpb.sectors_per_fat_32 as u64 * ebpb.num_fats as u64,
            rootdir_cluster: Cluster::from(ebpb.root_cluster as u32)
        };
        Ok(VFatHandle::new(vfat))
    }

    /// A method to read from an offset of a cluster into a buffer.
    pub fn read_cluster(&mut self, cluster: Cluster, offset: usize, buf: &mut [u8]) -> io::Result<usize> {
        // println!("Read cluster {:?} offset {} size {} sectorsize {} clustersize {}", cluster, offset, buf.len(), self.bytes_per_sector, self.cluster_size());
        if cluster.raw_value() < 2 {
            return ioerr!(InvalidInput, "attempting to read reserved cluster");
        }
        let mut n = 0;
        let sector_size = self.bytes_per_sector as usize;
        if offset >= self.cluster_size() {
            return ioerr!(InvalidInput, "offset must be less than cluster size");
        }
        let skip_sectors = offset as u64 / sector_size as u64;
        let mut sector_offset = offset as usize % sector_size as usize;
        let start_sector = self.data_start_sector + self.sectors_per_cluster as u64 * cluster.logical_value() as u64 + skip_sectors;
        let n_sectors = core::cmp::min(self.sectors_per_cluster as u64 - skip_sectors, (buf.len() + sector_size - 1) as u64 / sector_size as u64);
        // println!(" n_sectors {} skip_sectors {}", n_sectors, skip_sectors);
        for i in 0..n_sectors as usize {
            // println!("  secs {} {} {} {}", i, n_sectors, n, buf.len());
            assert!(n < buf.len());
            let mut sector_buf = vec![0 as u8; sector_size];
            if self.device.read_sector(start_sector + i as u64, &mut sector_buf)? < sector_buf.len() {
                return ioerr!(UnexpectedEof, "unexpected end of sector");
            }
            let readlen = core::cmp::min(buf.len() - n, sector_size - sector_offset);
            // println!("  Readlen {}", readlen);
            buf[n..n + readlen].copy_from_slice(&sector_buf[sector_offset..sector_offset+readlen]);
            n += readlen;
            sector_offset = 0;
        }
        assert!(n <= buf.len());
        Ok(n)
    }

    pub fn next_cluster(&mut self, current: Cluster) -> io::Result<Option<Cluster>> {
        if current.raw_value() < 2 {
            return ioerr!(InvalidInput, "attempting to query reserved cluster");
        }
        match self.fat_entry(current) {
            Err(e) => Err(e),
            Ok(fat_entry) => {
                match fat_entry.status() {
                    Status::Eoc(_) => Ok(None),
                    Status::Free | Status::Reserved | Status::Bad => ioerr!(InvalidData, "bad sector in chain"),
                    Status::Data(next_cluster) => Ok(Some(next_cluster))
                }
            }
        }
    }

    /// A method to read all of the clusters chained from a starting cluster
    /// into a vector.
    pub fn read_chain(&mut self, start: Cluster, buf: &mut Vec<u8>) -> io::Result<usize> {
        let cluster_size = self.cluster_size();
        let mut i = 0;
        let mut current = Some(start);
        while let Some(cluster) = current {
            buf.resize(i + cluster_size, 0);
            i += self.read_cluster(cluster, 0, &mut buf[i..])?;
            current = self.next_cluster(cluster)?;
        }
        buf.truncate(i);
        Ok(i)
    }
    
    /// A method to return a reference to a `FatEntry` for a cluster where the
    /// reference points directly into a cached sector.
    pub fn fat_entry(&mut self, cluster: Cluster) -> io::Result<&FatEntry> {
        let entries_per_sector = self.bytes_per_sector as usize / core::mem::size_of::<FatEntry>();
        let fat_sector = self.fat_start_sector as u64 + cluster.raw_value() as u64 / entries_per_sector as u64;
        let offset = cluster.raw_value() as usize % entries_per_sector as usize;
        Ok(unsafe { &self.device.get(fat_sector)?.cast()[offset] }) // cast from [u8] to [FatEntry]
    }
}

impl<'a, HANDLE: VFatHandle> FileSystem for &'a HANDLE {
    type File = File<HANDLE>;
    type Dir = Dir<HANDLE>;
    type Entry = Entry<HANDLE>;

    fn open<P: AsRef<Path>>(self, path: P) -> io::Result<Self::Entry> {
        if !path.as_ref().is_absolute() {
            return ioerr!(InvalidInput, "path is not absolute");
        }
        let mut entry = Entry::Dir(Dir::from_root(self.clone()));
        for component in path.as_ref().components() {
            if component == path::Component::RootDir {
                continue;
            }
            if let Some(dir) = crate::traits::Entry::into_dir(entry) {
                entry = crate::traits::Dir::find(&dir, component)?;
            } else {
                return ioerr!(InvalidInput, "not a directory");
            }
        }
        Ok(entry)
    }
}

use shim::io::{self, SeekFrom};
use shim::ioerr;
use shim::newioerr;

use crate::traits;
use crate::vfat::entry::EntryInfo;
use crate::vfat::{Cluster, VFatHandle};

#[derive(Debug)]
pub struct File<HANDLE: VFatHandle> {
    pub entry_info: EntryInfo<HANDLE>,
    pub filesize: u64,

    filepos: u64,
    cur_cluster: Option<Cluster>
}

impl<HANDLE: VFatHandle> File<HANDLE> {
    pub fn from(entry_info: EntryInfo<HANDLE>, filesize: u64) -> File<HANDLE> {
        // println!("{}: size {} cluster {:?}", entry_info.name, filesize, entry_info.cluster);
        let start_cluster = entry_info.cluster;
        File{
            entry_info,
            filesize,
            filepos: 0,
            cur_cluster: Some(start_cluster)
        }
    }

    fn cur_cluster(&self) -> io::Result<Cluster> {
        self.cur_cluster.ok_or(newioerr!(InvalidData, "broken fat cluster chain"))
    }

    fn seek_chain_forward(&mut self, mut delta: u64) -> io::Result<()> {
        assert!(self.filepos + delta <= self.filesize);
        let cluster_size = self.entry_info.vfat.lock(|vfat| vfat.cluster_size()) as u64;
        // println!(" Seek forward {}, cur_pos = {}/{}, cluster_size = {}, cur_cluster = {:?}", delta, self.filepos, self.filesize, cluster_size, self.cur_cluster);
        while delta >= cluster_size - (self.filepos % cluster_size) {
            self.cur_cluster = self.entry_info.vfat.lock(|vfat| vfat.next_cluster(self.cur_cluster()?))?;
            let advance = core::cmp::min(delta, cluster_size);
            delta -= advance;
            self.filepos += advance;
            assert!(self.filepos <= self.filesize);
            // println!("  +{}, cur_pos = {}, cur_cluster = {:?}", advance, self.filepos, self.cur_cluster);
        }
        self.filepos += delta;
        assert!(self.filepos <= self.filesize);
        Ok(())
    }

    fn seek_chain(&mut self, mut delta: i64) -> io::Result<()> {
        if delta < 0 { // singly-linked list; start over.
            self.cur_cluster = Some(self.entry_info.cluster);
            delta += self.filepos as i64; // convert to absolute
            self.filepos = 0;
        }
        assert!(delta >= 0);
        self.seek_chain_forward(delta as u64)
    }

    fn remaining_bytes(&self) -> usize {
        self.filesize as usize - self.filepos as usize
    }
}

impl<HANDLE: VFatHandle> io::Seek for File<HANDLE> {
    /// Seek to offset `pos` in the file.
    ///
    /// A seek to the end of the file is allowed. A seek _beyond_ the end of the
    /// file returns an `InvalidInput` error.
    ///
    /// If the seek operation completes successfully, this method returns the
    /// new position from the start of the stream. That position can be used
    /// later with SeekFrom::Start.
    ///
    /// # Errors
    ///
    /// Seeking before the start of a file or beyond the end of the file results
    /// in an `InvalidInput` error.
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::End(n) => self.filesize as i64 + n,
            SeekFrom::Current(n) => self.filepos as i64 + n
        };
        if new_pos < 0 || new_pos as u64 > self.filesize {
            return ioerr!(InvalidInput, "seek past beyond of file");
        }
        // println!("{}: Seek to {}", self.entry_info.name, new_pos);
        self.seek_chain(new_pos - self.filepos as i64)?;
        assert_eq!(self.filepos, new_pos as u64);
        return Ok(self.filepos);
    }
}

impl<HANDLE: VFatHandle> io::Read for File<HANDLE> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // println!("{}: Read of len {} at {}/{} requested", self.entry_info.name, buf.len(), self.filepos, self.filesize);
        let cluster_size = self.entry_info.vfat.lock(|vfat| vfat.cluster_size()) as usize;
        let read_len = core::cmp::min(buf.len(), self.remaining_bytes());
        let mut n = 0;
        while n < read_len {
            let cluster_offset = self.filepos as usize % cluster_size as usize;
            let mut n_read = self.entry_info.vfat.lock(|vfat| vfat.read_cluster(self.cur_cluster()?, cluster_offset, &mut buf[n..]))?;
            // println!(" Read returns {}", n_read);
            n_read = core::cmp::min(n_read, self.remaining_bytes()); // Don't report reading past the end of the file
            self.seek_chain_forward(n_read as u64)?;
            // println!(" Now at {}/{}", self.filepos, self.filesize);
            n += n_read;
        }
        // println!(" {} bytes actually read", n);
        assert!(n <= buf.len());
        Ok(n)
    }
}

impl<HANDLE: VFatHandle> io::Write for File<HANDLE> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        ioerr!(PermissionDenied, "sorry, this filesystem is read-only")
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<HANDLE: VFatHandle> traits::File for File<HANDLE> {
    fn sync(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn size(&self) -> u64 {
        self.filesize
    }
}

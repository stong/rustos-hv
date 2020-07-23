#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone, Hash)]
pub struct Cluster(u32);

impl From<u32> for Cluster {
    fn from(raw_num: u32) -> Cluster {
        Cluster(raw_num & !(0xF << 28))
    }
}

impl Cluster {
    pub fn raw_value(&self) -> u32 {
        return self.0;
    }

    pub fn logical_value(&self) -> u32 {
        return self.0 - 2;
    }
}

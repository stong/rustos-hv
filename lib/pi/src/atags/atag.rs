use crate::atags::raw;

pub use crate::atags::raw::{Core, Mem};

/// An ATAG.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Atag {
    Core(raw::Core),
    Mem(raw::Mem),
    Cmd(&'static str),
    Unknown(u32),
    None,
}

impl Atag {
    /// Returns `Some` if this is a `Core` ATAG. Otherwise returns `None`.
    pub fn core(self) -> Option<Core> {
        if let Atag::Core(s) = self {
            Some(s)
        } else {
            None
        }
    }

    /// Returns `Some` if this is a `Mem` ATAG. Otherwise returns `None`.
    pub fn mem(self) -> Option<Mem> {
        if let Atag::Mem(s) = self {
            Some(s)
        } else {
            None
        }
    }

    /// Returns `Some` with the command line string if this is a `Cmd` ATAG.
    /// Otherwise returns `None`.
    pub fn cmd(self) -> Option<&'static str> {
        if let Atag::Cmd(s) = self {
            Some(s)
        } else {
            None
        }
    }
}

// FIXME: Implement `From<&raw::Atag> for `Atag`.
impl From<&'static raw::Atag> for Atag {
    fn from(atag: &'static raw::Atag) -> Atag {
        unsafe {
            match (atag.tag, &atag.kind) {
                (raw::Atag::CORE, &raw::Kind { core }) => Atag::Core{0: core},
                (raw::Atag::MEM, &raw::Kind { mem }) => Atag::Mem{0: mem},
                (raw::Atag::CMDLINE, &raw::Kind { ref cmd }) => {
                    let raw = &cmd.cmd as *const u8; // &cmd.cmd NOT cmd.cmd!
                    let mut i = 0;
                    while *(raw.offset(i)) != 0 {
                        i += 1;
                    }
                    Atag::Cmd{0: core::str::from_utf8_unchecked(core::slice::from_raw_parts(raw, i as usize))}
                },
                (raw::Atag::NONE, _) => Atag::None,
                (id, _) => Atag::Unknown{0: id},
            }
        }
    }
}

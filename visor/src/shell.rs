use shim::io;
use shim::ioerr;
use shim::path::{Path, PathBuf, Component};
use alloc::string::String;

use core::time::Duration;

use stack_vec::StackVec;
use core::fmt::Write as FmtWrite;
use shim::io::Write as IoWrite;

use pi::atags::Atags;
use pi::timer;
use pi::power;

use fat32::traits::FileSystem;
use fat32::traits::{Dir, Entry, Metadata, Timestamp};

use crate::fs;
use crate::console::{kprint, kprintln, CONSOLE};
use crate::ALLOCATOR;
use crate::FILESYSTEM;

/// Error type for `Command` parse failures.
#[derive(Debug)]
enum Error {
    Empty,
    TooManyArgs,
}

/// A structure representing a single shell command.
struct Command<'a> {
    args: StackVec<'a, &'a str>,
}

impl<'a> Command<'a> {
    /// Parse a command from a string `s` using `buf` as storage for the
    /// arguments.
    ///
    /// # Errors
    ///
    /// If `s` contains no arguments, returns `Error::Empty`. If there are more
    /// arguments than `buf` can hold, returns `Error::TooManyArgs`.
    fn parse(s: &'a str, buf: &'a mut [&'a str]) -> Result<Command<'a>, Error> {
        let mut args = StackVec::new(buf);
        for arg in s.split(' ').filter(|a| !a.is_empty()) {
            args.push(arg).map_err(|_| Error::TooManyArgs)?;
        }

        if args.is_empty() {
            return Err(Error::Empty);
        }

        Ok(Command { args })
    }

    /// Returns this command's path. This is equivalent to the first argument.
    fn path(&self) -> &str {
        &self.args[0]
    }
}

pub struct Shell<'a, FS: FileSystem + Copy> {
    prefix: &'a str,
    cur_path: PathBuf,
    fs: FS
}

impl<'a> Shell<'a, &fs::FileSystem> {
    pub fn new(prefix: &str) -> Shell<'_, &fs::FileSystem> {
        let dir = FILESYSTEM.open_dir(Path::new("/")).expect("missing root");
        let mut path = PathBuf::new();
        path.push(Component::RootDir);
        Shell{
            prefix,
            cur_path: path,
            fs: &FILESYSTEM
        }
    }
}

fn canonicalize(path: PathBuf) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => (),
            Component::ParentDir => { result.pop(); },
            other => result.push(other)
        }
    }
    result
}

// stolen from unit test code lol
fn hash_entry<T: Entry>(hash: &mut String, entry: &T) -> core::fmt::Result {
    use core::fmt::Write;

    fn write_bool(to: &mut String, b: bool, c: char) -> core::fmt::Result {
        if b {
            write!(to, "{}", c)
        } else {
            write!(to, "-")
        }
    }

    fn write_timestamp<T: Timestamp>(to: &mut String, ts: T) -> core::fmt::Result {
        write!(
            to,
            "{:02}/{:02}/{} {:02}:{:02}:{:02} ",
            ts.month(),
            ts.day(),
            ts.year(),
            ts.hour(),
            ts.minute(),
            ts.second()
        )
    }

    write_bool(hash, entry.is_dir(), 'd')?;
    write_bool(hash, entry.is_file(), 'f')?;
    write_bool(hash, entry.metadata().read_only(), 'r')?;
    write_bool(hash, entry.metadata().hidden(), 'h')?;
    write!(hash, "\t")?;

    write_timestamp(hash, entry.metadata().created())?;
    write_timestamp(hash, entry.metadata().modified())?;
    write_timestamp(hash, entry.metadata().accessed())?;
    write!(hash, "\t")?;

    write!(hash, "{}", entry.name())?;
    Ok(())
}

impl<'a, FS: FileSystem + Copy> Shell<'a, FS> {
    fn open_relative(&mut self, path: PathBuf) -> io::Result<(PathBuf, FS::Entry)> {
        let mut abs_path: PathBuf;
        if path.is_absolute() {
            abs_path = path;
        } else {
            abs_path = PathBuf::new();
            abs_path.push(self.cur_path.clone());
            abs_path.push(path);
        }
        abs_path = canonicalize(abs_path);
        self.fs.open(&abs_path).map(|ok| (abs_path, ok))
    }

    fn chdir(&mut self, path: PathBuf) -> io::Result<()> {
        self.cur_path = self.open_relative(path)?.0;
        Ok(())
    }

    fn pretty_print(&self, entry: FS::Entry) {
        let mut e = String::new();
        hash_entry(&mut e, &entry).unwrap();
        kprintln!("{}", e);
    }

    fn ls(&mut self, cmd: Command) -> io::Result<()> {
        let mut show_hidden = false;
        let mut path = PathBuf::from("");
        for &arg in cmd.args.iter().skip(1) {
            if arg == "-a" {
                show_hidden = true;
            } else {
                path = PathBuf::from(arg);
            }
        }
        let (_, file) = self.open_relative(path)?;
        if file.is_dir() {
            for entry in file.into_dir().unwrap().entries()? {
                if !entry.metadata().hidden() || show_hidden {
                    self.pretty_print(entry);
                }
            }
        } else {
            self.pretty_print(file);
        }
        Ok(())
    }

    fn cat(&mut self, cmd: Command) -> io::Result<()> {
        for &arg in cmd.args.iter().skip(1) {
            match self.open_relative(PathBuf::from(arg)) {
                Err(e) => {
                    kprintln!("cat: {}: {}", arg, e);
                    continue
                },
                Ok((_, entry)) => {
                    if entry.is_dir() {
                        kprintln!("cat: {}: Is a directory", arg);
                        continue
                    }
                    let mut file = entry.into_file().unwrap();
                    shim::io::copy(&mut file, &mut *CONSOLE.lock())?;
                }
            }
        }
        Ok(())
    }

    fn call_command(&mut self, cmd: Command) -> io::Result<()> {
        match cmd.path() {
            "echo" => {
                for arg in cmd.args.iter().skip(1) {
                    kprint!("{} ", arg);
                }
                kprintln!();
                Ok(())
            },
            "reboot" => {
                kprint!("Time to die.");
                panic!("Goodnight");
            },
            "help" => {
                for _ in 0..3 {
                    kprint!(".");
                    timer::spin_sleep(Duration::from_millis(1000));
                }
                kprintln!("\nYou cry out for help, but no one seems to care.");
                Ok(())
            },
            "cd" => {
                if let Some(arg) = cmd.args.iter().nth(1) {
                    self.chdir(Path::new(arg).to_path_buf())
                } else {
                    kprintln!("usage: cd <directory>");
                    Ok(())
                }
            },
            "pwd" => {
                kprintln!("{}", self.cur_path.display());
                Ok(())
            },
            "ls" => self.ls(cmd),
            "cat" => self.cat(cmd),
            command => {
                kprintln!("unknown command: {}", command);
                Ok(())
            }
        }
    }

    fn on_command(&mut self, cmd: Command) {
        match self.call_command(cmd) {
            Err(e) => {
                kprintln!("error: {}", e);
            },
            Ok(_) => ()
        }
    }
        
    /// Starts a shell using `prefix` as the prefix for each line. This function
    /// never returns.
    pub fn do_forever(&mut self) -> ! {
        loop {
            self.do_cmd();
        }
    }

    pub fn do_cmd(&mut self) {
        let mut line_buf = [0 as u8; 512];
        let mut line = StackVec::new(&mut line_buf);
        kprint!("{} {}", self.cur_path.display(), self.prefix);
        loop {
            let b = CONSOLE.lock().read_byte();
            if b == b'\r' || b == b'\n' { // return
                CONSOLE.lock().write(&[b'\r', b'\n']).unwrap();
                let mut args_buf: [&str; 64] = [""; 64];
                // we know for sure it will be valid utf-8... only printables were added
                match Command::parse(core::str::from_utf8(&line).unwrap(), &mut args_buf) {
                    Ok(cmd) => self.on_command(cmd),
                    Err(Error::Empty) => {},
                    Err(Error::TooManyArgs) => kprintln!("error: too many arguments")
                };
                line.truncate(0);
                break;
            } else if b == 0x08 || b == 0x7f { // backspace
                match line.pop() {
                    Some(_) => { CONSOLE.lock().write(&[0x08, b' ', 0x08]).unwrap(); },
                    None => {}
                };
            } else if b == 0x12 { // ^R reboot
                panic!("Goodnight");
            } else if b < 0x20 || b > 0x7f {
                CONSOLE.lock().write_byte(0x07); // ring terminal bell
            } else {
                match line.push(b) {
                    Ok(_) => CONSOLE.lock().write_byte(b),
                    Err(_) => {}
                };
            }
        }
    }
}

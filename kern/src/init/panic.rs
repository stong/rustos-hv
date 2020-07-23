use core::panic::PanicInfo;

use crate::console::{kprintln,kprint,CONSOLE};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    kprint!("\n\nBruh moment: ");
    if let Some(s) = _info.message() {
        kprintln!("{:?}", s);
    } else if let Some(s) = _info.payload().downcast_ref::<&str>() {
        kprintln!("{:?}", s);
    } else {
        kprintln!("Unknown message or payload");
    }
    if let Some(s) = _info.location() {
        kprintln!("At file \"{}\", line {}", s.file(), s.line());
    }

    kprintln!("\n       m(_ _)m       ");
    kprintln!("Strike RETURN to reboot");
    loop {
        if CONSOLE.lock().read_byte() == b'\r' {
            use shim::io::Write;
            use pi::power;
            kprintln!("oyasumi.");
            CONSOLE.lock().flush();
            unsafe { power::PowerManager::new().reset(); }
        }
    }
}

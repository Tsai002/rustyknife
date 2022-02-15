#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = rustyknife::rfc5321::mailbox::<rustyknife::behaviour::Intl>(data);
});

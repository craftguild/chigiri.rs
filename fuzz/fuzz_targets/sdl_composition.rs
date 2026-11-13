#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| chigiri_fuzz::sdl_composition(data));

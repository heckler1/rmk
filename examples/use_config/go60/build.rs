//! Puts `memory.x` on the linker search path and selects the linker scripts.

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::env;

fn main() {
    println!("cargo:rerun-if-changed=keyboard.toml");

    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(include_bytes!("memory.x"))
        .unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory.x");

    println!("cargo:rustc-link-arg=--nmagic");
    println!("cargo:rustc-link-arg=-Tlink.x");
    println!("cargo:rustc-link-arg=-Tdefmt.x");
}

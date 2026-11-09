//! Compiles the MediaRemote helper that `now_playing.rs` embeds and runs inside `/usr/bin/perl`.

use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=macos/now_playing_helper.m");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("now_playing_helper.dylib");
    let status = Command::new("clang")
        .args(["-dynamiclib", "-fobjc-arc", "-framework", "AppKit", "-o"])
        .arg(&out)
        .arg("macos/now_playing_helper.m")
        .status()
        .expect("clang (Xcode command line tools) is required");
    assert!(status.success(), "compiling the now-playing helper failed");
}

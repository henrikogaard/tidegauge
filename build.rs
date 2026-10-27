//! Compiles widgets/Center/WidgetCenter.swift into a static library, so the
//! app can ask WidgetKit to reload the desktop widgets.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let source = "widgets/Center/WidgetCenter.swift";
    println!("cargo:rerun-if-changed={source}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        Ok(other) => panic!("no Swift target for {other}"),
        Err(_) => panic!("CARGO_CFG_TARGET_ARCH is not set"),
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is not set"));
    let status = Command::new("xcrun")
        .args([
            "swiftc",
            "-emit-library",
            "-static",
            "-parse-as-library",
            "-O",
        ])
        .args(["-module-name", "TidegaugeWidgetCenter"])
        .args(["-target", &format!("{arch}-apple-macos11.0")])
        .arg(source)
        .arg("-o")
        .arg(out.join("libtidegauge_widget_center.a"))
        .status()
        .expect("xcrun swiftc could not run; install the Xcode command line tools");
    assert!(status.success(), "swiftc failed on {source}");

    // The Swift runtime ships with macOS; the compatibility shims the
    // library may pull in ship with the toolchain.
    let toolchain = Command::new("xcrun")
        .args(["--find", "swiftc"])
        .output()
        .expect("xcrun --find swiftc could not run");
    let swiftc = PathBuf::from(String::from_utf8_lossy(&toolchain.stdout).trim());
    let shims = swiftc
        .parent()
        .and_then(|bin| bin.parent())
        .map(|usr| usr.join("lib/swift/macosx"));
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=tidegauge_widget_center");
    println!("cargo:rustc-link-search=native=/usr/lib/swift");
    if let Some(shims) = shims {
        println!("cargo:rustc-link-search=native={}", shims.display());
    }
    println!("cargo:rustc-link-lib=framework=WidgetKit");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}

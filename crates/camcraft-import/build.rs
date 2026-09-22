//! Link what OpenCascade needs from Windows but `occt-sys` does not ask for.
//!
//! Found in the STEP spike (docs/decisions/0001-step-backend.md): registry and SID
//! symbols from advapi32 go unresolved on MSVC without this. If a Windows build still
//! reports unresolved externals, the library that defines them belongs here too.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=advapi32");
    }
}

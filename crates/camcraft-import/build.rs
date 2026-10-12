//! Link what OpenCascade needs from Windows but `occt-sys` does not ask for, when
//! the `opencascade` feature builds it.
//!
//! Found in the STEP spike (docs/decisions/0001-step-backend.md): registry and SID
//! symbols from advapi32 go unresolved on MSVC without this. If a Windows build still
//! reports unresolved externals, the library that defines them belongs here too.

fn main() {
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if windows && std::env::var_os("CARGO_FEATURE_OPENCASCADE").is_some() {
        println!("cargo:rustc-link-lib=advapi32");
    }
}

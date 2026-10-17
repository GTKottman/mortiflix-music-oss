//! Diagnostics: which decoder MFTs and DXVA profiles does this machine offer? `mfcaps`
#[cfg(target_os = "windows")]
fn main() {
    println!("{}", mortiflix_platform::media_foundation::capabilities());
}
#[cfg(not(target_os = "windows"))]
fn main() {}

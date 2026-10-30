// The CEF runtime is found next to the host (`cef/`, the preview in ~/rare-native) or in the image
// (/usr/lib/rare/cef), without LD_LIBRARY_PATH.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN/cef:/usr/lib/rare/cef");
    }
}

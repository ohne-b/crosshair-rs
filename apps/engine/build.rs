#[path = "../../packaging/windows/icon.rs"]
mod icon;

fn main() {
    icon::embed();
    if std::env::var("TARGET").unwrap().contains("windows-msvc") {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}

pub fn embed() {
    use std::{env, fs, path::PathBuf, process::Command};

    println!("cargo:rerun-if-changed=../../assets/app.ico");
    println!("cargo:rerun-if-changed=../../packaging/windows/icon.rs");
    if !env::var("TARGET").unwrap().contains("windows-msvc") {
        return;
    }
    let icon = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../assets/app.ico")
        .canonicalize()
        .expect("Application icon must exist");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let resource = output.join("app.rc");
    let compiled = output.join("app.res");
    fs::write(
        &resource,
        format!(
            "1 ICON \"{}\"\n",
            icon.display().to_string().replace('\\', "/")
        ),
    )
    .expect("Write icon resource");
    let compiler = PathBuf::from(env::var_os("ProgramFiles(x86)").unwrap())
        .join("Windows Kits/10/bin/10.0.26100.0/x64/rc.exe");
    let status = Command::new(compiler)
        .args(["/nologo", "/c65001", "/fo"])
        .arg(&compiled)
        .arg(&resource)
        .status()
        .expect("Windows SDK 10.0.26100 resource compiler is required");
    assert!(status.success(), "Application icon compilation failed");
    println!("cargo:rustc-link-arg={}", compiled.display());
}

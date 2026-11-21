pub fn embed(role: &str) {
    use std::{env, path::PathBuf, process::Command};

    println!("cargo:rerun-if-changed=../../assets/app.ico");
    println!("cargo:rerun-if-changed=../../packaging/windows/icon.rs");
    println!("cargo:rerun-if-changed=../../packaging/windows/process.rc");
    if !env::var("TARGET").unwrap().contains("windows-msvc") {
        return;
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let compiled = output.join("app.res");
    let compiler = PathBuf::from(env::var_os("ProgramFiles(x86)").unwrap())
        .join("Windows Kits/10/bin/10.0.26100.0/x64/rc.exe");
    let status = Command::new(compiler)
        .args([
            "/nologo",
            "/c65001",
            "/i",
            "../../assets",
            "/d",
            role,
            "/fo",
        ])
        .arg(&compiled)
        .arg("../../packaging/windows/process.rc")
        .status()
        .expect("Windows SDK 10.0.26100 resource compiler is required");
    assert!(status.success(), "Application icon compilation failed");
    println!("cargo:rustc-link-arg={}", compiled.display());
}

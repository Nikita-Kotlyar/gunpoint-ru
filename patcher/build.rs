use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-env-changed=GUNPOINT_RU_NO_MANIFEST");
    // Embed the requireAdministrator manifest only into release builds.
    // Test/debug binaries must stay runnable without UAC.
    if std::env::var("PROFILE").unwrap_or_default() != "release" {
        return;
    }
    if std::env::var_os("GUNPOINT_RU_NO_MANIFEST").is_some() {
        return;
    }
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let obj = format!("{}/app_res.o", out);
    let windres = r"C:\msys64\mingw64\bin\windres.exe";
    let status = Command::new(windres)
        .args(["-i", "app.rc", "-o", &obj, "-O", "coff"])
        .status()
        .unwrap_or_else(|e| panic!("windres not found ({}): {}", windres, e));
    assert!(status.success(), "windres failed");
    println!("cargo:rustc-link-arg={}", obj);
}

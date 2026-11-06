fn main() {
    slint_build::compile("ui/app.slint").expect("slint compile failed");
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let v = std::process::Command::new(rustc)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown rustc".into());
    println!("cargo:rustc-env=RUSTC_VERSION={}", v.trim());
}

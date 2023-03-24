fn main() {
    let ios_target = std::env::var("IPHONEOS_DEPLOYMENT_TARGET")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(14.0)
        .max(14.0);
    std::env::set_var("IPHONEOS_DEPLOYMENT_TARGET", format!("{:.1}", ios_target));
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=native/picked.m");
    cc::Build::new()
        .file("native/picker.m")
        .flag("-fobjc-arc")
        .flag("-std=c11")
        .compile("picker");
}

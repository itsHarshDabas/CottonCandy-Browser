fn main() {
    // libcef.so sits next to the executable on Linux.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
    }
    // The window and taskbar icon on Windows.
    // In Explorer and on shortcuts, the .exe's own icon.
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/cottoncandy.ico");
        println!("cargo:rerun-if-changed=assets/cottoncandy.manifest");
        let mut res = winresource::WindowsResource::new();
        // Says which Windows versions it supports; without it Chromium sees Windows 8 and turns the GPU off.
        res.set_manifest_file("assets/cottoncandy.manifest");
        res.set_icon("assets/cottoncandy.ico")
            .set("FileDescription", "CottonCandy")
            .set("ProductName", "CottonCandy")
            .set("CompanyName", "GraceOS");
        res.compile().expect("Windows resources");
    }
}

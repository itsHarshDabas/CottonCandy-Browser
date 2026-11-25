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
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/cottoncandy.ico")
            .set("FileDescription", "CottonCandy")
            .set("ProductName", "CottonCandy")
            .set("CompanyName", "GraceOS");
        res.compile().expect("Windows resources");
    }
}

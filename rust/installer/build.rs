fn main() {
    // On Windows: the CottonCandy icon, the version in the file's properties, and a manifest that
    // runs the installer as the current user (it asks for administrator only for a folder that needs
    // it), sharp on high-DPI screens, with the modern look for the folder picker.
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=../app/assets/cottoncandy.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../app/assets/cottoncandy.ico")
            .set("FileDescription", "CottonCandy Setup")
            .set("ProductName", "CottonCandy")
            .set("CompanyName", "GraceOS")
            .set("LegalCopyright", "GPL-3.0-or-later")
            .set_manifest(
                r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security>
  </trustInfo>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
  <dependency><dependentAssembly>
    <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
  </dependentAssembly></dependency>
</assembly>"#,
            );
        res.compile().expect("Windows resources");
    }
}

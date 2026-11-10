// Embeds the app icon and version info into the .exe (what Explorer, the
// taskbar, and Start show).
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/disc-icon.ico");
        res.set("ProductName", "Disc Lite");
        res.set("FileDescription", "Disc Lite");
        if let Err(e) = res.compile() {
            // Missing Windows SDK resource compiler: still build, just
            // without the embedded icon.
            println!("cargo:warning=couldn't embed the app icon: {e}");
        }
    }
}

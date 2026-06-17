//! Build script: on Windows, embed the app icon as a Win32 resource so the
//! `.exe` shows the Freally Snipper icon in Explorer / the taskbar (the runtime
//! `with_icon` only covers the live window, not the file on disk). No-op on other
//! platforms.

fn main() {
    #[cfg(windows)]
    {
        const ICON: &str = "assets/Freally_Snipper_Icon_Light.ico";
        println!("cargo:rerun-if-changed={ICON}");
        let mut res = winresource::WindowsResource::new();
        res.set_icon(ICON);
        if let Err(err) = res.compile() {
            // Don't fail the build if the resource compiler is unavailable — the
            // app still runs, just without the embedded .exe icon.
            println!("cargo:warning=could not embed Windows icon resource: {err}");
        }
    }
}

fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "PhoneSync");
        res.set("FileDescription", "PhoneSync - phone to laptop sync");
        res.set("LegalCopyright", "MIT License");
        res.set("OriginalFilename", "phonesync.exe");
        if let Err(err) = res.compile() {
            // A missing resource compiler should not stop the build; the app
            // still runs, it just falls back to the generic Windows icon.
            println!("cargo:warning=could not embed the Windows icon: {err}");
        }
    }
}

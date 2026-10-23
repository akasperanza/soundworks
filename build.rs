fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/app.ico");
        res.set("ProductName", "Soundworks");
        res.set("FileDescription", "Soundworks - bit-perfect music player");
        res.set("InternalName", "Soundworks");
        res.set("OriginalFilename", "Soundworks.exe");
        res.set("CompanyName", "Soundworks");
        res.set("LegalCopyright", "Copyright (c) Soundworks contributors");
        if let Err(e) = res.compile() {

            println!("cargo:warning=exe simgesi gömülemedi: {e}");
        }
    }
}

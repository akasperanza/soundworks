fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/app.ico");
        res.set("ProductName", "Soundworks");
        res.set("FileDescription", "Soundworks");
        if let Err(e) = res.compile() {

            println!("cargo:warning=exe simgesi gömülemedi: {e}");
        }
    }
}

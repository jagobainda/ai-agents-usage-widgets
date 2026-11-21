fn main() -> std::io::Result<()> {
    let description = env!("CARGO_PKG_DESCRIPTION");
    let exe = description.replace(' ', "");
    let icon = format!("assets/{exe}.ico");
    println!("cargo:rerun-if-changed={icon}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set("ProductName", description)
            .set("FileDescription", description)
            .set("OriginalFilename", &format!("{exe}.exe"))
            .set_icon(&icon)
            .compile()?;
    }
    Ok(())
}

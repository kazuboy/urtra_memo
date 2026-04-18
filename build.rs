fn main() {
    if std::env::var("CARGO_CFG_TARGET_ARCH").ok().as_deref() == Some("wasm32") {
        return;
    }

    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("app.ico");
        res.compile().expect("failed to compile windows resources");
    }
}

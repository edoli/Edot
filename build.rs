fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon/edot.rc");
    println!("cargo:rerun-if-changed=assets/app-icon/edot.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // GPUI loads icon resource 1 from the executable for its window class.
        embed_resource::compile_for(
            "assets/app-icon/edot.rc",
            ["edot"],
            embed_resource::ParamsIncludeDirs(["assets/app-icon"]),
        )
        .manifest_required()
        .expect("Edot app icon resource must compile");
    }
}

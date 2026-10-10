fn main() {
    for name in ["EDOT_REPOSITORY", "EDOT_BUILD_LABEL"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let parts: Vec<u16> = version
        .split('.')
        .map(|part| {
            assert!(!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
            assert!(part == "0" || !part.starts_with('0'));
            part.parse().expect("Version components must fit in u16")
        })
        .collect();
    assert_eq!(parts.len(), 3, "Cargo version must be major.minor.patch");
    let repository = std::env::var("EDOT_REPOSITORY").unwrap_or_else(|_| "edoli/Edot".into());
    assert!(repository.split('/').count() == 2);
    assert!(repository.split('/').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    }));
    println!("cargo:rustc-env=EDOT_APP_VERSION={version}");
    println!("cargo:rustc-env=EDOT_RELEASE_REPOSITORY={repository}");
    println!(
        "cargo:rustc-env=EDOT_BUILD_LABEL={}",
        std::env::var("EDOT_BUILD_LABEL").unwrap_or_default()
    );
    println!("cargo:rerun-if-changed=assets/app-icon/edot.rc");
    println!("cargo:rerun-if-changed=assets/app-icon/edot.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // GPUI loads icon resource 1 from the executable for its window class.
        let resource =
            std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("edot-version.rc");
        std::fs::write(
            &resource,
            format!(
                r#"
#include "edot.rc"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "FileDescription", "Edot"
      VALUE "FileVersion", "{version}"
      VALUE "ProductName", "Edot"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "edot.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
                major = parts[0],
                minor = parts[1],
                patch = parts[2]
            ),
        )
        .unwrap();
        embed_resource::compile_for(
            resource,
            ["edot"],
            embed_resource::ParamsIncludeDirs(["assets/app-icon"]),
        )
        .manifest_required()
        .expect("Edot app icon resource must compile");
    }
}

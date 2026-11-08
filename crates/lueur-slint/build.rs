//! Compiles the interface in `ui/` into Rust, with the icons, the face and
//! the pictures it names built into the binary, and on Windows gives the
//! executable its icon and version.
//!
//! The icons are looked up by name at run time (a light's mode, a page of
//! the sidebar), which Slint cannot do with a file path: this writes
//! `icons.slint`, one global whose `get` turns a name and a weight into the
//! picture, from what `assets/icons` holds, so a glyph added to the folder
//! needs no code.

use std::fmt::Write as _;
use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it");
    let out = std::env::var("OUT_DIR").expect("cargo sets it");
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-changed=ui");

    write_icons(Path::new(&manifest), Path::new(&out));
    write_photos(Path::new(&manifest), Path::new(&out));

    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent".into())
        .with_include_paths(vec![Path::new(&out).to_path_buf()])
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
    slint_build::compile_with_config("ui/app.slint", config).expect("the interface compiles");

    if std::env::var("TARGET").is_ok_and(|t| t.contains("windows")) {
        windows_resources(&manifest);
    }

    if std::env::var("TARGET").is_ok_and(|t| t.ends_with("windows-msvc")) {
        // DLLs imported at load time come from System32 only, never from the
        // folder the setup file was started in (LOAD_LIBRARY_SEARCH_SYSTEM32),
        // as the Tauri build did.
        println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
    }
}

/// `icons.slint`: `Icons.get("bulb", "regular")` is `assets/icons/bulb-regular.svg`.
fn write_icons(manifest: &Path, out: &Path) {
    let dir = manifest.join("assets").join("icons");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .expect("assets/icons")
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".svg"))
        .collect();
    files.sort();
    let mut slint = String::from(
        "// Written by build.rs from assets/icons: do not edit.\n\
         export global Icons {\n    public pure function get(name: string, weight: string) -> image {\n",
    );
    for file in &files {
        let stem = file.trim_end_matches(".svg");
        let Some((name, weight)) = stem.rsplit_once('-') else {
            continue;
        };
        let path = dir.join(file).display().to_string().replace('\\', "/");
        writeln!(
            slint,
            "        if (name == {name:?} && weight == {weight:?}) {{ return @image-url({path:?}); }}"
        )
        .expect("writing to a string");
    }
    slint.push_str("        return @image-url(\"\");\n    }\n}\n");
    std::fs::write(out.join("icons.slint"), slint).expect("OUT_DIR is writable");
}

/// `photos.rs`: the room's photographs, by name, for `src/images.rs`.
fn write_photos(manifest: &Path, out: &Path) {
    let dir = manifest.join("assets").join("modes");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .expect("assets/modes")
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".webp"))
        .collect();
    files.sort();
    let mut table = String::from("pub static PHOTOS: &[(&str, &[u8])] = &[\n");
    for file in files {
        let name = file.trim_end_matches(".webp");
        let path = dir.join(&file).display().to_string().replace('\\', "/");
        writeln!(table, "    ({name:?}, include_bytes!({path:?})),").expect("writing to a string");
    }
    table.push_str("];\n");
    std::fs::write(out.join("photos.rs"), table).expect("OUT_DIR is writable");
}

/// The executable's icon and version, as the Tauri builds carried them:
/// Explorer, the taskbar and Installed apps show these, and the setup file
/// is this very executable. Written as a resource script and compiled with
/// the toolchain's resource compiler.
fn windows_resources(manifest: &str) {
    let version = std::env::var("CARGO_PKG_VERSION").expect("cargo sets it");
    let mut parts = version
        .split(['.', '-', '+'])
        .map(|p| p.parse::<u16>().unwrap_or(0));
    let numbers: Vec<String> = (0..4)
        .map(|_| parts.next().unwrap_or(0).to_string())
        .collect();
    let numbers = numbers.join(",");
    let icon = Path::new(manifest).join("assets").join("icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    // A resource script reads backslashes in a string as escapes.
    let icon = icon.display().to_string().replace('\\', "\\\\");
    // 32512 is IDI_APPLICATION: the icon Windows shows for the executable.
    // CompanyName is the second part of the identifier com.lueur.app, as
    // the Tauri builds had it.
    let script = format!(
        r#"32512 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numbers}
PRODUCTVERSION {numbers}
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "lueur"
      VALUE "FileDescription", "Lueur"
      VALUE "FileVersion", "{version}"
      VALUE "ProductName", "Lueur"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let out = Path::new(&std::env::var("OUT_DIR").expect("cargo sets it")).join("lueur.rc");
    std::fs::write(&out, script).expect("OUT_DIR is writable");
    if let Err(e) = embed_resource::compile(&out, embed_resource::NONE).manifest_optional() {
        panic!("cannot build the Windows resources: {e}");
    }
}

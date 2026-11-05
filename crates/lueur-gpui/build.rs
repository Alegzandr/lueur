//! Embeds `assets/` in the binary: writes the table `src/assets.rs` serves,
//! one `include_bytes!` per file, so a glyph added to the folder needs no
//! code.

use std::fmt::Write as _;
use std::path::Path;

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .expect("assets folder")
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(root, &path, out);
        } else if path
            .extension()
            .is_some_and(|e| e == "svg" || e == "png" || e == "webp" || e == "ttf")
        {
            let rel = path.strip_prefix(root).expect("under assets");
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it");
    let root = Path::new(&manifest).join("assets");
    println!("cargo:rerun-if-changed=assets");
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    let mut table = String::from("pub static ASSETS: &[(&str, &[u8])] = &[\n");
    for file in files {
        writeln!(
            table,
            "    ({file:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/assets/{file}\"))),"
        )
        .expect("writing to a string");
    }
    table.push_str("];\n");
    let out = Path::new(&std::env::var("OUT_DIR").expect("cargo sets it")).join("assets.rs");
    std::fs::write(out, table).expect("OUT_DIR is writable");

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

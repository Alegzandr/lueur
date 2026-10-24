fn main() {
    tauri_build::build();

    if std::env::var("TARGET").is_ok_and(|t| t.ends_with("windows-msvc")) {
        // DLLs imported at load time come from System32 only, never from the
        // folder the setup file was started in (LOAD_LIBRARY_SEARCH_SYSTEM32).
        println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
    }
}

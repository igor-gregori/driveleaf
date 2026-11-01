fn main() {
    // Imported DLLs resolve before `main` runs. This flag keeps them to System32,
    // so a DLL planted next to the program cannot load, even when elevated.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
    }
}

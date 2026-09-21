fn main() {
    println!("cargo:rerun-if-changed=src/tls_align.c");

    // Only where the loader demands it; see src/tls_align.c.
    let target = std::env::var("TARGET").unwrap_or_default();
    if target == "aarch64-linux-android" {
        cc::Build::new()
            .file("src/tls_align.c")
            // Without this the NDK emits `__emutls_v.*` - emulated TLS, which
            // allocates on the heap and never reaches the ELF TLS segment, so
            // the alignment this file exists to raise stays at 8.
            .flag("-fno-emulated-tls")
            .compile("probe_tls_align");
    }
}

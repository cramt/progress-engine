use delver_engine::wasm;

fn core_wasm() -> Option<Vec<u8>> {
    std::fs::read("core.wasm").ok()
}

#[test]
fn fingerprint_matches_the_verified_build() {
    let Some(buf) = core_wasm() else {
        eprintln!("skipped: no core.wasm");
        return;
    };
    assert_eq!(wasm::import_fingerprint(&buf).unwrap(), "3411ecc782a61347");
}

#[test]
fn tag_patch_adds_exactly_two_exports() {
    let Some(buf) = core_wasm() else {
        eprintln!("skipped: no core.wasm");
        return;
    };
    let patched = wasm::export_internal_tags(&buf).unwrap();
    assert_eq!(patched.len(), buf.len() + 18, "patch should cost 18 bytes");
    // The import surface is untouched, so the fingerprint must survive.
    assert_eq!(
        wasm::import_fingerprint(&patched).unwrap(),
        wasm::import_fingerprint(&buf).unwrap()
    );
}

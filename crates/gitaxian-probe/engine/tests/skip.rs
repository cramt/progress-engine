/// Announce that a test is not going to check what it claims to check.
///
/// Everything below needs the upstream blobs, and the accuracy cases need
/// `magick` as well. A sandbox with neither makes every one of them return
/// early and the suite go green having verified nothing - the numbers in the
/// README would then survive a change that broke them. Set
/// `PROBE_REQUIRE_ENGINE=1` wherever they are meant to be a claim and a skip
/// becomes a failure instead.
#[track_caller]
pub fn skipped(why: &str) {
    if std::env::var("PROBE_REQUIRE_ENGINE").is_ok_and(|v| !v.is_empty() && v != "0") {
        panic!("PROBE_REQUIRE_ENGINE is set, but {why}");
    }
    eprintln!("skipped: {why}");
}

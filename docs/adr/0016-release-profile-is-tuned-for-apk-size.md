# The workspace release profile is tuned for APK size

`[profile.release]` is `opt-level = "s"`, LTO and strip, because Gitaxian Probe's app ships as an APK and binary size is its budget (`b828fe9`). Cargo has one release profile per workspace, so this also governs `gauntlet`. The Cargo.toml comment says the CLIs "do not care".

**Nobody has measured that.** `gauntlet` is a CPU-bound enumeration engine whose wall times this repo tracks. Before relying on this ADR, compare a timing sweep under `opt-level = 3`. If the gap matters, a named profile for the APK is the obvious way out.

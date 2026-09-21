/* Android's arm64 loader refuses to start an executable whose PT_TLS segment
 * is aligned to less than 64 bytes:
 *
 *   executable's TLS segment is underaligned:
 *   alignment is 8 (skew 0), needs to be at least 64 for ARM64 Bionic
 *
 * Nothing else in probe-host asks for more than 8, so the segment inherits 8
 * and the process aborts before main. This raises it, at a cost of 64 bytes
 * per thread.
 *
 * It is C because Rust's `thread_local!` compiles to pthread keys on Android
 * rather than to `__thread`, so it never reaches the ELF TLS segment at all.
 *
 * Only executables are checked, and only on arm64: the app's own libmain.so
 * has the same 8-byte alignment and loads fine, and x86_64 bionic does not
 * check, which is why an emulator will never reproduce this.
 */
__thread char probe_tls_pad __attribute__((aligned(64)));

/* Referenced from Rust so the archive member is pulled in and not dropped. */
char *probe_tls_align_anchor(void) { return &probe_tls_pad; }

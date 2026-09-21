use dioxus_native::prelude::*;

// Blitz does not resolve env(safe-area-inset-*) on Android yet, so these are
// measured by eye on one phone rather than reported by the OS. Symptom of
// getting them wrong: the header slides under the notch, or the footer sits
// beneath the gesture pill and eats swipes.
// https://github.com/DioxusLabs/blitz/pull/370
const SAFE_TOP_PX: u32 = 48;
const SAFE_BOTTOM_PX: u32 = 32;

pub fn app() -> Element {
    let mut taps = use_signal(|| 0);

    rsx! {
        style { {CSS} }
        div {
            class: "screen",
            style: "padding-top: {SAFE_TOP_PX}px; padding-bottom: {SAFE_BOTTOM_PX}px;",

            header { class: "bar", "safe area top" }

            main { class: "body",
                h1 { "hello from blitz" }
                p { class: "sub", "no webview, painted by skia" }
                button {
                    class: "tap",
                    onclick: move |_| taps += 1,
                    "tapped {taps} times"
                }
            }

            footer { class: "bar", "safe area bottom" }
        }
    }
}

const CSS: &str = r#"
html, body {
    margin: 0;
    padding: 0;
    height: 100%;
}

.screen {
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    height: 100%;
    width: 100%;
    font-family: sans-serif;
    color: #f5f5f5;
    background: linear-gradient(160deg, #1b1f3b 0%, #b00b69 100%);
}

/* Tinted so it is obvious when the padding above is wrong: these bars are the
   first thing a notch or gesture pill will swallow. */
.bar {
    flex: 0 0 auto;
    padding: 6px 14px;
    font-size: 0.75rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: rgba(245, 245, 245, 0.55);
    background: rgba(0, 0, 0, 0.25);
}

.body {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    gap: 14px;
    padding: 24px;
}

h1 {
    margin: 0;
    font-size: 2.25rem;
    line-height: 1.1;
    text-align: center;
}

.sub {
    margin: 0;
    font-size: 1rem;
    color: rgba(245, 245, 245, 0.7);
}

.tap {
    margin-top: 10px;
    padding: 16px 28px;
    font-size: 1.15rem;
    font-family: sans-serif;
    line-height: 1;
    color: #1b1f3b;
    background: #f5f5f5;
    border: none;
    border-radius: 999px;
}

.tap:active {
    background: #cfcfcf;
}
"#;

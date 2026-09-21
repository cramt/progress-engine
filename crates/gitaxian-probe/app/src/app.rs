use dioxus::prelude::*;

// Measured by eye on one phone rather than reported by the OS. A WebView can
// resolve env(safe-area-inset-*) properly, unlike Blitz - but only once the
// generated page carries `viewport-fit=cover`, which dx controls. Until then
// these stay hardcoded. Symptom of getting them wrong: the header slides under
// the notch, or the footer sits beneath the gesture pill and eats swipes.
const SAFE_TOP_PX: u32 = 48;
const SAFE_BOTTOM_PX: u32 = 32;

pub fn app() -> Element {
    let mut taps = use_signal(|| 0);
    let mut engine_status = use_signal(|| "engine not started".to_string());
    let mut camera_status = use_signal(|| "camera off".to_string());

    rsx! {
        style { {CSS} }
        div {
            class: "screen",
            style: "padding-top: {SAFE_TOP_PX}px; padding-bottom: {SAFE_BOTTOM_PX}px;",

            header { class: "bar", "safe area top" }

            main { class: "body",
                h1 { "gitaxian probe" }
                p { class: "sub", "card scanning, one day" }
                button {
                    class: "tap",
                    onclick: move |_| taps += 1,
                    "tapped {taps} times"
                }
                button {
                    class: "tap",
                    onclick: move |_| {
                        engine_status.set("starting...".to_string());
                        // Off the painting thread: this downloads tens of
                        // megabytes on a cold cache, and Android's input
                        // watchdog gives up after five seconds.
                        spawn(async move {
                            let line = tokio::task::spawn_blocking(
                                crate::probe::open_and_describe,
                            )
                            .await
                            .unwrap_or_else(|error| format!("host panicked: {error}"));
                            engine_status.set(line);
                        });
                    },
                    "start engine"
                }
                p { class: "sub", "{engine_status}" }

                // The preview is a plain <video>. This is the half of the app
                // that a WebView gives away: getUserMedia is live preview,
                // permission prompt and frame source in one, where Blitz would
                // have meant NDK Camera2 by hand.
                video {
                    id: "preview",
                    class: "preview",
                    autoplay: true,
                    playsinline: true,
                    muted: true,
                }
                button {
                    class: "tap",
                    onclick: move |_| {
                        camera_status.set("asking...".to_string());
                        spawn(async move {
                            let result = document::eval(CAMERA_JS).await;
                            camera_status.set(match result {
                                Ok(value) => value.as_str().unwrap_or("camera on").to_string(),
                                Err(error) => format!("camera failed: {error:?}"),
                            });
                        });
                    },
                    "start camera"
                }
                p { class: "sub", "{camera_status}" }
            }

            footer { class: "bar", "safe area bottom" }
        }
    }
}

/// Ask for the back camera and show it. Returns a line for the UI either way -
/// a rejected permission and an absent camera both land in the catch.
const CAMERA_JS: &str = r#"
    const video = document.getElementById("preview");
    try {
        const stream = await navigator.mediaDevices.getUserMedia({
            video: { facingMode: { ideal: "environment" } },
            audio: false,
        });
        video.srcObject = stream;
        await video.play();
        const track = stream.getVideoTracks()[0];
        const { width, height } = track.getSettings();
        dioxus.send(`camera on: ${width}x${height}`);
    } catch (error) {
        dioxus.send(`camera failed: ${error.name}: ${error.message}`);
    }
"#;

const CSS: &str = r#"
/* #main is Dioxus's mount node. Leave it out and it keeps its auto height, so
   .screen's height:100% resolves against nothing and the app collapses to
   content height with the rest of the screen left unpainted. */
html, body, #main {
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

/* Sized by the stream once it arrives; until then it is an empty box rather
   than a gap, so it is obvious whether the element exists at all. */
.preview {
    width: 82%;
    max-height: 38vh;
    border-radius: 14px;
    background: rgba(0, 0, 0, 0.35);
    object-fit: cover;
}
"#;

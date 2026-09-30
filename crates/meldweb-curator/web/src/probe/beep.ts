/**
 * A short tone for a card scanned, so the user can keep their eyes on the
 * cards. An AudioContext may only start from a user gesture, so make one when
 * scanning starts and hand it here.
 */
export function beep(audio: AudioContext, frequency = 1320, ms = 90): void {
  const now = audio.currentTime;
  const tone = audio.createOscillator();
  const gain = audio.createGain();
  tone.type = "sine";
  tone.frequency.value = frequency;
  // A fast attack and release, or the edges of the tone click.
  gain.gain.setValueAtTime(0, now);
  gain.gain.linearRampToValueAtTime(0.25, now + 0.005);
  gain.gain.setValueAtTime(0.25, now + ms / 1000 - 0.02);
  gain.gain.linearRampToValueAtTime(0, now + ms / 1000);
  tone.connect(gain).connect(audio.destination);
  tone.start(now);
  tone.stop(now + ms / 1000);
}

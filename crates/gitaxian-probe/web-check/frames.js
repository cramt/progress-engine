// The fixture frames, decoded to RGBA. Decoding is the caller's job, as it is
// any caller's, and a canvas is how a page - or a worker - does it.
const SLUGS = ["lotus", "counterspell", "llanowar", "shock", "swords", "thoughtseize"];

async function rgba(slug) {
  const response = await fetch(new URL(`fixtures/${slug}-frame.jpg`, import.meta.url));
  const bitmap = await createImageBitmap(await response.blob());
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const g = canvas.getContext("2d");
  g.drawImage(bitmap, 0, 0);
  const { data } = g.getImageData(0, 0, bitmap.width, bitmap.height);
  return { rgba: new Uint8Array(data.buffer), width: bitmap.width, height: bitmap.height };
}

export async function frames() {
  return Object.fromEntries(await Promise.all(SLUGS.map(async (s) => [s, await rgba(s)])));
}

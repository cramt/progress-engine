/** Hands `bytes` to the browser as a PDF download named `file`. */
export function downloadPdf(bytes: Uint8Array, file: string) {
  const url = URL.createObjectURL(
    new Blob([bytes as BlobPart], { type: "application/pdf" }),
  );
  const a = document.createElement("a");
  a.href = url;
  a.download = file;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

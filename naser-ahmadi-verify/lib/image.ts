"use client";

// Shrinks a phone photo to a JPEG data URL small enough to upload quickly.
export async function compressImage(file: File, maxSide = 1400, quality = 0.72): Promise<string> {
  const bitmap = await createImageBitmap(file, { imageOrientation: "from-image" });
  const scale = Math.min(1, maxSide / Math.max(bitmap.width, bitmap.height));
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(bitmap.width * scale);
  canvas.height = Math.round(bitmap.height * scale);
  canvas.getContext("2d")!.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  bitmap.close();
  return canvas.toDataURL("image/jpeg", quality);
}

// Reads the PDF417 barcode on the back of a driver's license. Uses the
// original full-resolution file; barcodes need the detail.
export async function readLicenseBarcode(file: File): Promise<string | null> {
  const { prepareZXingModule, readBarcodes } = await import("zxing-wasm/reader");
  prepareZXingModule({
    overrides: {
      locateFile: (path: string, prefix: string) => (path.endsWith(".wasm") ? "/zxing_reader.wasm" : prefix + path),
    },
  });
  const results = await readBarcodes(file, {
    formats: ["PDF417"],
    tryHarder: true,
    maxNumberOfSymbols: 1,
    textMode: "Plain",
  });
  return results.find((r) => r.isValid && r.text)?.text ?? null;
}

// Reads the printed text on a card photo (OCR) in the browser. The OCR engine
// and English language data are downloaded on first use (~3 MB).
export async function readCardText(image: string, onProgress?: (pct: number) => void): Promise<string> {
  const { createWorker } = await import("tesseract.js");
  const worker = await createWorker("eng", 1, {
    logger: (m: { status: string; progress: number }) => {
      if (m.status === "recognizing text") onProgress?.(Math.round(m.progress * 100));
    },
  });
  try {
    const { data } = await worker.recognize(image);
    return data.text;
  } finally {
    await worker.terminate();
  }
}

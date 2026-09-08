export function fileToDataURL(file: File): Promise<string> {
  // Check if FileReader exists (it doesn't on iOS WebView)
  if (typeof FileReader === "undefined") {
    // Use canvas to convert blob to data URL
    return new Promise((resolve, reject) => {
      const blobUrl = URL.createObjectURL(file);
      const img = new Image();

      img.onload = () => {
        try {
          const canvas = document.createElement("canvas");
          const ctx = canvas.getContext("2d");

          if (!ctx) {
            throw new Error("Failed to get canvas context");
          }

          // Set canvas size to image size
          canvas.width = img.naturalWidth;
          canvas.height = img.naturalHeight;

          // Draw image to canvas
          ctx.drawImage(img, 0, 0);

          // Convert to data URL
          const dataUrl = canvas.toDataURL(file.type || "image/png");

          // Clean up
          URL.revokeObjectURL(blobUrl);

          resolve(dataUrl);
        } catch (error) {
          URL.revokeObjectURL(blobUrl);
          reject(error);
        }
      };

      img.onerror = () => {
        URL.revokeObjectURL(blobUrl);
        reject(new Error("Failed to load image"));
      };

      img.src = blobUrl;
    });
  }

  // Standard FileReader approach
  return new Promise((res, rej) => {
    const reader = new FileReader();
    reader.onload = () => {
      if (typeof reader.result === "string") {
        res(reader.result);
      } else {
        rej(new Error("Unexpected FileReader result type"));
      }
    };
    reader.onerror = () => rej(reader.error);
    reader.onabort = () => rej(new Error("FileReader operation was aborted"));
    reader.readAsDataURL(file);
  });
}

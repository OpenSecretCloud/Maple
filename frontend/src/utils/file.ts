export function fileToDataURL(file: File): Promise<string> {
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

interface StorageMock {
  [key: string]: string;
}

function storageMock(): Storage {
  const storage: StorageMock = {};

  return {
    setItem(key: string, value: string) {
      storage[key] = value || "";
    },
    getItem(key: string): string | null {
      return key in storage ? storage[key] : null;
    },
    removeItem(key: string) {
      delete storage[key];
    },
    clear() {
      Object.keys(storage).forEach((key) => delete storage[key]);
    },
    get length(): number {
      return Object.keys(storage).length;
    },
    key(i: number): string | null {
      const keys = Object.keys(storage);
      return keys[i] || null;
    },
    // Required Storage interface properties
    [Symbol.iterator](): IterableIterator<string> {
      return Object.keys(storage)[Symbol.iterator]();
    }
  };
}

global.localStorage = storageMock();
global.sessionStorage = storageMock();
// @ts-expect-error - window is not defined
global.window = global;

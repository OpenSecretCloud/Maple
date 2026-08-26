// Declare .der files as Uint8Array
declare module "*.der" {
  const content: Uint8Array;
  export default content;
}

interface ImportMetaEnv {
  readonly VITE_TEST_NACL_PUBLIC_KEY?: string;
  readonly VITE_TEST_NACL_SECRET_KEY?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

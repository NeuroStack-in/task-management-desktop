/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Set to "1" to bypass the mock core and invoke the real Tauri commands in dev. */
  readonly VITE_REAL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

/// <reference types="vite/client" />

interface ImportMetaEnv {
  // No app-specific vars: the panel reads everything from the core over IPC, and the core reads
  // its own config from the environment (WP_*). Nothing the backend needs belongs in the webview.
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

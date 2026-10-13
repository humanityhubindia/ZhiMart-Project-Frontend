/// <reference types="vite/client" />

interface ImportMetaEnv {
  /**
   * Origin of the backend API (e.g. `https://api.zhimart.com`).
   * Leave empty to call the API on the same origin (dev proxy / reverse proxy).
   */
  readonly VITE_API_BASE_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

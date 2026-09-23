import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import { svelteCssGuard } from "./vite-svelte-css.js";

const host = process.env.TAURI_DEV_HOST;

// Target platform set by the Tauri CLI (linux/windows/darwin/android/ios).
// Only this value reaches the frontend; other TAURI_* env stays in the build process.
const platform = process.env.TAURI_ENV_PLATFORM ?? "unknown";

// Separate ports so desktop and android can run at the same time.
const isAndroid = platform === "android";
const port = isAndroid ? 1430 : 1420;
const hmrPort = isAndroid ? 1431 : host ? 1421 : 1420;
// Separate optimizer caches. A shared node_modules/.vite lets Android dev
// poison desktop CSS (raw .svelte files get served as stylesheets).
const cacheDir = isAndroid ? "node_modules/.vite-android" : "node_modules/.vite-desktop";

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [svelteCssGuard(), sveltekit()],
  cacheDir,
  define: {
    __TAURI_PLATFORM__: JSON.stringify(platform),
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port,
    strictPort: true,
    // Bind IPv4 loopback explicitly: on dual-stack hosts `false`/localhost binds
    // only IPv6 (::1), and the WebKitGTK dev webview's HMR websocket resolves
    // localhost to 127.0.0.1 — so hot reload never connects and edits don't show.
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: hmrPort,
        }
      : { protocol: "ws", host: "127.0.0.1", port },
    watch: {
      // 3. tell Vite to ignore watching `src-tauri` and local toolchain dirs
      ignored: ["**/src-tauri/**", "**/.dev-prefix/**", "**/.toolchains/**", "**/.pnpm-store/**"],
    },
  },
}));

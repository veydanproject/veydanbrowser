import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";

const host = process.env.TAURI_DEV_HOST;

// Target platform set by the Tauri CLI (linux/windows/darwin/android/ios).
// Only this value reaches the frontend; other TAURI_* env stays in the build process.
const platform = process.env.TAURI_ENV_PLATFORM ?? "unknown";

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [sveltekit()],
  define: {
    __TAURI_PLATFORM__: JSON.stringify(platform),
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    // Bind IPv4 loopback explicitly: on dual-stack hosts `false`/localhost binds
    // only IPv6 (::1), and the WebKitGTK dev webview's HMR websocket resolves
    // localhost to 127.0.0.1 — so hot reload never connects and edits don't show.
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : { protocol: "ws", host: "127.0.0.1", port: 1420 },
    watch: {
      // 3. tell Vite to ignore watching `src-tauri` and local toolchain dirs
      ignored: ["**/src-tauri/**", "**/.dev-prefix/**", "**/.toolchains/**", "**/.pnpm-store/**"],
    },
  },
}));

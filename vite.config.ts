import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // The dev server watches the whole repo, and on Windows its watcher opens a
    // handle on every folder created while it runs that Windows then refuses to
    // move. Without this, a project made in `projects/` during a dev session
    // cannot be deleted until the dev server restarts.
    watch: { ignored: ["**/projects/**"] },
  },
  build: { target: "es2021", sourcemap: true },
});

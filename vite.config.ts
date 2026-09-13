import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // The dev server watches the whole repo unless told otherwise.
    //
    // `projects/`: on Windows the watcher opens a handle on every folder created
    // while it runs that Windows then refuses to move, so a project made there
    // during a dev session could not be deleted until the server restarted.
    //
    // `src-tauri/`: its `target/` holds tens of thousands of build files, and
    // crawling them at startup kept the server from answering the window's
    // first request for seconds (measured: 3.6 s to serve `index.html`, with
    // every module behind it slow too). Nothing in there is served anyway.
    watch: { ignored: ["**/projects/**", "**/src-tauri/**"] },
    // `tauri dev` opens the window the moment this port answers, and without
    // this the window's first request paid for transforming every module from
    // cold. Transforming them as the server comes up takes that off the wait.
    warmup: { clientFiles: ["./src/main.ts"] },
  },
  build: { target: "es2021", sourcemap: true },
});

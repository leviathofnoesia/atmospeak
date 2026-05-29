import { defineConfig } from "vite";

export default defineConfig({
  root: "website",
  publicDir: "assets",
  build: {
    outDir: "../dist-site",
    emptyOutDir: true,
  },
  server: {
    port: 1430,
    strictPort: true,
    host: "127.0.0.1",
  },
  preview: {
    port: 1431,
    strictPort: true,
    host: "127.0.0.1",
  },
});

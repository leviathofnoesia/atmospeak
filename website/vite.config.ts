import { defineConfig } from "vite";

export default defineConfig({
  root: "website",
  base: process.env.GITHUB_ACTIONS ? "/atmospeak/" : "/",
  publicDir: "../src/assets/nov-pax",
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

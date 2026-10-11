import type { NextConfig } from "next";

const isProd = process.env.NODE_ENV === "production";
const internalHost = process.env.TAURI_DEV_HOST || "localhost";

const nextConfig: NextConfig = {
  // Tauri empaqueta archivos estáticos: la app no lleva servidor de Next.js.
  output: "export",
  images: { unoptimized: true },
  assetPrefix: isProd ? undefined : `http://${internalHost}:3000`,

  // Conecta Tailwind CSS v4 con Turbopack. Sin este bloque las clases de
  // Tailwind (bg-..., flex, grid...) no generan ningún estilo.
  turbopack: {
    rules: {
      "*.css": {
        loaders: ["@tailwindcss/turbopack"],
        as: "*.css",
      },
    },
  },
};

export default nextConfig;

import type { Metadata } from "next";
import { Nunito, Pixelify_Sans } from "next/font/google";
import { config } from "@/lib/config";
import "./globals.css";

// Next.js descarga estas fuentes al compilar y las empaqueta dentro del
// launcher, así que el programa final no depende de internet para mostrarlas.
const nunito = Nunito({
  variable: "--font-nunito",
  subsets: ["latin"],
});

const pixelify = Pixelify_Sans({
  variable: "--font-pixelify",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: config.launcherName,
  description: `Launcher de ${config.serverName}`,
};

export default function RootLayout({ children }: LayoutProps<"/">) {
  return (
    <html lang="es" className={`${nunito.variable} ${pixelify.variable} h-full antialiased`}>
      <body className="h-full">{children}</body>
    </html>
  );
}

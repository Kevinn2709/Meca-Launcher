import { config } from "@/lib/config";

/**
 * Logo del launcher: la imagen `public/logo.webp`, con fondo transparente.
 * Para cambiarlo basta con sustituir ese archivo. Si la imagen nueva tiene
 * otra proporción, ajusta WIDTH y HEIGHT para que no se deforme.
 *
 * Para hacerlo más grande o más pequeño, cambia WIDTH y pon en HEIGHT
 * el ancho dividido entre 2.5 (la proporción de la imagen).
 */
const WIDTH = 370;
const HEIGHT = 148;

export function Logo({ className }: { className?: string }) {
  return (
    // eslint-disable-next-line @next/next/no-img-element
    <img
      src="/logo.webp"
      alt={config.launcherName}
      width={WIDTH}
      height={HEIGHT}
      draggable={false}
      className={`block drop-shadow-[0_10px_14px_rgb(8_18_48/0.45)] ${className ?? ""}`}
    />
  );
}

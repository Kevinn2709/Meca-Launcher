import type { CSSProperties } from "react";

type SceneProps = {
  /** Detiene la animación, por ejemplo mientras Minecraft está abierto. */
  paused?: boolean;
};

/**
 * Destellos que flotan cerca del sol. Las posiciones son fijas (no al azar)
 * para que la pantalla se dibuje igual cada vez que se abre el launcher.
 * left/top en % de la ventana, size en px, duration/delay en segundos.
 */
const MOTES = [
  { left: 58, top: 62, size: 4, duration: 15, delay: -2, drift: -26, peak: 0.75 },
  { left: 63, top: 48, size: 3, duration: 19, delay: -11, drift: 18, peak: 0.6 },
  { left: 67, top: 72, size: 5, duration: 17, delay: -6, drift: -14, peak: 0.8 },
  { left: 71, top: 55, size: 3, duration: 21, delay: -15, drift: 22, peak: 0.55 },
  { left: 74, top: 38, size: 4, duration: 18, delay: -4, drift: -18, peak: 0.7 },
  { left: 77, top: 66, size: 6, duration: 16, delay: -9, drift: 12, peak: 0.85 },
  { left: 80, top: 50, size: 3, duration: 22, delay: -18, drift: -24, peak: 0.6 },
  { left: 82, top: 78, size: 4, duration: 15, delay: -1, drift: 16, peak: 0.75 },
  { left: 85, top: 42, size: 5, duration: 20, delay: -13, drift: -10, peak: 0.8 },
  { left: 87, top: 60, size: 3, duration: 17, delay: -7, drift: 20, peak: 0.65 },
  { left: 89, top: 30, size: 4, duration: 23, delay: -20, drift: -16, peak: 0.6 },
  { left: 91, top: 70, size: 6, duration: 18, delay: -3, drift: -22, peak: 0.85 },
  { left: 93, top: 52, size: 4, duration: 16, delay: -10, drift: 14, peak: 0.8 },
  { left: 95, top: 36, size: 3, duration: 21, delay: -16, drift: -12, peak: 0.65 },
  { left: 96, top: 64, size: 5, duration: 19, delay: -8, drift: -20, peak: 0.8 },
  { left: 69, top: 34, size: 3, duration: 24, delay: -22, drift: 10, peak: 0.5 },
];

/**
 * Fondo del launcher. Usa `public/bg.webp`; para cambiar el paisaje basta
 * con sustituir ese archivo.
 *
 * Tiene tres movimientos lentos: el paisaje se acerca y se aleja muy
 * despacio, el resplandor del sol respira y unos destellos suben flotando.
 * Todo se detiene si el sistema pide reducir animaciones.
 */
export function Scene({ paused = false }: SceneProps) {
  return (
    <div aria-hidden className="scene" data-paused={paused ? "" : undefined}>
      <div
        className="scene__art"
        style={{
          backgroundImage:
            "url(/bg.webp), linear-gradient(180deg, #3c86dc 0%, #9cc8f0 52%, #5f9b3c 52%, #2f6a2a 100%)",
        }}
      />
      <div className="scene__sun" />
      <div className="scene__motes">
        {MOTES.map((mote, index) => (
          <span
            key={index}
            className="scene__mote"
            style={
              {
                left: `${mote.left}%`,
                top: `${mote.top}%`,
                "--size": `${mote.size}px`,
                "--drift": `${mote.drift}px`,
                "--peak": mote.peak,
                animationDuration: `${mote.duration}s`,
                animationDelay: `${mote.delay}s`,
              } as CSSProperties
            }
          />
        ))}
      </div>
      {/* Oscurece un poco arriba y abajo para que el texto se lea sobre el paisaje. */}
      <div className="scene__scrim" />
    </div>
  );
}

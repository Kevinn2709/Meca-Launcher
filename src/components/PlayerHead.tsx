"use client";

import { useEffect, useRef, useState } from "react";
import { DefaultFace } from "./PixelArt";

type PlayerHeadProps = {
  /** Textura de la skin (64×64 o 64×32). Sin ella se muestra una cara genérica. */
  skinUrl: string | null;
  className?: string;
};

/**
 * Cabeza del jugador recortada de su skin: la cara (8×8 en la posición 8,8)
 * con la capa del gorro encima (8×8 en la posición 40,8).
 */
export function PlayerHead({ skinUrl, className }: PlayerHeadProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [drawnUrl, setDrawnUrl] = useState<string | null>(null);

  useEffect(() => {
    if (!skinUrl) return;

    let active = true;
    const image = new Image();
    image.onload = () => {
      const context = canvasRef.current?.getContext("2d");
      if (!active || !context) return;
      context.imageSmoothingEnabled = false;
      context.clearRect(0, 0, 8, 8);
      context.drawImage(image, 8, 8, 8, 8, 0, 0, 8, 8);
      context.drawImage(image, 40, 8, 8, 8, 0, 0, 8, 8);
      setDrawnUrl(skinUrl);
    };
    image.src = skinUrl;

    return () => {
      active = false;
    };
  }, [skinUrl]);

  const ready = skinUrl !== null && drawnUrl === skinUrl;

  return (
    <span className={`relative block overflow-hidden ${className ?? ""}`}>
      {!ready && <DefaultFace className="absolute inset-0 h-full w-full" />}
      <canvas
        ref={canvasRef}
        width={8}
        height={8}
        aria-hidden
        className={`pixelated absolute inset-0 h-full w-full ${ready ? "" : "invisible"}`}
      />
    </span>
  );
}

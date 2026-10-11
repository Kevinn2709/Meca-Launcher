type PixelArtProps = {
  /** Cada cadena es una fila; cada carácter, un píxel. El punto es transparente. */
  rows: readonly string[];
  /** Color de cada carácter. */
  palette: Readonly<Record<string, string>>;
  className?: string;
  /** Texto para lectores de pantalla. Si se omite, el dibujo es decorativo. */
  label?: string;
};

/** Dibuja pixel art nítido a cualquier tamaño a partir de filas de texto. */
export function PixelArt({ rows, palette, className, label }: PixelArtProps) {
  const width = Math.max(...rows.map((row) => row.length));
  const height = rows.length;

  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      className={className}
      shapeRendering="crispEdges"
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      {rows.flatMap((row, y) =>
        Array.from(row).map((char, x) => {
          const fill = palette[char];
          if (!fill) return null;
          return (
            <rect key={`${x}-${y}`} x={x} y={y} width={1} height={1} fill={fill} />
          );
        }),
      )}
    </svg>
  );
}

const FACE_PALETTE = {
  Y: "#ffcc2f",
  L: "#fff48f",
  D: "#e9720c",
  K: "#43190a",
} as const;

/** Cara de 8×8 con los colores del logo. Es el avatar mientras no hay skin. */
const FACE_ROWS = [
  "LLLLLLLL",
  "YYYYYYYY",
  "YKKYYKKY",
  "YKKYYKKY",
  "YYYYYYYY",
  "YYKKKKYY",
  "YYYYYYYY",
  "DDDDDDDD",
] as const;

export function DefaultFace({ className }: { className?: string }) {
  return <PixelArt rows={FACE_ROWS} palette={FACE_PALETTE} className={className} />;
}

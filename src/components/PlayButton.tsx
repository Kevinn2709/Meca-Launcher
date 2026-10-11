import type { CSSProperties, ReactNode } from "react";
import type { Phase } from "@/hooks/useLauncher";

type PlayButtonProps = {
  phase: Phase;
  /** Avance de 0 a 1. Solo se dibuja al descargar. */
  progress: number;
  /** Línea de apoyo bajo el botón. */
  hint: ReactNode;
  onPress: () => void;
};

/**
 * Aspecto del bloque:
 * - honey: bloque de miel lleno, listo para pulsar.
 * - progress: tarro oscuro que se llena de miel con el avance.
 * - idle: bloque apagado mientras no hay nada que pulsar.
 * - danger: algo falló y se puede reintentar.
 */
type Variant = "honey" | "progress" | "idle" | "danger";

const VIEW: Record<Phase, { label: string; variant: Variant; pressable: boolean }> = {
  booting: { label: "CARGANDO…", variant: "idle", pressable: false },
  "signed-out": { label: "INICIAR SESIÓN", variant: "honey", pressable: true },
  "signing-in": { label: "INICIANDO SESIÓN…", variant: "idle", pressable: false },
  ready: { label: "JUGAR", variant: "honey", pressable: true },
  checking: { label: "COMPROBANDO…", variant: "honey", pressable: false },
  downloading: { label: "DESCARGANDO…", variant: "progress", pressable: false },
  installing: { label: "INSTALANDO…", variant: "honey", pressable: false },
  launching: { label: "INICIANDO…", variant: "honey", pressable: false },
  running: { label: "EN JUEGO", variant: "idle", pressable: false },
  error: { label: "REINTENTAR", variant: "danger", pressable: true },
};

export function PlayButton({ phase, progress, hint, onPress }: PlayButtonProps) {
  const { label, variant, pressable } = VIEW[phase];
  const showsProgress = variant === "progress";
  const fill = showsProgress ? Math.min(1, Math.max(0, progress)) : variant === "idle" ? 0 : 1;
  const percent = Math.floor(fill * 100);

  const content = (
    <span className="honey-btn__label">
      <span>{label}</span>
      {showsProgress && <span className="honey-btn__percent">{percent}%</span>}
    </span>
  );

  return (
    <div className="flex w-full flex-col items-stretch gap-3">
      {/* La sombra va en este contenedor: el botón recorta todo lo que sale de su silueta. */}
      <div className="drop-shadow-[0_10px_16px_rgb(6_20_4/0.5)]">
        <button
          type="button"
          className="honey-btn"
          data-variant={variant}
          data-busy={
            phase === "checking" || phase === "installing" || phase === "launching" || showsProgress
              ? ""
              : undefined
          }
          disabled={!pressable}
          aria-busy={!pressable && phase !== "running"}
          onClick={onPress}
          style={{ "--fill": fill } as CSSProperties}
        >
          <span className="honey-btn__face">
            {content}
            <span className="honey-btn__fill" aria-hidden>
              {content}
            </span>
            {showsProgress && <span className="honey-btn__edge" aria-hidden />}
          </span>
        </button>
      </div>

      {showsProgress && (
        <div
          role="progressbar"
          aria-label={label}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent}
          className="sr-only"
        />
      )}

      <div
        aria-live={showsProgress ? "off" : "polite"}
        className="min-h-10 text-center text-sm leading-5 font-bold text-cream text-shadow-scene"
      >
        {hint}
      </div>
    </div>
  );
}

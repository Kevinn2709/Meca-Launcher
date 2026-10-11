import type { ReactNode } from "react";
import { config } from "@/lib/config";
import { formatCount } from "@/lib/format";
import type { LauncherInfo, ModpackStatus, ServerStatus } from "@/lib/types";

type StatusCornerProps = {
  server: ServerStatus | null;
  modpack: ModpackStatus | null;
  /** `true` si no se pudo consultar el modpack (se sigue intentando). */
  modpackUnavailable: boolean;
  info: LauncherInfo | null;
};

/** Datos de estado, pequeños y sin recuadro, en la esquina inferior izquierda. */
export function StatusCorner({ server, modpack, modpackUnavailable, info }: StatusCornerProps) {
  return (
    <dl className="grid w-fit grid-cols-[auto_auto] gap-x-3 gap-y-0.5 text-[13px] leading-5 text-shadow-scene">
      <Row label="Servidor">
        <ServerLine server={server} />
      </Row>
      <Row label="Modpack">
        <ModpackLine modpack={modpack} unavailable={modpackUnavailable} />
      </Row>
      <Row label="Juego">{config.gameVersion}</Row>
      {info && <Row label="Launcher">{info.version}</Row>}
    </dl>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="font-semibold text-cream/65">{label}</dt>
      <dd className="font-bold text-cream/95">{children}</dd>
    </>
  );
}

function ServerLine({ server }: { server: ServerStatus | null }) {
  if (!server) return <span className="text-cream/60">Consultando…</span>;

  if (!server.online) {
    return (
      <span className="flex items-center gap-1.5">
        <Dot className="bg-poppy" />
        Sin respuesta
      </span>
    );
  }

  return (
    <span className="flex items-center gap-1.5">
      <Dot className="bg-grass" />
      {formatCount(server.players)} jugadores en línea
    </span>
  );
}

function ModpackLine({ modpack, unavailable }: { modpack: ModpackStatus | null; unavailable: boolean }) {
  if (!modpack) {
    return <span className="text-cream/60">{unavailable ? "Sin conexión" : "Consultando…"}</span>;
  }
  if (!modpack.gameInstalled || modpack.installed === null) return <>{modpack.latest}, sin instalar</>;
  if (modpack.pendingFiles === 0) return <>{modpack.latest}, al día</>;
  // Misma versión pero faltan o sobran archivos: se arregla al pulsar Jugar.
  if (modpack.installed === modpack.latest) return <>{modpack.latest}, por reparar</>;
  return (
    <>
      {modpack.installed}, hay actualización a {modpack.latest}
    </>
  );
}

function Dot({ className }: { className: string }) {
  return <span aria-hidden className={`inline-block h-2 w-2 shrink-0 ${className}`} />;
}

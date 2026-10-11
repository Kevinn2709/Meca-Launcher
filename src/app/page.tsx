"use client";

import type { ReactNode } from "react";
import { InstallNow } from "@/components/InstallNow";
import { Logo } from "@/components/Logo";
import { PlayButton } from "@/components/PlayButton";
import { Scene } from "@/components/Scene";
import { StatusCorner } from "@/components/StatusCorner";
import { TopBar } from "@/components/TopBar";
import { useLauncher, type LauncherState } from "@/hooks/useLauncher";
import { config } from "@/lib/config";
import { formatBytes, formatCount, formatSpeed } from "@/lib/format";

export default function Home() {
  const { state, signOut, cancelSignIn, prepare, dismissError, primaryAction } = useLauncher();
  const locked =
    state.phase === "checking" ||
    state.phase === "downloading" ||
    state.phase === "installing" ||
    state.phase === "launching" ||
    state.phase === "running";

  // Sin sesión todavía, el juego se puede ir instalando. Con sesión no hace
  // falta el atajo: "Jugar" ya instala y actualiza antes de abrir.
  const waitingForSignIn =
    state.phase === "signed-out" || (state.phase === "error" && state.error?.retry === "signIn");
  const needsInstall =
    state.modpack !== null && (!state.modpack.gameInstalled || state.modpack.pendingFiles > 0);

  return (
    <main className="relative h-full min-h-[640px] min-w-[1120px] overflow-hidden">
      {/* Con Minecraft abierto el fondo deja de moverse para no restarle recursos. */}
      <Scene paused={state.phase === "running"} />

      <div className="relative z-10 grid h-full grid-cols-[1fr_480px_1fr] grid-rows-[auto_minmax(0,1fr)_auto] gap-x-6 px-6 pt-5 pb-5">
        <div className="col-span-3">
          <TopBar session={state.session} demo={state.demo} locked={locked} onSignOut={signOut} />
        </div>

        {/* El logo queda centrado en el cielo, por encima de la montaña. */}
        <div className="col-span-3 row-start-2 mt-1.5 justify-self-center">
          <Logo />
        </div>

        <div className="col-start-1 row-start-3 self-end">
          <StatusCorner
            server={state.server}
            modpack={state.modpack}
            modpackUnavailable={state.modpackUnavailable}
            info={state.info}
          />
        </div>

        <div className="col-start-2 row-start-3">
          <PlayButton
            phase={state.phase}
            progress={state.progress}
            hint={hintFor(state, cancelSignIn, dismissError)}
            onPress={primaryAction}
          />
        </div>

        <div className="col-start-3 row-start-3 self-end justify-self-end">
          {waitingForSignIn && needsInstall && <InstallNow onPress={prepare} />}
        </div>
      </div>
    </main>
  );
}

/** Frase bajo el botón: dice qué va a pasar o qué está pasando. */
function hintFor(
  state: LauncherState,
  onCancelSignIn: () => void,
  onDismissError: () => void,
): ReactNode {
  switch (state.phase) {
    case "booting":
      return "Preparando el launcher…";

    case "signed-out":
      return state.prepared
        ? "Juego instalado. Inicia sesión con tu cuenta de Microsoft para jugar."
        : "Usa la cuenta de Microsoft con la que compraste Minecraft.";

    case "signing-in":
      return (
        <span className="flex items-center justify-center gap-3">
          <span>Termina de iniciar sesión en tu navegador.</span>
          <button
            type="button"
            onClick={onCancelSignIn}
            className="rounded-xs underline underline-offset-4 hover:text-honey-light"
          >
            Cancelar
          </button>
        </span>
      );

    case "ready": {
      const { modpack } = state;
      if (!modpack) {
        return state.modpackUnavailable
          ? "No se pudo consultar el modpack. Se comprobará al pulsar Jugar."
          : "Consultando la versión del modpack…";
      }
      if (!modpack.gameInstalled) {
        return modpack.installed === null
          ? "La primera vez se descargan Minecraft, Forge y los mods: cerca de 1 GB."
          : "Antes de entrar se actualizará el juego.";
      }
      if (modpack.pendingFiles === 0) return "Modpack al día. Entrarás directo al servidor.";
      if (modpack.pendingBytes === 0) return "Antes de entrar se actualizará el modpack.";
      return `Antes de entrar se actualizará el modpack (${formatBytes(modpack.pendingBytes)}).`;
    }

    case "checking": {
      // Solo la cuenta: el total se va conociendo sobre la marcha
      // (Java, luego Minecraft, luego los mods), así que no hay porcentaje.
      const event = state.lastEvent;
      if (event?.kind !== "checking" || event.done === 0) return "Comprobando tus archivos…";
      return `Comprobando tus archivos: ${formatCount(event.done)} revisados`;
    }

    case "downloading": {
      const event = state.lastEvent;
      if (event?.kind !== "downloading") return "Descargando…";
      return (
        <span className="flex items-center justify-between gap-4">
          <span className="truncate">{fileName(event.file) || "Preparando la descarga"}</span>
          <span className="shrink-0 tabular-nums">
            {formatBytes(event.receivedBytes)} de {formatBytes(event.totalBytes)} a{" "}
            {formatSpeed(event.bytesPerSecond)}
          </span>
        </span>
      );
    }

    case "installing":
      return "Instalando Forge. Solo pasa la primera vez y tarda uno o dos minutos.";

    case "launching":
      return `Abriendo Minecraft con ${config.gameVersion}…`;

    case "running":
      return "Minecraft está abierto. Al cerrarlo podrás volver a jugar desde aquí.";

    case "error": {
      const message = state.error?.message ?? "Algo falló. Inténtalo de nuevo.";
      // Un fallo al instalar no debe impedir iniciar sesión: se puede cerrar.
      if (state.error?.retry !== "prepare") return message;
      return (
        <span>
          {message}{" "}
          <button
            type="button"
            onClick={onDismissError}
            className="rounded-xs underline underline-offset-4 hover:text-honey-light"
          >
            Volver
          </button>
        </span>
      );
    }
  }
}

function fileName(path: string): string {
  return path.split("/").pop() ?? path;
}

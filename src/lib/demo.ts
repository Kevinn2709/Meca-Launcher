import type {
  ModpackStatus,
  PlayEvent,
  ServerStatus,
  Session,
} from "./types";

/**
 * Respuestas simuladas para ver y retocar la interfaz en un navegador
 * normal, sin abrir el launcher. Nada de este archivo toca la red ni el
 * disco, y el launcher de verdad no lo usa.
 */

const MB = 1024 * 1024;

const wait = (ms: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));

const DEMO_FILES = [
  "Java 17",
  "Minecraft 1.20.1",
  "Recursos de Minecraft",
  "mods/L_Enders_Cataclysm-3.31.jar",
  "mods/voicechat-forge-1.20.1-2.6.22.jar",
];

let session: Session | null = null;
let upToDate = false;
let signInCancelled = false;

export const demo = {
  async restoreSession(): Promise<Session | null> {
    await wait(350);
    return session;
  },

  async signIn(): Promise<Session> {
    signInCancelled = false;
    await wait(1600);
    if (signInCancelled) {
      throw Object.assign(new Error("Inicio de sesión cancelado."), { code: "cancelled" });
    }
    session = {
      name: "Jugador_Demo",
      uuid: "00000000-0000-0000-0000-000000000000",
      skinUrl: null,
    };
    return session;
  },

  async cancelSignIn(): Promise<void> {
    signInCancelled = true;
  },

  async signOut(): Promise<void> {
    await wait(200);
    session = null;
  },

  async serverStatus(): Promise<ServerStatus> {
    await wait(500);
    return { online: true, players: 128, maxPlayers: 500 };
  },

  async modpackStatus(): Promise<ModpackStatus> {
    await wait(450);
    return upToDate
      ? { installed: "1.4.0", latest: "1.4.0", pendingFiles: 0, pendingBytes: 0, gameInstalled: true }
      : {
          installed: "1.3.2",
          latest: "1.4.0",
          pendingFiles: 38,
          pendingBytes: 212 * MB,
          gameInstalled: true,
        };
  },

  async prepare(onEvent: (event: PlayEvent) => void): Promise<void> {
    await bringUpToDate(onEvent);
  },

  async play(onEvent: (event: PlayEvent) => void): Promise<void> {
    await bringUpToDate(onEvent);
    onEvent({ kind: "launching" });
    await wait(2200);
    onEvent({ kind: "running" });
    await wait(6000);
  },
};

/** Simula comprobar, descargar e instalar. La segunda vez no hay nada que hacer. */
async function bringUpToDate(onEvent: (event: PlayEvent) => void): Promise<void> {
  if (upToDate) return;

  const totalFiles = 124;
  for (let done = 0; done <= totalFiles; done += 4) {
    onEvent({ kind: "checking", done: Math.min(done, totalFiles), total: totalFiles });
    await wait(35);
  }

  const totalBytes = 212 * MB;
  let receivedBytes = 0;
  while (receivedBytes < totalBytes) {
    const bytesPerSecond = (9 + Math.random() * 7) * MB;
    receivedBytes = Math.min(totalBytes, receivedBytes + bytesPerSecond * 0.2);
    const fileIndex = Math.min(
      DEMO_FILES.length - 1,
      Math.floor((receivedBytes / totalBytes) * DEMO_FILES.length),
    );
    onEvent({
      kind: "downloading",
      file: DEMO_FILES[fileIndex],
      receivedBytes,
      totalBytes,
      bytesPerSecond,
    });
    await wait(80);
  }

  onEvent({ kind: "installing" });
  await wait(1800);
  upToDate = true;
}

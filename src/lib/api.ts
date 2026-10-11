import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { demo } from "./demo";
import type {
  LauncherApi,
  LauncherInfo,
  ModpackStatus,
  PlayEvent,
  ServerStatus,
  Session,
} from "./types";

/**
 * Único punto de contacto entre la interfaz y el backend.
 *
 * Dentro del launcher todo es real: lo atiende el núcleo en Rust.
 * En un navegador normal no hay núcleo, así que todo usa `demo.ts`. Eso
 * permite seguir retocando el diseño sin abrir el launcher.
 */

/**
 * Llama a un comando que avisa de su avance. Los avisos viajan por un
 * canal propio y llegan en orden mientras el comando sigue en marcha.
 */
function withProgress(
  command: "prepare" | "play",
  onEvent: (event: PlayEvent) => void,
): Promise<void> {
  const channel = new Channel<PlayEvent>();
  channel.onmessage = onEvent;
  return invoke<void>(command, { onEvent: channel });
}

export const api: LauncherApi = {
  async info() {
    if (!isTauri()) return null;
    return invoke<LauncherInfo>("launcher_info");
  },

  restoreSession() {
    return isTauri() ? invoke<Session | null>("restore_session") : demo.restoreSession();
  },

  signIn() {
    return isTauri() ? invoke<Session>("sign_in") : demo.signIn();
  },

  cancelSignIn() {
    return isTauri() ? invoke<void>("cancel_sign_in") : demo.cancelSignIn();
  },

  signOut() {
    return isTauri() ? invoke<void>("sign_out") : demo.signOut();
  },

  serverStatus() {
    return isTauri() ? invoke<ServerStatus>("server_status") : demo.serverStatus();
  },

  modpackStatus() {
    return isTauri() ? invoke<ModpackStatus>("modpack_status") : demo.modpackStatus();
  },

  prepare(onEvent) {
    return isTauri() ? withProgress("prepare", onEvent) : demo.prepare(onEvent);
  },

  play(onEvent) {
    return isTauri() ? withProgress("play", onEvent) : demo.play(onEvent);
  },
};

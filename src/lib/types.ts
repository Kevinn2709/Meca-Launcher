/** Datos del propio launcher. Vienen del comando Rust `launcher_info`. */
export type LauncherInfo = {
  version: string;
  os: string;
  arch: string;
};

/** Jugador con sesión iniciada. El token nunca llega a la interfaz. */
export type Session = {
  name: string;
  uuid: string;
  /** URL de la textura de la skin. `null` muestra el avatar por defecto. */
  skinUrl: string | null;
};

export type ServerStatus =
  | { online: true; players: number; maxPlayers: number }
  | { online: false };

export type ModpackStatus = {
  /** Versión instalada en este equipo, o `null` si nunca se instaló. */
  installed: string | null;
  /** Versión publicada en el servidor. */
  latest: string;
  /** Archivos que faltan o cambiaron. 0 significa que está al día. */
  pendingFiles: number;
  /** Bytes que hay que descargar para ponerse al día. */
  pendingBytes: number;
  /** `false` hasta que Java, Minecraft y Forge estén instalados en este equipo. */
  gameInstalled: boolean;
};

/**
 * Error que devuelve el backend: `code` identifica el caso
 * ("cancelled", "network", "not_approved", "not_owned"…) y `message`
 * ya viene redactado para mostrarlo al jugador.
 */
export type ApiError = {
  code: string;
  message: string;
};

/** Avisos que emite el backend mientras prepara y abre el juego. */
export type PlayEvent =
  | { kind: "checking"; done: number; total: number }
  | {
      kind: "downloading";
      file: string;
      receivedBytes: number;
      totalBytes: number;
      bytesPerSecond: number;
    }
  /** Forge se está instalando. Solo ocurre la primera vez. */
  | { kind: "installing" }
  | { kind: "launching" }
  | { kind: "running" };

/**
 * Todo lo que la interfaz le pide al backend. Dentro del launcher cada
 * función llama a un comando de Rust; en un navegador normal responde `demo.ts`.
 */
export interface LauncherApi {
  /** Datos del launcher, o `null` si la página se abrió en un navegador normal. */
  info(): Promise<LauncherInfo | null>;
  restoreSession(): Promise<Session | null>;
  /**
   * Abre el navegador para iniciar sesión con Microsoft y espera el resultado.
   * Si falla, rechaza con un `ApiError`.
   */
  signIn(): Promise<Session>;
  /** Abandona un inicio de sesión que sigue esperando al navegador. */
  cancelSignIn(): Promise<void>;
  signOut(): Promise<void>;
  serverStatus(): Promise<ServerStatus>;
  modpackStatus(): Promise<ModpackStatus>;
  /** Instala o actualiza el juego sin abrirlo. No necesita sesión iniciada. */
  prepare(onEvent: (event: PlayEvent) => void): Promise<void>;
  /** Sincroniza, abre el juego y se resuelve cuando el juego se cierra. */
  play(onEvent: (event: PlayEvent) => void): Promise<void>;
}

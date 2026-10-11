"use client";

import { useCallback, useEffect, useReducer, useRef } from "react";
import { api } from "@/lib/api";
import type {
  LauncherInfo,
  ModpackStatus,
  PlayEvent,
  ServerStatus,
  Session,
} from "@/lib/types";

/** Momento en el que está el launcher. Decide qué muestra el botón principal. */
export type Phase =
  | "booting"
  | "signed-out"
  | "signing-in"
  | "ready"
  | "checking"
  | "downloading"
  | "installing"
  | "launching"
  | "running"
  | "error";

/** Qué repite el botón "Reintentar" después de un fallo. */
type Retry = "signIn" | "prepare" | "play";

/** Cada cuánto se vuelve a preguntar al servidor cuántos jugadores hay. */
const SERVER_REFRESH_MS = 30_000;

export type LauncherState = {
  phase: Phase;
  session: Session | null;
  server: ServerStatus | null;
  modpack: ModpackStatus | null;
  /** `true` si la última consulta del modpack falló (sin conexión, por ejemplo). */
  modpackUnavailable: boolean;
  info: LauncherInfo | null;
  /** `true` si la página está en un navegador normal y los datos son de ejemplo. */
  demo: boolean;
  /** `true` si el juego se instaló o actualizó en esta sesión del launcher. */
  prepared: boolean;
  /** Avance de la tarea en curso, de 0 a 1. */
  progress: number;
  /** Último aviso recibido del backend durante "Jugar". */
  lastEvent: PlayEvent | null;
  error: { message: string; retry: Retry } | null;
};

type Action =
  | { type: "info"; info: LauncherInfo | null }
  | { type: "server"; server: ServerStatus }
  | { type: "modpack"; modpack: ModpackStatus }
  | { type: "modpack-unavailable" }
  | { type: "session-restored"; session: Session | null }
  | { type: "sign-in-started" }
  | { type: "signed-in"; session: Session }
  | { type: "signed-out" }
  | { type: "play-started" }
  | { type: "play-event"; event: PlayEvent }
  | { type: "prepared" }
  | { type: "game-closed" }
  | { type: "failed"; message: string; retry: Retry }
  | { type: "error-dismissed" };

const initialState: LauncherState = {
  phase: "booting",
  session: null,
  server: null,
  modpack: null,
  modpackUnavailable: false,
  info: null,
  demo: false,
  prepared: false,
  progress: 0,
  lastEvent: null,
  error: null,
};

function ratio(done: number, total: number): number {
  if (total <= 0) return 0;
  return Math.min(1, Math.max(0, done / total));
}

/** Fases en las que hay una tarea en marcha que va enviando avisos. */
const WORKING: readonly Phase[] = ["checking", "downloading", "installing", "launching", "running"];

function reducer(state: LauncherState, action: Action): LauncherState {
  switch (action.type) {
    case "info":
      // Sin datos del launcher es que la página se abrió en un navegador normal.
      return { ...state, info: action.info, demo: action.info === null };
    case "server":
      return { ...state, server: action.server };
    case "modpack":
      return { ...state, modpack: action.modpack, modpackUnavailable: false };
    case "modpack-unavailable":
      return { ...state, modpackUnavailable: true };

    case "session-restored":
      if (state.phase !== "booting") return state;
      return {
        ...state,
        session: action.session,
        phase: action.session ? "ready" : "signed-out",
      };
    case "sign-in-started":
      return { ...state, phase: "signing-in", error: null };
    case "signed-in":
      return { ...state, phase: "ready", session: action.session, error: null };
    case "signed-out":
      return {
        ...state,
        phase: "signed-out",
        session: null,
        error: null,
        progress: 0,
        lastEvent: null,
      };

    case "play-started":
      return { ...state, phase: "checking", progress: 0, lastEvent: null, error: null };
    case "play-event": {
      // Los avisos y el resultado final viajan por caminos distintos: un
      // aviso que llegue cuando la tarea ya terminó no debe cambiar nada.
      if (!WORKING.includes(state.phase)) return state;
      const { event } = action;
      switch (event.kind) {
        case "checking":
          return {
            ...state,
            phase: "checking",
            progress: ratio(event.done, event.total),
            lastEvent: event,
          };
        case "downloading":
          return {
            ...state,
            phase: "downloading",
            progress: ratio(event.receivedBytes, event.totalBytes),
            lastEvent: event,
          };
        case "installing":
          return { ...state, phase: "installing", progress: 1, lastEvent: event };
        case "launching":
          return { ...state, phase: "launching", progress: 1, lastEvent: event };
        case "running":
          return { ...state, phase: "running", progress: 0, lastEvent: event };
      }
      return state;
    }
    case "prepared":
      return {
        ...state,
        phase: state.session ? "ready" : "signed-out",
        prepared: true,
        progress: 0,
        lastEvent: null,
      };
    case "game-closed":
      return { ...state, phase: "ready", progress: 0, lastEvent: null };

    case "failed":
      return {
        ...state,
        phase: "error",
        progress: 0,
        lastEvent: null,
        error: { message: action.message, retry: action.retry },
      };
    case "error-dismissed":
      if (state.phase !== "error") return state;
      return { ...state, phase: state.session ? "ready" : "signed-out", error: null };
  }
}

/** Código del error si viene del backend (ver `ApiError`), o `null`. */
function errorCode(error: unknown): string | null {
  if (typeof error === "object" && error !== null && "code" in error) {
    return typeof error.code === "string" ? error.code : null;
  }
  return null;
}

/** Texto para mostrar al jugador: el del backend si existe, o uno genérico. */
function describe(error: unknown, fallback: string): string {
  if (typeof error === "string" && error.trim()) return error;
  if (typeof error === "object" && error !== null && "message" in error) {
    const { message } = error;
    if (typeof message === "string" && message.trim()) return message;
  }
  return fallback;
}

export function useLauncher() {
  const [state, dispatch] = useReducer(reducer, initialState);

  // Carga inicial. Cada dato llega por separado para que un fallo
  // (por ejemplo, sin conexión) no deje en blanco el resto de la pantalla.
  useEffect(() => {
    let active = true;
    const whenActive =
      <T,>(handler: (value: T) => void) =>
      (value: T) => {
        if (active) handler(value);
      };

    api
      .info()
      .then(whenActive((info) => dispatch({ type: "info", info })))
      .catch(() => {});
    api
      .restoreSession()
      .then(whenActive((session) => dispatch({ type: "session-restored", session })))
      .catch(whenActive(() => dispatch({ type: "session-restored", session: null })));
    api
      .serverStatus()
      .then(whenActive((server) => dispatch({ type: "server", server })))
      .catch(whenActive(() => dispatch({ type: "server", server: { online: false } })));
    return () => {
      active = false;
    };
  }, []);

  // `true` cuando ya se sabe el estado del modpack. Mientras no se sepa
  // (sin conexión al abrir, por ejemplo) se vuelve a preguntar cada poco.
  const modpackKnown = useRef(false);

  const refreshModpack = useCallback(() => {
    api
      .modpackStatus()
      .then((modpack) => {
        modpackKnown.current = true;
        dispatch({ type: "modpack", modpack });
      })
      .catch(() => {
        modpackKnown.current = false;
        dispatch({ type: "modpack-unavailable" });
      });
  }, []);

  useEffect(() => {
    refreshModpack();
  }, [refreshModpack]);

  // El número de jugadores cambia: se vuelve a preguntar cada cierto tiempo.
  useEffect(() => {
    const timer = setInterval(() => {
      api
        .serverStatus()
        .then((server) => dispatch({ type: "server", server }))
        .catch(() => dispatch({ type: "server", server: { online: false } }));
      if (!modpackKnown.current) refreshModpack();
    }, SERVER_REFRESH_MS);
    return () => clearInterval(timer);
  }, [refreshModpack]);

  const signIn = useCallback(async () => {
    dispatch({ type: "sign-in-started" });
    try {
      const session = await api.signIn();
      dispatch({ type: "signed-in", session });
    } catch (error) {
      // Cancelar no es un fallo: se vuelve al botón de iniciar sesión.
      if (errorCode(error) === "cancelled") {
        dispatch({ type: "signed-out" });
        return;
      }
      dispatch({
        type: "failed",
        retry: "signIn",
        message: describe(
          error,
          "No se pudo iniciar sesión. Revisa tu conexión e inténtalo de nuevo.",
        ),
      });
    }
  }, []);

  const cancelSignIn = useCallback(() => {
    // El resultado llega por `signIn`, que termina con el código "cancelled".
    api.cancelSignIn().catch(() => {});
  }, []);

  const signOut = useCallback(async () => {
    try {
      await api.signOut();
    } finally {
      dispatch({ type: "signed-out" });
    }
  }, []);

  /** Instala o actualiza el juego sin abrirlo. Sirve aunque no haya sesión. */
  const prepare = useCallback(async () => {
    dispatch({ type: "play-started" });
    try {
      await api.prepare((event) => dispatch({ type: "play-event", event }));
      dispatch({ type: "prepared" });
    } catch (error) {
      dispatch({
        type: "failed",
        retry: "prepare",
        message: describe(
          error,
          "No se pudo instalar el juego. Revisa tu conexión e inténtalo de nuevo.",
        ),
      });
    }
    refreshModpack();
  }, [refreshModpack]);

  const play = useCallback(async () => {
    dispatch({ type: "play-started" });
    try {
      await api.play((event) => dispatch({ type: "play-event", event }));
      dispatch({ type: "game-closed" });
    } catch (error) {
      const message = describe(
        error,
        "No se pudo preparar el juego. Revisa tu conexión e inténtalo de nuevo.",
      );
      if (errorCode(error) === "expired") {
        // La sesión ya no sirve: hay que volver a entrar con Microsoft.
        dispatch({ type: "signed-out" });
        dispatch({ type: "failed", retry: "signIn", message });
      } else {
        dispatch({ type: "failed", retry: "play", message });
      }
    }
    refreshModpack();
  }, [refreshModpack]);

  /** Cierra el aviso de error y vuelve al botón normal. */
  const dismissError = useCallback(() => dispatch({ type: "error-dismissed" }), []);

  /** Lo que hace el botón principal según el momento. */
  const primaryAction = useCallback(() => {
    switch (state.phase) {
      case "signed-out":
        return signIn();
      case "ready":
        return play();
      case "error":
        switch (state.error?.retry) {
          case "signIn":
            return signIn();
          case "prepare":
            return prepare();
          default:
            return play();
        }
      default:
        return undefined;
    }
  }, [state.phase, state.error, signIn, prepare, play]);

  return { state, signOut, cancelSignIn, prepare, dismissError, primaryAction };
}

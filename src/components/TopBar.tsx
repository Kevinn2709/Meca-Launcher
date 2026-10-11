"use client";

import { useEffect, useRef, useState } from "react";
import type { Session } from "@/lib/types";
import { PlayerHead } from "./PlayerHead";

type TopBarProps = {
  session: Session | null;
  /** Muestra el aviso de que los datos son de ejemplo (solo en un navegador normal). */
  demo: boolean;
  /** Mientras se descarga o se juega no se puede cerrar sesión. */
  locked: boolean;
  onSignOut: () => void;
};

export function TopBar({ session, demo, locked, onSignOut }: TopBarProps) {
  return (
    <header className="relative z-20 flex h-[52px] items-start justify-between">
      <div className="flex flex-col items-start gap-2">
        {/* Aviso que piden las normas de uso de Minecraft a todo producto no oficial. */}
        <p className="max-w-[330px] text-[11px] leading-4 font-semibold text-cream/60 text-shadow-scene">
          No es un producto oficial de Minecraft. No está aprobado por Mojang o Microsoft ni
          asociado con ellos.
        </p>
        {demo && (
          <span
            className="rounded-sm bg-glass px-2 py-1 text-xs font-bold text-cream/85 backdrop-blur-md"
            title="Estás viendo el launcher en un navegador: los datos son de ejemplo."
          >
            Datos de ejemplo
          </span>
        )}
      </div>

      {session && <AccountMenu session={session} locked={locked} onSignOut={onSignOut} />}
    </header>
  );
}

function AccountMenu({
  session,
  locked,
  onSignOut,
}: {
  session: Session;
  locked: boolean;
  onSignOut: () => void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;

    const onPointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };

    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        aria-haspopup="menu"
        aria-expanded={open}
        className="glass flex items-center gap-3 py-1.5 pr-4 pl-1.5 transition-colors hover:bg-glass-strong"
      >
        <PlayerHead skinUrl={session.skinUrl} className="h-10 w-10 rounded-sm" />
        <span className="max-w-44 truncate text-[15px] font-extrabold text-cream">
          {session.name}
        </span>
        <svg
          viewBox="0 0 10 6"
          aria-hidden
          className={`h-1.5 w-2.5 text-cream/70 transition-transform ${open ? "rotate-180" : ""}`}
        >
          <path d="M1 1l4 4 4-4" fill="none" stroke="currentColor" strokeWidth="1.6" />
        </svg>
      </button>

      {open && (
        <div role="menu" className="glass absolute right-0 z-20 mt-2 w-56 bg-glass-solid p-1.5">
          <button
            type="button"
            role="menuitem"
            disabled={locked}
            onClick={() => {
              setOpen(false);
              onSignOut();
            }}
            className="w-full rounded-sm px-3 py-2 text-left text-sm font-bold text-cream transition-colors hover:bg-white/10 disabled:cursor-not-allowed disabled:text-cream/45 disabled:hover:bg-transparent"
          >
            Cerrar sesión
          </button>
          {locked && (
            <p className="px-3 pt-1 pb-2 text-xs leading-snug text-cream/70">
              Disponible cuando termine la descarga o cierres el juego.
            </p>
          )}
        </div>
      )}
    </div>
  );
}

type InstallNowProps = {
  onPress: () => void;
};

/**
 * Atajo discreto en la esquina inferior derecha: instala o actualiza el
 * juego sin abrirlo. Sirve para ir descargando antes de iniciar sesión.
 */
export function InstallNow({ onPress }: InstallNowProps) {
  return (
    <div className="flex flex-col items-end gap-1.5 text-[13px] leading-5 text-shadow-scene">
      <span className="font-semibold text-cream/65">No hace falta iniciar sesión</span>
      <button
        type="button"
        onClick={onPress}
        className="glass px-3 py-1.5 font-bold text-cream/95 transition-colors hover:bg-glass-strong"
      >
        Instalar el juego ahora
      </button>
    </div>
  );
}

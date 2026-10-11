//! Avisos de progreso hacia la interfaz.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;

/// Lo que la interfaz recibe mientras se prepara y se abre el juego.
/// Coincide con `PlayEvent` en types.ts.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PlayEvent {
    Checking {
        done: u64,
        total: u64,
    },
    #[serde(rename_all = "camelCase")]
    Downloading {
        file: String,
        received_bytes: u64,
        total_bytes: u64,
        bytes_per_second: u64,
    },
    /// Forge se está instalando (solo la primera vez).
    Installing,
    Launching,
    Running,
}

/// Destino de los avisos. Fuera de las pruebas es el canal hacia la interfaz.
pub struct Events {
    sink: Box<dyn Fn(PlayEvent) + Send + Sync>,
}

impl Events {
    pub fn new(sink: impl Fn(PlayEvent) + Send + Sync + 'static) -> Self {
        Self {
            sink: Box::new(sink),
        }
    }

    pub fn send(&self, event: PlayEvent) {
        (self.sink)(event);
    }
}

/// Como mucho un aviso cada tanto: la interfaz no necesita más y así no se satura.
const INTERVAL: Duration = Duration::from_millis(100);
/// Cada cuánto se recalcula la velocidad de descarga.
const SPEED_WINDOW: Duration = Duration::from_millis(500);

/// Cuenta los archivos comprobados.
pub struct CheckProgress {
    events: Arc<Events>,
    done: AtomicU64,
    total: AtomicU64,
    emitted_at: Mutex<Instant>,
}

impl CheckProgress {
    pub fn new(events: Arc<Events>) -> Self {
        let progress = Self {
            events,
            done: AtomicU64::new(0),
            total: AtomicU64::new(0),
            emitted_at: Mutex::new(Instant::now()),
        };
        progress.emit(true);
        progress
    }

    /// Anuncia que hay `files` archivos más por comprobar.
    pub fn expect(&self, files: u64) {
        self.total.fetch_add(files, Ordering::Relaxed);
    }

    /// Un archivo comprobado.
    pub fn tick(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
        self.emit(false);
    }

    pub fn finish(&self) {
        self.emit(true);
    }

    fn emit(&self, force: bool) {
        let mut emitted_at = self.emitted_at.lock().unwrap_or_else(PoisonError::into_inner);
        if !force && emitted_at.elapsed() < INTERVAL {
            return;
        }
        *emitted_at = Instant::now();
        let total = self.total.load(Ordering::Relaxed);
        self.events.send(PlayEvent::Checking {
            done: self.done.load(Ordering::Relaxed).min(total),
            total,
        });
    }
}

struct Meter {
    emitted_at: Instant,
    sampled_at: Instant,
    sampled_bytes: u64,
    bytes_per_second: f64,
}

/// Cuenta los bytes descargados entre todas las descargas simultáneas.
pub struct DownloadProgress {
    events: Arc<Events>,
    total: AtomicU64,
    received: AtomicU64,
    meter: Mutex<Meter>,
}

impl DownloadProgress {
    pub fn new(events: Arc<Events>, total: u64) -> Self {
        let now = Instant::now();
        let progress = Self {
            events,
            total: AtomicU64::new(total),
            received: AtomicU64::new(0),
            meter: Mutex::new(Meter {
                emitted_at: now,
                sampled_at: now,
                sampled_bytes: 0,
                bytes_per_second: 0.0,
            }),
        };
        progress.emit("", true);
        progress
    }

    /// Llegaron `bytes` del archivo `label`.
    pub fn add(&self, label: &str, bytes: u64) {
        self.received.fetch_add(bytes, Ordering::Relaxed);
        self.emit(label, false);
    }

    /// Suma al total un archivo cuyo tamaño no se conocía hasta empezar a bajarlo.
    pub fn expect(&self, bytes: u64) {
        self.total.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Descuenta lo recibido en un intento que falló y se va a repetir.
    pub fn undo(&self, bytes: u64) {
        self.received.fetch_sub(bytes, Ordering::Relaxed);
    }

    pub fn finish(&self) {
        self.emit("", true);
    }

    fn emit(&self, label: &str, force: bool) {
        let mut meter = self.meter.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        if !force && now.duration_since(meter.emitted_at) < INTERVAL {
            return;
        }
        meter.emitted_at = now;

        let received = self.received.load(Ordering::Relaxed);
        let window = now.duration_since(meter.sampled_at);
        if window >= SPEED_WINDOW {
            let recent = received.saturating_sub(meter.sampled_bytes) as f64 / window.as_secs_f64();
            // Media suavizada: el número no salta con cada archivo pequeño.
            meter.bytes_per_second = if meter.bytes_per_second == 0.0 {
                recent
            } else {
                meter.bytes_per_second * 0.6 + recent * 0.4
            };
            meter.sampled_at = now;
            meter.sampled_bytes = received;
        }

        self.events.send(PlayEvent::Downloading {
            file: label.to_owned(),
            received_bytes: received,
            // Si algún tamaño no se conocía de antemano, el total nunca queda por debajo.
            total_bytes: self.total.load(Ordering::Relaxed).max(received),
            bytes_per_second: meter.bytes_per_second as u64,
        });
    }
}

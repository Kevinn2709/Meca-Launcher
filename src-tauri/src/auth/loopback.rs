//! Servidor local de un solo uso. Microsoft termina el inicio de sesión
//! enviando al navegador a `http://localhost:<puerto>/?code=...`, y este
//! módulo es quien recibe esa visita.
//!
//! Solo escucha en la propia máquina (127.0.0.1 y ::1), nunca en la red.

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// Parámetros con los que Microsoft devolvió al navegador.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Callback {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WaitError {
    Cancelled,
    TimedOut,
}

pub struct Loopback {
    listeners: Vec<TcpListener>,
    port: u16,
}

impl Loopback {
    /// Abre un puerto libre elegido por el sistema.
    pub fn bind() -> std::io::Result<Self> {
        let v4 = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = v4.local_addr()?.port();
        v4.set_nonblocking(true)?;
        let mut listeners = vec![v4];

        // "localhost" puede resolverse a IPv6 en algunos equipos, así que se
        // escucha también ahí cuando el mismo puerto está libre.
        if let Ok(v6) = TcpListener::bind((Ipv6Addr::LOCALHOST, port)) {
            if v6.set_nonblocking(true).is_ok() {
                listeners.push(v6);
            }
        }

        Ok(Self { listeners, port })
    }

    /// Dirección de regreso. Azure tiene registrado `http://localhost`
    /// y, para localhost, acepta cualquier puerto.
    pub fn redirect_uri(&self) -> String {
        format!("http://localhost:{}", self.port)
    }

    /// Espera la visita del navegador. Termina antes si `cancel` pasa a
    /// `true` o si se agota `timeout`. Bloquea el hilo que la llama.
    pub fn wait_for_callback(
        &self,
        cancel: &AtomicBool,
        timeout: Duration,
    ) -> Result<Callback, WaitError> {
        let deadline = Instant::now() + timeout;

        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(WaitError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(WaitError::TimedOut);
            }

            let mut had_connection = false;
            for listener in &self.listeners {
                match listener.accept() {
                    Ok((stream, _)) => {
                        had_connection = true;
                        if let Some(callback) = handle_connection(stream) {
                            return Ok(callback);
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                    Err(_) => {}
                }
            }

            if !had_connection {
                thread::sleep(Duration::from_millis(80));
            }
        }
    }
}

/// Atiende una conexión. Devuelve `Some` solo si era el regreso de Microsoft;
/// cualquier otra petición (el icono de la pestaña, por ejemplo) se ignora.
fn handle_connection(mut stream: TcpStream) -> Option<Callback> {
    // En Windows la conexión hereda el modo "sin bloqueo" del puerto.
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    stream.set_write_timeout(Some(Duration::from_secs(3))).ok()?;

    let head = read_request_head(&mut stream)?;
    let request_line = head.lines().next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;

    if method != "GET" {
        respond(&mut stream, "405 Method Not Allowed", &page(false, "Petición no válida."));
        return None;
    }

    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path, query),
        None => (target, ""),
    };
    if path != "/" {
        respond(&mut stream, "404 Not Found", "");
        return None;
    }

    let callback = parse_callback(query);
    if callback.code.is_none() && callback.error.is_none() {
        respond(
            &mut stream,
            "400 Bad Request",
            &page(false, "Esta página solo sirve para terminar el inicio de sesión."),
        );
        return None;
    }

    let body = if callback.code.is_some() {
        page(true, "Ya puedes cerrar esta pestaña y volver al launcher.")
    } else {
        page(false, "No se completó el inicio de sesión. Vuelve al launcher para intentarlo de nuevo.")
    };
    respond(&mut stream, "200 OK", &body);
    Some(callback)
}

/// Lee la cabecera de la petición (hasta la línea en blanco).
fn read_request_head(stream: &mut TcpStream) -> Option<String> {
    const LIMIT: usize = 16 * 1024;
    let mut data = Vec::new();
    let mut chunk = [0u8; 1024];

    while data.len() < LIMIT {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                data.extend_from_slice(&chunk[..read]);
                if data.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    if data.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&data).into_owned())
}

fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// Página mínima que ve el jugador en el navegador. No incluye ningún dato
/// de la petición, así que no hay nada que un tercero pueda inyectar.
fn page(success: bool, message: &str) -> String {
    let title = if success { "Sesión iniciada" } else { "Inicio de sesión" };
    let accent = if success { "#ffc22e" } else { "#ef4b3f" };
    format!(
        "<!doctype html><html lang=\"es\"><head><meta charset=\"utf-8\"><title>{title} - Meca Launcher</title>\
<style>body{{margin:0;min-height:100vh;display:grid;place-items:center;background:#0e1a2e;color:#f7f9f2;\
font-family:Segoe UI,system-ui,sans-serif}}main{{max-width:28rem;padding:2rem;text-align:center}}\
h1{{margin:0 0 .75rem;font-size:1.75rem;color:{accent}}}p{{margin:0;font-size:1.05rem;line-height:1.5}}</style>\
</head><body><main><h1>{title}</h1><p>{message}</p></main></body></html>"
    )
}

/// Convierte `code=abc&state=xyz` en un `Callback`.
pub fn parse_callback(query: &str) -> Callback {
    let mut callback = Callback::default();
    for pair in query.split('&') {
        let (key, value) = match pair.split_once('=') {
            Some((key, value)) => (key, value),
            None => (pair, ""),
        };
        let value = decode_component(value);
        match decode_component(key).as_str() {
            "code" => callback.code = Some(value),
            "state" => callback.state = Some(value),
            "error" => callback.error = Some(value),
            "error_description" => callback.error_description = Some(value),
            _ => {}
        }
    }
    callback
}

/// Deshace la codificación de un valor de URL: `%2F` es `/` y `+` es un espacio.
fn decode_component(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'+' => output.push(b' '),
            b'%' if index + 2 < bytes.len() => {
                match (hex_value(bytes[index + 1]), hex_value(bytes[index + 2])) {
                    (Some(high), Some(low)) => {
                        output.push(high * 16 + low);
                        index += 2;
                    }
                    _ => output.push(b'%'),
                }
            }
            other => output.push(other),
        }
        index += 1;
    }

    String::from_utf8_lossy(&output).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::Arc;

    fn request(port: u16, target: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "GET {target} HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn parses_and_decodes_the_query() {
        let callback = parse_callback("code=M.C5_BAY.2%2Fabc%2B1&state=x-y_z&extra=1");
        assert_eq!(callback.code.as_deref(), Some("M.C5_BAY.2/abc+1"));
        assert_eq!(callback.state.as_deref(), Some("x-y_z"));
        assert_eq!(callback.error, None);

        let denied = parse_callback("error=access_denied&error_description=The+user+denied%20access%2e&state=s");
        assert_eq!(denied.error.as_deref(), Some("access_denied"));
        assert_eq!(denied.error_description.as_deref(), Some("The user denied access."));

        // Entradas rotas no deben hacer fallar nada.
        assert_eq!(decode_component("100%"), "100%");
        assert_eq!(decode_component("%zz%4"), "%zz%4");
        assert_eq!(decode_component("%C3%B1"), "ñ");
        assert_eq!(parse_callback(""), Callback::default());
    }

    #[test]
    fn receives_the_browser_redirect_and_ignores_other_requests() {
        let loopback = Loopback::bind().unwrap();
        let port = loopback.port;
        assert_eq!(loopback.redirect_uri(), format!("http://localhost:{port}"));

        let cancel = Arc::new(AtomicBool::new(false));
        let waiter = {
            let cancel = cancel.clone();
            thread::spawn(move || loopback.wait_for_callback(&cancel, Duration::from_secs(10)))
        };

        // Una conexión que no envía nada y la petición del icono no cuentan.
        drop(TcpStream::connect(("127.0.0.1", port)).unwrap());
        assert!(request(port, "/favicon.ico").starts_with("HTTP/1.1 404"));
        assert!(request(port, "/?foo=bar").starts_with("HTTP/1.1 400"));

        let response = request(port, "/?code=abc%20123&state=xyz");
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("Sesión iniciada"));

        let callback = waiter.join().unwrap().unwrap();
        assert_eq!(callback.code.as_deref(), Some("abc 123"));
        assert_eq!(callback.state.as_deref(), Some("xyz"));
    }

    #[test]
    fn stops_when_cancelled_or_out_of_time() {
        let loopback = Loopback::bind().unwrap();
        let cancel = AtomicBool::new(false);
        assert_eq!(
            loopback.wait_for_callback(&cancel, Duration::from_millis(200)),
            Err(WaitError::TimedOut)
        );
        cancel.store(true, Ordering::SeqCst);
        assert_eq!(
            loopback.wait_for_callback(&cancel, Duration::from_secs(10)),
            Err(WaitError::Cancelled)
        );
    }
}

//! Pregunta al servidor de Minecraft si está encendido y cuántos jugadores
//! hay, con el mismo saludo que usa el juego en la lista de servidores.

use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::Value;

use super::config::{LAN_CONNECT_TIMEOUT, PING_REPLY_TIMEOUT, PUBLIC_CONNECT_TIMEOUT};
use super::manifest::ServerSpec;

/// Protocolo de Minecraft 1.20.1. Para consultar el estado vale cualquiera.
const PROTOCOL: i32 = 763;
/// Las respuestas de servidores con mods son grandes, pero no tanto.
const MAX_RESPONSE: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pong {
    pub players: u64,
    pub max_players: u64,
}

fn write_varint(out: &mut Vec<u8>, value: i32) {
    let mut value = value as u32;
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn read_varint(stream: &mut impl Read) -> io::Result<i32> {
    let mut value: u32 = 0;
    for position in 0..5 {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte)?;
        value |= u32::from(byte[0] & 0x7f) << (7 * position);
        if byte[0] & 0x80 == 0 {
            return Ok(value as i32);
        }
    }
    Err(io::ErrorKind::InvalidData.into())
}

fn handshake(host: &str, port: u16) -> Vec<u8> {
    let mut body = Vec::new();
    write_varint(&mut body, 0x00); // paquete: saludo
    write_varint(&mut body, PROTOCOL);
    write_varint(&mut body, host.len() as i32);
    body.extend_from_slice(host.as_bytes());
    body.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut body, 1); // siguiente paso: estado

    let mut packet = Vec::new();
    write_varint(&mut packet, body.len() as i32);
    packet.extend_from_slice(&body);
    packet.extend_from_slice(&[0x01, 0x00]); // paquete: petición de estado
    packet
}

fn read_status(stream: &mut impl Read) -> io::Result<Pong> {
    let invalid = || io::Error::from(io::ErrorKind::InvalidData);

    let _packet_length = read_varint(stream)?;
    if read_varint(stream)? != 0x00 {
        return Err(invalid());
    }
    let length = usize::try_from(read_varint(stream)?).map_err(|_| invalid())?;
    if length > MAX_RESPONSE {
        return Err(invalid());
    }
    let mut data = vec![0u8; length];
    stream.read_exact(&mut data)?;

    let status: Value = serde_json::from_slice(&data).map_err(|_| invalid())?;
    let players = status.get("players").ok_or_else(invalid)?;
    let count = |key: &str| players.get(key).and_then(Value::as_u64).unwrap_or(0);
    Ok(Pong {
        players: count("online"),
        max_players: count("max"),
    })
}

/// Consulta un servidor. `None` si no responde a tiempo o no es de Minecraft.
/// `connect` limita la espera para conectar; una vez dentro, el servidor
/// tiene `PING_REPLY_TIMEOUT` para contestar.
pub fn ping(host: &str, port: u16, connect: Duration) -> Option<Pong> {
    // Un nombre puede tener varias direcciones (IPv4 e IPv6): se prueban
    // las primeras, con las IPv4 por delante.
    let mut addresses: Vec<_> = (host, port).to_socket_addrs().ok()?.collect();
    addresses.sort_by_key(|address| !address.is_ipv4());
    addresses.into_iter().take(3).find_map(|address| {
        let mut stream = TcpStream::connect_timeout(&address, connect).ok()?;
        stream.set_read_timeout(Some(PING_REPLY_TIMEOUT)).ok()?;
        stream.set_write_timeout(Some(PING_REPLY_TIMEOUT)).ok()?;
        stream.write_all(&handshake(host, port)).ok()?;
        read_status(&mut stream).ok()
    })
}

/// Busca el servidor: primero por la red de casa (más rápido, y la única
/// forma de entrar desde la misma casa con muchos routers) y luego por internet.
/// Devuelve la dirección que respondió.
pub fn locate(server: &ServerSpec) -> Option<(String, Pong)> {
    if let Some(lan) = &server.lan_address {
        if let Some(pong) = ping(lan, server.port, LAN_CONNECT_TIMEOUT) {
            return Some((lan.clone(), pong));
        }
    }
    ping(&server.address, server.port, PUBLIC_CONNECT_TIMEOUT).map(|pong| (server.address.clone(), pong))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn varints_round_trip() {
        for (value, bytes) in [
            (0, vec![0x00]),
            (127, vec![0x7f]),
            (128, vec![0x80, 0x01]),
            (763, vec![0xfb, 0x05]),
            (25565, vec![0xdd, 0xc7, 0x01]),
            (-1, vec![0xff, 0xff, 0xff, 0xff, 0x0f]),
        ] {
            let mut out = Vec::new();
            write_varint(&mut out, value);
            assert_eq!(out, bytes);
            assert_eq!(read_varint(&mut bytes.as_slice()).unwrap(), value);
        }
    }

    /// Servidor de mentira que contesta como uno de Minecraft.
    fn fake_server(json: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let length = read_varint(&mut stream).unwrap() as usize;
            let mut hello = vec![0u8; length + 2];
            stream.read_exact(&mut hello).unwrap();

            let mut body = vec![0x00];
            write_varint(&mut body, json.len() as i32);
            body.extend_from_slice(json.as_bytes());
            let mut packet = Vec::new();
            write_varint(&mut packet, body.len() as i32);
            packet.extend_from_slice(&body);
            stream.write_all(&packet).unwrap();
        });
        port
    }

    #[test]
    fn reads_the_player_count() {
        let port = fake_server(
            r#"{"version":{"name":"1.20.1","protocol":763},"players":{"max":20,"online":3},"description":{"text":"Meca"},"forgeData":{"fmlNetworkVersion":3}}"#,
        );
        assert_eq!(
            ping("127.0.0.1", port, Duration::from_secs(2)),
            Some(Pong { players: 3, max_players: 20 })
        );
    }

    #[test]
    fn a_closed_port_or_a_stranger_is_offline() {
        let closed = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        assert_eq!(ping("127.0.0.1", closed, Duration::from_millis(500)), None);
        let stranger = fake_server("esto no es un servidor de Minecraft");
        assert_eq!(ping("127.0.0.1", stranger, Duration::from_secs(2)), None);
    }

    #[test]
    fn the_home_network_is_tried_first() {
        let port = fake_server(r#"{"players":{"max":10,"online":1}}"#);
        let server = ServerSpec {
            address: "192.0.2.1".into(), // dirección reservada: nunca responde
            port,
            lan_address: Some("127.0.0.1".into()),
        };
        assert_eq!(locate(&server).map(|(host, _)| host).as_deref(), Some("127.0.0.1"));
    }
}

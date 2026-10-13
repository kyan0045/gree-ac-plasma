//! Minimal client for the Gree local network protocol.
//!
//! This follows the same on-the-wire behaviour as the community
//! [`greeclimate`](https://github.com/cmroche/greeclimate) implementation:
//! discover by UDP broadcast, negotiate a per-session key with a `bind`
//! handshake, then read (`status`) and write (`cmd`) properties.

use aes::{
    Aes128,
    cipher::{Block, BlockEncrypt, KeyInit},
};
use aes_gcm::{Aes128Gcm, Nonce, aead::Aead};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ecb::cipher::{BlockDecryptMut, BlockEncryptMut, block_padding::Pkcs7};
use if_addrs::{IfAddr, get_if_addrs};
use serde_json::Value;
use std::{collections::HashMap, net::Ipv4Addr, time::Duration};
use tokio::{net::UdpSocket, time::timeout_at};

use crate::model::AcState;

pub const GREE_PORT: u16 = 7000;
const GREE_V1_KEY: &[u8; 16] = b"a3K8Bx%2r8Y7#xDh";
const GREE_V2_KEY: &[u8; 16] = b"{yxAHAY_Lm6pbC/<";
const GREE_V2_NONCE: &[u8; 12] = b"T@xDIgZQl^c\x13";
const GREE_V2_AAD: &[u8] = b"qualcomm-test";

/// Properties requested when reading state.
const STATUS_COLUMNS: [&str; 9] = [
    "Pow", "Mod", "SetTem", "TemSen", "TemUn", "TemRec", "WdSpd", "SwUpDn", "Lig",
];

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// A unit located on the local network.
#[derive(Clone, Debug)]
pub struct Device {
    pub ip: Ipv4Addr,
    pub mac: String,
    pub name: String,
    pub model: String,
    pub version: String,
}

/// A bound connection to one unit, holding the negotiated session key.
pub struct GreeClient {
    pub device: Device,
    socket: UdpSocket,
    key: String,
    use_v2: bool,
}

impl GreeClient {
    /// Bind to a previously discovered unit, ready to read and write properties.
    pub async fn connect(device: Device) -> Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
        socket.set_broadcast(true)?;

        // A device only answers a bind request immediately after it has seen a
        // scan, so prime it first.
        send_scan(&socket).await;

        let mut bind_errors = Vec::new();
        for use_v2 in [false, true] {
            match bind_once(&socket, &device, use_v2).await {
                Ok(key) => {
                    return Ok(GreeClient {
                        device,
                        socket,
                        key,
                        use_v2,
                    });
                }
                Err(error) => bind_errors.push(error.to_string()),
            }
            // Re-prime between attempts.
            if !use_v2 {
                send_scan(&socket).await;
            }
        }

        if bind_errors.is_empty() {
            return Err("the unit did not complete the local bind handshake".into());
        }
        Err(format!("bind failed for {}: {}", device.ip, bind_errors.join("; ")).into())
    }

    /// Request the unit's current state.
    pub async fn read_state(&self) -> Result<AcState> {
        let pack = serde_json::json!({
            "t": "status",
            "mac": self.device.mac,
            "cols": STATUS_COLUMNS,
        });
        self.send_packet(pack).await?;
        let properties = self.receive_state().await?;
        Ok(AcState::from_properties(&properties))
    }

    /// Send a `cmd` packet carrying the given property updates.
    pub async fn send_command(&self, props: &[(String, i64)], beep: bool) -> Result<()> {
        let mut opt = Vec::new();
        let mut values = Vec::new();
        for (name, value) in props {
            opt.push(Value::String(name.clone()));
            values.push(Value::from(*value));
        }
        // Match the reference client's buzzer handling so some firmwares accept
        // the command without beeping.
        if !beep {
            opt.push(Value::String("Buzzer_ON_OFF".to_owned()));
            values.push(Value::from(1));
        }
        opt.push(Value::String("BuzzerCtrl".to_owned()));
        values.push(Value::from(if beep { 1 } else { 0 }));

        let pack = serde_json::json!({
            "t": "cmd",
            "mac": self.device.mac,
            "opt": opt,
            "p": values,
        });
        self.send_packet(pack).await
    }

    async fn send_packet(&self, pack: Value) -> Result<()> {
        let key: &[u8; 16] = self
            .key
            .as_bytes()
            .try_into()
            .map_err(|_| "unexpected session key length")?;
        let pack_text = serde_json::to_vec(&pack)?;

        let mut packet = serde_json::json!({
            "cid": "app",
            "i": 0,
            "t": "pack",
            "uid": 0,
            "tcid": self.device.mac,
        });

        if self.use_v2 {
            let cipher = Aes128Gcm::new_from_slice(key)
                .map_err(|error| format!("invalid GCM key: {error:?}"))?;
            let encrypted = cipher
                .encrypt(
                    Nonce::from_slice(GREE_V2_NONCE),
                    aes_gcm::aead::Payload {
                        msg: &pack_text,
                        aad: GREE_V2_AAD,
                    },
                )
                .map_err(|_| "GCM encryption failed")?;
            let split = encrypted
                .len()
                .checked_sub(16)
                .ok_or("invalid GCM output")?;
            packet["pack"] = Value::String(STANDARD.encode(&encrypted[..split]));
            packet["tag"] = Value::String(STANDARD.encode(&encrypted[split..]));
        } else {
            let encrypted = ecb::Encryptor::<Aes128>::new(key.into())
                .encrypt_padded_vec_mut::<Pkcs7>(&pack_text);
            packet["pack"] = Value::String(STANDARD.encode(encrypted));
        }

        self.socket
            .send_to(&serde_json::to_vec(&packet)?, (self.device.ip, GREE_PORT))
            .await?;
        Ok(())
    }

    async fn receive_state(&self) -> Result<Value> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        let mut buffer = [0_u8; 8192];
        loop {
            let (length, source) = timeout_at(deadline, self.socket.recv_from(&mut buffer))
                .await
                .map_err(|error| {
                    format!(
                        "timed out waiting for state reply from {}: {error}",
                        self.device.ip
                    )
                })??;
            if source.ip() != std::net::IpAddr::V4(self.device.ip) {
                continue;
            }
            let Ok(packet) = serde_json::from_slice::<Value>(&buffer[..length]) else {
                continue;
            };
            let Some(encrypted) = packet.get("pack").and_then(Value::as_str) else {
                continue;
            };
            let tag = packet.get("tag").and_then(Value::as_str);
            let key: &[u8; 16] = self
                .key
                .as_bytes()
                .try_into()
                .map_err(|_| "unexpected session key length")?;
            let Some(plaintext) = decrypt_payload(encrypted, tag, key, self.use_v2) else {
                continue;
            };
            let Ok(reply) = serde_json::from_slice::<Value>(&plaintext) else {
                continue;
            };
            if reply.get("t").and_then(Value::as_str) == Some("dat") {
                return properties_from_reply(&reply);
            }
        }
    }
}

/// Discover units on all broadcast-capable IPv4 interfaces.
pub async fn discover(wait: Duration) -> Result<Vec<Device>> {
    let broadcasts = broadcast_addresses()?;
    if broadcasts.is_empty() {
        return Err("no active IPv4 broadcast interfaces found".into());
    }

    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    socket.set_broadcast(true)?;

    // Broadcast replies are lossy, so scan in two rounds and keep everything
    // found in either round.
    let mut devices = HashMap::<String, Device>::new();
    let mut buffer = [0_u8; 8192];
    let rounds = 2u32;
    let round_wait = wait / rounds;
    for round in 0..rounds {
        for address in &broadcasts {
            let _ = socket
                .send_to(br#"{"t":"scan"}"#, (*address, GREE_PORT))
                .await;
        }
        let deadline = tokio::time::Instant::now() + round_wait;
        loop {
            match timeout_at(deadline, socket.recv_from(&mut buffer)).await {
                Ok(Ok((length, source))) => {
                    if let Some(device) = decode_discovery_reply(&buffer[..length], source.ip()) {
                        devices.insert(device.mac.clone(), device);
                    }
                }
                Ok(Err(_)) => continue,
                Err(_) => break,
            }
        }
        if round + 1 < rounds {
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }

    let mut devices: Vec<_> = devices.into_values().collect();
    devices.sort_by(|a, b| a.mac.cmp(&b.mac));
    Ok(devices)
}

async fn send_scan(socket: &UdpSocket) {
    if let Ok(addresses) = broadcast_addresses() {
        for address in addresses {
            let _ = socket
                .send_to(br#"{"t":"scan"}"#, (address, GREE_PORT))
                .await;
        }
    }
    // Give the unit a moment to register the scan before we bind.
    tokio::time::sleep(Duration::from_millis(600)).await;
}

async fn bind_once(socket: &UdpSocket, device: &Device, use_v2: bool) -> Result<String> {
    let pack = serde_json::json!({ "t": "bind", "mac": device.mac, "uid": 0 });
    let key = if use_v2 {
        GREE_V2_KEY.as_slice()
    } else {
        GREE_V1_KEY.as_slice()
    };
    let key: &[u8; 16] = key.try_into().expect("static key length");

    let pack_text = serde_json::to_vec(&pack)?;
    let mut packet = serde_json::json!({
        "cid": "app",
        "i": 1,
        "t": "pack",
        "uid": 0,
        "tcid": device.mac,
    });
    if use_v2 {
        let cipher = Aes128Gcm::new_from_slice(key)
            .map_err(|error| format!("invalid GCM key: {error:?}"))?;
        let encrypted = cipher
            .encrypt(
                Nonce::from_slice(GREE_V2_NONCE),
                aes_gcm::aead::Payload {
                    msg: &pack_text,
                    aad: GREE_V2_AAD,
                },
            )
            .map_err(|_| "GCM encryption failed")?;
        let split = encrypted
            .len()
            .checked_sub(16)
            .ok_or("invalid GCM output")?;
        packet["pack"] = Value::String(STANDARD.encode(&encrypted[..split]));
        packet["tag"] = Value::String(STANDARD.encode(&encrypted[split..]));
    } else {
        let encrypted =
            ecb::Encryptor::<Aes128>::new(key.into()).encrypt_padded_vec_mut::<Pkcs7>(&pack_text);
        packet["pack"] = Value::String(STANDARD.encode(encrypted));
    }

    socket
        .send_to(&serde_json::to_vec(&packet)?, (device.ip, GREE_PORT))
        .await?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let mut buffer = [0_u8; 8192];
    loop {
        let (length, source) = timeout_at(deadline, socket.recv_from(&mut buffer))
            .await
            .map_err(|error| {
                format!(
                    "timed out waiting for bind reply from {}: {error}",
                    device.ip
                )
            })??;
        if source.ip() != std::net::IpAddr::V4(device.ip) {
            continue;
        }
        let Ok(reply_packet) = serde_json::from_slice::<Value>(&buffer[..length]) else {
            continue;
        };
        let Some(encrypted) = reply_packet.get("pack").and_then(Value::as_str) else {
            continue;
        };
        let tag = reply_packet.get("tag").and_then(Value::as_str);
        let Some(plaintext) = decrypt_payload(encrypted, tag, key, use_v2) else {
            continue;
        };
        let Ok(reply) = serde_json::from_slice::<Value>(&plaintext) else {
            continue;
        };
        if reply.get("t").and_then(Value::as_str) == Some("bindok")
            && let Some(key) = reply.get("key").and_then(Value::as_str)
        {
            return Ok(key.to_owned());
        }
    }
}

fn properties_from_reply(reply: &Value) -> Result<Value> {
    let columns = reply
        .get("cols")
        .and_then(Value::as_array)
        .ok_or("status reply has no columns")?;
    let values = reply
        .get("dat")
        .and_then(Value::as_array)
        .ok_or("status reply has no values")?;
    let properties = columns
        .iter()
        .zip(values)
        .filter_map(|(name, value)| Some((name.as_str()?.to_owned(), value.clone())))
        .collect::<serde_json::Map<_, _>>();
    Ok(Value::Object(properties))
}

fn broadcast_addresses() -> Result<Vec<Ipv4Addr>> {
    let mut addresses = Vec::new();
    for interface in get_if_addrs()? {
        if let IfAddr::V4(address) = interface.addr
            && !address.ip.is_loopback()
            && let Some(broadcast) = address.broadcast
            && !addresses.contains(&broadcast)
        {
            addresses.push(broadcast);
        }
    }
    Ok(addresses)
}

fn decode_discovery_reply(bytes: &[u8], source: std::net::IpAddr) -> Option<Device> {
    let packet: Value = serde_json::from_slice(bytes).ok()?;
    let encrypted = packet.get("pack")?.as_str()?;
    let tag = packet.get("tag").and_then(Value::as_str);
    // Discovery replies use the shared cipher; the session key is not known yet.
    let plaintext = decrypt_gree_payload(encrypted, tag)?;
    let details: Value = serde_json::from_slice(&plaintext).ok()?;
    let ip = match source {
        std::net::IpAddr::V4(ip) => ip,
        std::net::IpAddr::V6(_) => return None,
    };

    Some(Device {
        ip,
        mac: field(&details, &["mac", "cid"]).unwrap_or_else(|| ip.to_string()),
        name: field(&details, &["name"]).unwrap_or_default(),
        model: field(&details, &["model"]).unwrap_or_default(),
        version: field(&details, &["ver"]).unwrap_or_default(),
    })
}

fn decrypt_gree_payload(encoded: &str, tag: Option<&str>) -> Option<Vec<u8>> {
    if let Some(plaintext) = decrypt_with_v1_key(encoded, GREE_V1_KEY) {
        return Some(plaintext);
    }
    let ciphertext = STANDARD.decode(encoded).ok()?;
    decrypt_v2(&ciphertext, tag, GREE_V2_KEY)
}

fn decrypt_payload(
    encoded: &str,
    tag: Option<&str>,
    key: &[u8; 16],
    use_v2: bool,
) -> Option<Vec<u8>> {
    if use_v2 {
        let ciphertext = STANDARD.decode(encoded).ok()?;
        decrypt_v2(&ciphertext, tag, key)
    } else {
        decrypt_with_v1_key(encoded, key)
    }
}

fn decrypt_with_v1_key(encoded: &str, key: &[u8; 16]) -> Option<Vec<u8>> {
    let ciphertext = STANDARD.decode(encoded).ok()?;
    let plaintext = ecb::Decryptor::<Aes128>::new(key.into())
        .decrypt_padded_vec_mut::<Pkcs7>(&ciphertext)
        .ok()?;
    serde_json::from_slice::<Value>(&plaintext).ok()?;
    Some(plaintext)
}

fn decrypt_v2(ciphertext: &[u8], tag: Option<&str>, key: &[u8; 16]) -> Option<Vec<u8>> {
    let cipher = Aes128Gcm::new_from_slice(key).ok()?;
    let nonce = Nonce::from_slice(GREE_V2_NONCE);
    if let Some(tag) = tag.and_then(|value| STANDARD.decode(value).ok())
        && let Ok(plaintext) = cipher.decrypt(
            nonce,
            aes_gcm::aead::Payload {
                msg: &[ciphertext, tag.as_slice()].concat(),
                aad: GREE_V2_AAD,
            },
        )
        && serde_json::from_slice::<Value>(&plaintext).is_ok()
    {
        return Some(plaintext);
    }

    // Gree's reference implementation decrypts GCM payload bytes without
    // verifying the authentication tag. Retain that for replies that omit the
    // tag, accepting only output that parses as JSON.
    log::debug!("GCM verification failed, trying unauthenticated CTR fallback");
    let cipher = Aes128::new_from_slice(key).ok()?;
    let mut plaintext = ciphertext.to_vec();
    for (index, block_bytes) in plaintext.chunks_mut(16).enumerate() {
        let counter = (index as u32).checked_add(2)?;
        let mut counter_block = [0_u8; 16];
        counter_block[..12].copy_from_slice(GREE_V2_NONCE);
        counter_block[12..].copy_from_slice(&counter.to_be_bytes());
        let mut keystream = Block::<Aes128>::default();
        keystream.copy_from_slice(&counter_block);
        cipher.encrypt_block(&mut keystream);
        for (value, mask) in block_bytes.iter_mut().zip(keystream.iter()) {
            *value ^= mask;
        }
    }
    serde_json::from_slice::<Value>(&plaintext).ok()?;
    Some(plaintext)
}

fn field(value: &Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        value
            .get(*name)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    })
}

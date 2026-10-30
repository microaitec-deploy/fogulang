//! HTTP client/server, `fogu:web`, WebSocket, Socket.IO, HTML, mail, security (feature `network`).

#[cfg(not(target_family = "wasm"))]
pub mod dns;
#[cfg(not(target_family = "wasm"))]
pub mod http;
#[cfg(not(target_family = "wasm"))]
pub mod http_client;
#[cfg(not(target_family = "wasm"))]
pub mod websocket;
#[cfg(not(target_family = "wasm"))]
pub mod socketio;
#[cfg(not(target_family = "wasm"))]
pub mod web;
#[cfg(not(target_family = "wasm"))]
pub mod topcoat;
pub mod jwt;
pub mod html;
#[cfg(not(target_family = "wasm"))]
pub mod mail;
#[cfg(not(target_family = "wasm"))]
pub mod security;

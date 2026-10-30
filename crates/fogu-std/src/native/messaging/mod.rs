//! NATS, MQTT, Kafka, RabbitMQ, BullMQ and FoguMQ (feature `messaging`).

pub mod nats;
#[cfg(not(target_family = "wasm"))]
pub mod mqtt;
#[cfg(not(target_family = "wasm"))]
pub mod kafka;
#[cfg(not(target_family = "wasm"))]
pub mod rabbitmq;
#[cfg(not(target_family = "wasm"))]
pub mod amqp;
#[cfg(not(target_family = "wasm"))]
pub mod bullmq;
pub mod fogumq;
#[cfg(not(target_family = "wasm"))]
#[path = "foguMQ-client.rs"]
pub mod fogumq_client;
#[cfg(not(target_family = "wasm"))]
pub mod broker;
#[cfg(not(target_family = "wasm"))]
pub mod broker_admin;

mod client;
mod flash_info;
mod hex_dump;
mod parse_utils;
mod terminal_key;
mod variables;
mod version_info;

#[cfg(feature = "tftp")]
mod client_tftp;
#[cfg(feature = "tftp")]
mod tftp_server;

pub type Map<K, V> = indexmap::IndexMap<K, V, fxhash::FxBuildHasher>;

pub type Result<T> = anyhow::Result<T>;

pub use client::UBootClient;

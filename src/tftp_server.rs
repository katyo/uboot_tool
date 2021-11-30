use async_tftp::packet;
use async_tftp::server::Handler;
use std::{
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
};
use tokio_util::compat::Compat;

pub struct TftpHandler {
    base_path: PathBuf,
    auth_ip: Option<IpAddr>,
    allow_read: bool,
    allow_write: bool,
}

impl TftpHandler {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            base_path: path.as_ref().to_owned(),
            auth_ip: None,
            allow_read: false,
            allow_write: false,
        }
    }

    pub fn auth_ip(mut self, ip: IpAddr) -> Self {
        self.auth_ip = Some(ip);
        self
    }

    pub fn allow_read(mut self, allow: bool) -> Self {
        self.allow_read = allow;
        self
    }

    pub fn allow_write(mut self, allow: bool) -> Self {
        self.allow_write = allow;
        self
    }
}

#[async_tftp::async_trait]
impl Handler for TftpHandler {
    type Reader = Compat<tokio::fs::File>;
    type Writer = Compat<tokio::fs::File>;

    async fn read_req_open(
        &mut self,
        client: &SocketAddr,
        path: &Path,
    ) -> Result<(Self::Reader, Option<u64>), packet::Error> {
        use tokio_util::compat::TokioAsyncReadCompatExt;

        if let Some(ip) = self.auth_ip {
            if client.ip() != ip {
                return Err(packet::Error::PermissionDenied);
            }
        }

        if !self.allow_read {
            return Err(packet::Error::PermissionDenied);
        }

        let path = self.base_path.join(path);

        match tokio::fs::File::open(path).await {
            Ok(file) => Ok((file.compat(), None)),
            Err(_) => Err(packet::Error::FileNotFound),
        }
    }

    async fn write_req_open(
        &mut self,
        client: &SocketAddr,
        path: &Path,
        _size: Option<u64>,
    ) -> Result<Self::Writer, packet::Error> {
        use tokio_util::compat::TokioAsyncWriteCompatExt;

        if let Some(ip) = self.auth_ip {
            if client.ip() != ip {
                return Err(packet::Error::PermissionDenied);
            }
        }

        if !self.allow_write {
            return Err(packet::Error::PermissionDenied);
        }

        let path = self.base_path.join(path);

        match tokio::fs::File::create(path).await {
            Ok(file) => Ok(file.compat_write()),
            Err(_) => Err(packet::Error::FileNotFound),
        }
    }
}

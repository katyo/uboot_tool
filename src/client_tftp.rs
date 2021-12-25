use std::{net::IpAddr, path::Path};

use ipnetwork::IpNetwork;

use crate::{tftp_server::TftpHandler, variables::MemRegion, Map, Result, UBootClient};

//const PING_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_millis(150);

impl UBootClient {
    /// Dump MTD part via tftp (fast)
    pub async fn dump_mtd_part_tftp(
        &mut self,
        name: impl AsRef<str>,
        region: &MemRegion,
        address: u64,
    ) -> Result<()> {
        let _name = name.as_ref();

        self.read_mtd_part(region, address).await?;

        //self.tftp_send(name, address, region.size).await?;

        Ok(())
    }

    /// Send memory via TFTP
    pub async fn tftp_send(_name: impl AsRef<str>, _base: u64, _size: u64) -> Result<()> {
        // TODO:
        unimplemented! {}
    }

    /// Start TFTP server
    pub async fn tftp_server(
        client_ip: IpAddr,
        path: impl AsRef<Path>,
        read: bool,
        write: bool,
    ) -> Result<tokio::task::JoinHandle<Result<()>>> {
        let server_ip = Self::server_ip(client_ip)?;

        let handler = TftpHandler::new(path)
            .auth_ip(client_ip)
            .allow_read(read)
            .allow_write(write);

        // Build server
        let tftpd = async_tftp::server::TftpServerBuilder::with_handler(handler)
            .bind(std::net::SocketAddr::new(server_ip, 69))
            // Workaround to handle cases where client is behind VPN
            .block_size_limit(1024)
            .build()
            .await?;

        Ok(tokio::task::spawn(async move {
            // Serve
            let _ = tftpd.serve().await?;

            Ok(())
        }))
    }

    /// Get list of networks to configure tftp server
    pub fn networks() -> Result<Map<String, Vec<IpNetwork>>> {
        let mut interfaces = Map::<String, Vec<IpNetwork>>::default();
        for iface in if_addrs::get_if_addrs()? {
            if iface.is_loopback() {
                continue;
            }
            let networks = interfaces.entry(iface.name).or_default();
            networks.push(match iface.addr {
                if_addrs::IfAddr::V4(addr) => {
                    ipnetwork::Ipv4Network::with_netmask(addr.ip, addr.netmask)?.into()
                }
                if_addrs::IfAddr::V6(addr) => {
                    ipnetwork::Ipv6Network::with_netmask(addr.ip, addr.netmask)?.into()
                }
            });
        }
        Ok(interfaces)
    }

    /// Select server ip address
    pub fn server_ip(ip: IpAddr) -> Result<IpAddr> {
        for (_, networks) in UBootClient::networks()? {
            for network in networks {
                if network.contains(ip) {
                    return Ok(network.ip());
                }
            }
        }
        Err(anyhow::anyhow!("Unable to determine server IP addess"))
    }
}

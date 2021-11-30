mod flash_info;
mod hex_dump;
mod parse_utils;
mod terminal_key;
mod tftp_server;
mod variables;
mod version_info;

use core::{
    pin::Pin,
    task::{Context, Poll},
};
use futures::{select, AsyncRead, AsyncWrite, Future, FutureExt, Stream, StreamExt};
use ipnetwork::IpNetwork;
use std::{borrow::Cow, collections::VecDeque, marker::Unpin, net::IpAddr, path::Path};
use tokio::sync::mpsc;
use tokio_serial as serial;

use flash_info::{FlashInfo, FlashKind};
use hex_dump::HexDump;
use terminal_key::TerminalKey;
use tftp_server::TftpHandler;
use variables::{MemRegion, Variables};
use version_info::VersionInfo;

pub type Map<K, V> = indexmap::IndexMap<K, V, fxhash::FxBuildHasher>;

pub type Result<T> = anyhow::Result<T>;

type Payload = Vec<u8>;

/** Client control message */
#[derive(Debug)]
enum CtlMsg {
    /** Output data */
    Out { payload_data: Payload },
    /** Subscribe to input */
    Sub {
        payload_sender: mpsc::Sender<Payload>,
    },
}

const RX_DELAY: tokio::time::Duration = tokio::time::Duration::from_millis(50);
const TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_millis(150);
const PING_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_millis(150);

#[derive(Clone)]
pub struct UBootClient {
    ctl_tx: mpsc::Sender<CtlMsg>,
}

impl UBootClient {
    pub fn ports() -> Result<Vec<String>> {
        let ports = serial::available_ports()?;
        Ok(ports.into_iter().map(|port| port.port_name).collect())
    }

    pub fn new<'a>(name: impl Into<Cow<'a, str>>, rate: u32) -> Result<Self> {
        let builder = serial::new(name, rate)
            //.data_bits(serial::DataBits::Eight)
            //.stop_bits(serial::StopBits::One)
            //.parity(serial::Parity::None)
            //.flow_control(serial::FlowControl::None)
            //.timeout(core::time::Duration::from_millis(250))
            ;

        let mut port = serial::SerialStream::open(&builder)?;
        port.set_exclusive(true)?;
        let (mut rx_port, mut tx_port) = tokio::io::split(port);

        let (ctl_tx, mut ctl_rx) = mpsc::channel(1000);

        tokio::spawn(async move {
            use std::io::Cursor;
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let mut subscribers: slab::Slab<mpsc::Sender<Payload>> = slab::Slab::with_capacity(4);

            let rx_lim = 64 << 10;
            let mut rx_buf = Vec::with_capacity(rx_lim);

            loop {
                select! {
                    rx_res = rx_port.read_buf(&mut rx_buf).fuse() => {
                        //eprintln!("!!!! rx: {:?}", std::str::from_utf8(&rx_buf));
                        /* remove closed receivers */
                        subscribers.retain(|_, subscriber| !subscriber.is_closed());
                        match rx_res {
                            /* receiver error */
                            Err(_rx_err) => {
                                break;
                            },
                            /* received chunk */
                            Ok(_rx_len) => {
                                for (_, payload_sender) in &subscribers {
                                    let _ = payload_sender.send(rx_buf.clone()).await;
                                }
                                rx_buf.clear();
                            },
                        }
                    },
                    ctl_evt = ctl_rx.recv().fuse() => if let Some(ctl_evt) = ctl_evt {
                        match ctl_evt {
                            CtlMsg::Out {payload_data} => {
                                let mut cursor = Cursor::new(&payload_data);
                                let _ = tx_port.write_all_buf(&mut cursor).await.map_err(|tx_err| tx_err.to_string());
                            },
                            CtlMsg::Sub {payload_sender} => {
                                /* remove closed receivers */
                                subscribers.retain(|_, subscriber| !subscriber.is_closed());
                                subscribers.insert(payload_sender);
                            },
                        }
                    } else {
                        /* handle closed */
                        break;
                    },
                }
            }
        });

        Ok(Self { ctl_tx })
    }

    /// Send raw data
    pub async fn send_raw(&self, data: impl Into<Payload>) -> Result<()> {
        self.ctl_tx
            .send(CtlMsg::Out {
                payload_data: data.into(),
            })
            .await?;
        Ok(())
    }

    /// Send command line
    pub async fn send_cmd(&self, cmd: impl Into<String>) -> Result<()> {
        let mut cmd = cmd.into();
        cmd.push('\r');
        self.send_raw(cmd).await
    }

    /// Receive raw chunks
    pub async fn chunks(&self) -> Result<tokio_stream::wrappers::ReceiverStream<Payload>> {
        let (tx, rx) = mpsc::channel(1000);
        self.ctl_tx.send(CtlMsg::Sub { payload_sender: tx }).await?;
        Ok(tokio_stream::wrappers::ReceiverStream::new(rx))
    }

    /// Receive lines
    pub async fn lines(
        &self,
    ) -> Result<LinesStream<tokio_stream::wrappers::ReceiverStream<Payload>>> {
        Ok(LinesStream::new(self.chunks().await?, RX_DELAY))
    }

    /// Awaiting shell prompt and optionally stop autoboot
    pub async fn shell_presence(&mut self) -> Result<Payload> {
        self.send_raw(TerminalKey::Ctrl(b'C').encode().unwrap())
            .await?;

        let lines = self.lines().await?;
        futures::pin_mut!(lines);

        // try send empty command to get prompt
        self.send_cmd("").await?;

        // try get shell prompt
        loop {
            match tokio::time::timeout(TIMEOUT, lines.next()).await {
                Ok(Some(line)) => {
                    if !line.ends_with(b"\r") {
                        return Ok(line);
                    }
                }
                Ok(None) => anyhow::bail!("Unexpected EOF"),
                Err(_) => break,
            }
        }

        while let Some(line) = lines.next().await {
            if let Ok(line) = core::str::from_utf8(&line) {
                //eprintln!("rx: {:?}", line);
                self.send_raw(TerminalKey::Ctrl(b'C').encode().unwrap())
                    .await?;
                if let Ok(key) = TerminalKey::parse_stop_autoboot(&line) {
                    eprintln!("prevent autoboot!");
                    self.send_raw(key.encode()?).await?;
                    break;
                }
            }
        }

        // try get shell prompt
        loop {
            match tokio::time::timeout(TIMEOUT, lines.next()).await {
                Ok(Some(line)) => {
                    if !line.ends_with(b"\r") {
                        return Ok(line);
                    }
                }
                Ok(None) => anyhow::bail!("Unexpected EOF"),
                Err(_) => anyhow::bail!("Prompt await timeout"),
            }
        }
    }

    /// Get U-Boot version
    pub async fn get_version(&mut self) -> Result<VersionInfo> {
        let lines = self.lines().await?;
        futures::pin_mut!(lines);

        self.send_cmd("getinfo version").await?;

        loop {
            match tokio::time::timeout(TIMEOUT, lines.next()).await {
                Ok(Some(line)) => {
                    if line.ends_with(b"\r") {
                        if let Ok(line) = core::str::from_utf8(&line) {
                            //eprintln!(">> {:?}", line);
                            if let Ok(version) = VersionInfo::parse(&line) {
                                return Ok(version);
                            }
                        }
                    }
                }
                Ok(None) => anyhow::bail!("Unexpected EOF"),
                Err(_) => anyhow::bail!("Version request timeout"),
            }
        }
    }

    /// Get flash info
    pub async fn get_flash_info(&mut self) -> Result<FlashInfo> {
        let lines = self.lines().await?;
        futures::pin_mut!(lines);

        self.send_cmd("getinfo bootmode").await?;

        let kind = loop {
            match tokio::time::timeout(TIMEOUT, lines.next()).await {
                Ok(Some(line)) => {
                    if line.ends_with(b"\r") {
                        if let Ok(line) = core::str::from_utf8(&line) {
                            //eprintln!(">> {:?}", line);
                            if let Ok(kind) = FlashKind::parse(&line) {
                                break kind;
                            }
                        }
                    }
                }
                Ok(None) => anyhow::bail!("Unexpected EOF"),
                Err(_) => anyhow::bail!("Version info timeout"),
            }
        };

        self.send_cmd(match kind {
            FlashKind::Spi => "getinfo spi",
            FlashKind::Nand => "getinfo nand",
        })
        .await?;

        let mut info = FlashInfo::from_kind(kind);

        loop {
            match tokio::time::timeout(TIMEOUT, lines.next()).await {
                Ok(Some(line)) => {
                    if line.ends_with(b"\r") {
                        if let Ok(line) = core::str::from_utf8(&line) {
                            //eprintln!(">> {:?}", line);
                            let _ = info.fill_parse(&line);
                        }
                    }
                }
                Ok(None) => anyhow::bail!("Unexpected EOF"),
                Err(_) => break,
            }
        }

        Ok(info)
    }

    /// Get environment
    pub async fn get_environ(&mut self) -> Result<Variables> {
        let lines = self.lines().await?;
        futures::pin_mut!(lines);

        self.send_cmd("printenv").await?;
        let mut vars = Variables::default();

        loop {
            match tokio::time::timeout(TIMEOUT, lines.next()).await {
                Ok(Some(line)) => {
                    if line.ends_with(b"\r") {
                        if let Ok(line) = core::str::from_utf8(&line) {
                            let _ = vars.extend_parse_env(&line);
                        }
                    }
                }
                Ok(None) => anyhow::bail!("Unexpected EOF"),
                Err(_) => break,
            }
        }

        Ok(vars)
    }

    /// Get board info
    pub async fn get_bdinfo(&mut self) -> Result<Variables> {
        let lines = self.lines().await?;
        futures::pin_mut!(lines);

        self.send_cmd("bdinfo").await?;
        let mut vars = Variables::default();

        loop {
            match tokio::time::timeout(TIMEOUT, lines.next()).await {
                Ok(Some(line)) => {
                    if line.ends_with(b"\r") {
                        if let Ok(line) = core::str::from_utf8(&line) {
                            let _ = vars.extend_parse_env(&line);
                        }
                    }
                }
                Ok(None) => anyhow::bail!("Unexpected EOF"),
                Err(_) => break,
            }
        }

        Ok(vars)
    }

    /// Get RAM info (address and size)
    pub async fn get_ram_info(&mut self) -> Result<MemRegion> {
        let vars = self.get_bdinfo().await?;
        vars.get_ram_info()
    }

    /// Get MTD parts
    pub async fn get_mtd_parts(&mut self) -> Result<Map<String, MemRegion>> {
        let environ = self.get_environ().await?;
        let bootargs = environ
            .get("bootargs")
            .ok_or_else(|| anyhow::anyhow!("Bootargs for found in environment"))?;
        let mut iter = bootargs.splitn(2, "mtdparts=");
        match (iter.next(), iter.next()) {
            (Some(_), Some(args)) => Variables::parse_mtd_parts(args),
            _ => Err(anyhow::anyhow!("No mtdparts found in bootargs")),
        }
    }

    /// Send SPI flash command (sf)
    pub async fn spi_flash_cmd(&mut self, cmd: impl AsRef<str>) -> Result<()> {
        let lines = self.lines().await?;
        futures::pin_mut!(lines);

        self.send_cmd(format!("sf {}", cmd.as_ref())).await?;
        let mut count = 0;

        loop {
            match tokio::time::timeout(TIMEOUT, lines.next()).await {
                Ok(Some(line)) => {
                    if line.ends_with(b"\r") {
                        if let Ok(line) = core::str::from_utf8(&line) {
                            //println!(">> {}", line);
                            count += 1;
                        }
                    }
                }
                Ok(None) => anyhow::bail!("Unexpected EOF"),
                Err(_) => break,
            }
        }

        if count > 0 {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Unable to execute SPI command"))
        }
    }

    /// Read MTD part to RAM
    pub async fn read_mtd_part(&mut self, region: &MemRegion, address: u64) -> Result<()> {
        self.spi_flash_cmd("probe 0").await?;
        self.spi_flash_cmd(format!(
            "read {:#08x} {:#08x} {:#08x}",
            address, region.base, region.size
        ))
        .await?;

        Ok(())
    }

    /// Calculate CRC32 of memory region
    pub async fn calc_crc32(&mut self, address: u64, size: u64) -> Result<u32> {
        let lines = self.lines().await?;
        futures::pin_mut!(lines);

        self.send_cmd(format!("crc32 {:#08x} {:#08x}", address, size))
            .await?;

        loop {
            match tokio::time::timeout(TIMEOUT, lines.next()).await {
                Ok(Some(line)) => {
                    if line.ends_with(b"\r") {
                        let line = core::str::from_utf8(&line)?;
                        if line.starts_with("crc32 for") {
                            if let Some(sum) = line.rsplitn(2, ' ').next() {
                                let (_, sum) = parse_utils::hex_u64(sum).map_err(|err| {
                                    anyhow::anyhow!("Unable to parse crc32: {}", err)
                                })?;
                                return Ok(sum as _);
                            }
                        }
                    }
                }
                Ok(None) => anyhow::bail!("Unexpected EOF"),
                Err(_) => anyhow::bail!("Calc CRC32 timeout"),
            }
        }
    }

    /// Dump MTD part in text mode (slow)
    pub async fn dump_mtd_part(
        &mut self,
        //mut file: impl AsyncWrite + Unpin,
        mut file: impl tokio::io::AsyncWrite + Unpin,
        region: &MemRegion,
        address: u64,
        progress: mpsc::Sender<u64>,
    ) -> Result<()> {
        //use futures::AsyncWriteExt;
        use tokio::io::AsyncWriteExt;

        self.read_mtd_part(region, address).await?;
        let checksum = self.calc_crc32(address, region.size).await?;

        let lines = self.lines().await?;
        futures::pin_mut!(lines);

        self.send_cmd(format!("md.b {:#08x} {:#08x}", address, region.size))
            .await?;

        let mut hasher = crc32fast::Hasher::new();
        let mut off = 0;

        while off < region.size {
            let data = match tokio::time::timeout(TIMEOUT, lines.next()).await {
                Ok(Some(line)) => {
                    if line.ends_with(b"\r") {
                        let line = core::str::from_utf8(&line)?;
                        if line.starts_with("md.b") {
                            continue;
                        }
                        HexDump::parse_line(&line)?
                    } else {
                        anyhow::bail!("Unexpected end of dump");
                    }
                }
                Ok(None) => anyhow::bail!("Unexpected EOF"),
                Err(_) => anyhow::bail!("Dump memory timeout"),
            };

            if data.len() > 16 {
                anyhow::bail!(
                    "Number of bytes per line unexpectedly exceeds 16: {}",
                    data.len()
                );
            }

            hasher.update(&*data);
            file.write_all(&*data).await?;
            off += data.len() as u64;

            if let Err(_) = progress.send(off).await {
                self.send_raw(TerminalKey::Ctrl(b'C').encode().unwrap())
                    .await?;
                break;
            }
        }

        if off > region.size {
            anyhow::bail!("Out of region by {} bytes", off - region.size);
        }

        if checksum != hasher.finalize() {
            anyhow::bail!("Checksum does not matches!");
        }

        Ok(())
    }

    /// Dump MTD part via tftp (fast)
    pub async fn dump_mtd_part_tftp(
        &mut self,
        name: impl AsRef<str>,
        region: &MemRegion,
        address: u64,
    ) -> Result<()> {
        let name = name.as_ref();

        self.read_mtd_part(region, address).await?;

        //self.tftp_send(name, address, region.size).await?;

        Ok(())
    }

    //// Send memory via TFTP
    //pub async fn tftp_send(name: AsRef<str>, base: u64, size: u64) -> Result<()> {

    //}

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

    /*
    /// Get unused ip address in network
    pub async fn select_network_ip(network: &ipnetwork::IpNetwork) -> Result<std::net::IpAddr> {
        let pinger = tokio_icmp_echo::Pinger::new().await?;
        for addr in network.iter() {
            match pinger.ping(addr, 0x55, 0x0, PING_TIMEOUT).await {
                Ok(None) => return Ok(addr),
            }
        }
        Err(anyhow::anyhow!("Unable to search unused IP address"))
    }
    */
}

pin_project_lite::pin_project! {
    pub struct LinesStream<S> {
        #[pin]
        stream: S,
        #[pin]
        sleep: tokio::time::Sleep,
        timeout: tokio::time::Duration,
        leftover: VecDeque<Payload>,
    }
}

impl<S> LinesStream<S> {
    pub fn new(stream: S, timeout: tokio::time::Duration) -> Self {
        let sleep = tokio::time::sleep(timeout);
        let leftover = VecDeque::new();
        Self {
            stream,
            sleep,
            timeout,
            leftover,
        }
    }
}

impl<S> Stream for LinesStream<S>
where
    S: Stream<Item = Payload>,
{
    type Item = Payload;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        // keep last chunk, return other
        if this.leftover.len() > 1 {
            return Poll::Ready(this.leftover.pop_front());
        }

        let stream_poll = this.stream.poll_next(cx);
        let timer_poll = this.sleep.as_mut().poll(cx);

        //eprintln!("poll: {:?} {:?}", stream_poll, timer_poll);

        match stream_poll {
            Poll::Ready(Some(chunk)) => {
                // reset timer
                this.sleep
                    .reset(tokio::time::Instant::now() + *this.timeout);

                // split chunk to lines
                let mut iter = chunk.split(|b| *b == b'\n');

                if let Some(first) = iter.next() {
                    if let Some(last) = this.leftover.back_mut() {
                        // append to last
                        last.extend(first);
                        //last.append(first.into());
                    } else {
                        // push new
                        this.leftover.push_back(first.into());
                    }
                    for line in iter {
                        // push other lines
                        this.leftover.push_back(line.into());
                    }
                    // keep last chunk, return other
                    if this.leftover.len() > 1 {
                        return Poll::Ready(this.leftover.pop_front());
                    }
                }
            }
            // end of stream
            Poll::Ready(None) => {
                // return last keeped chunk
                return Poll::Ready(this.leftover.pop_front());
            }
            _ => (),
        }

        match timer_poll {
            // timeout reached
            Poll::Ready(_) => {
                // return last keeped chunk
                if !this.leftover.is_empty() {
                    return Poll::Ready(this.leftover.pop_front());
                }
            }
            _ => (),
        }

        Poll::Pending
    }
}

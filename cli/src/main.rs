use std::path::PathBuf;

#[cfg(feature = "tftp")]
use std::net::IpAddr;

use structopt::StructOpt;
use uboot_tool::{Result, UBootClient};

#[derive(Debug, StructOpt, Clone, PartialEq)]
#[structopt(about = "UBoot tool for IP Camera firmware management.")]
pub struct Args {
    /// Serial port
    #[structopt(short, long, env = "SERIAL_PORT")]
    pub port: Option<String>,

    /// Baud rate
    #[structopt(short, long, env = "SERIAL_BAUD", default_value = "115200")]
    pub baud: u32,

    /// Path for backup and restore
    #[structopt(short = "f", long, env = "FILE_PATH", parse(from_os_str))]
    pub path: Option<PathBuf>,

    #[cfg(feature = "tftp")]
    /// Ip address of device
    #[structopt(short, long, env = "IP_ADDRESS")]
    pub ip: Option<IpAddr>,

    /// Command
    #[structopt(subcommand)]
    pub command: Cmd,
}

#[derive(Debug, StructOpt, Clone, PartialEq)]
pub enum Cmd {
    /// Show available serial ports
    Ports,

    #[cfg(feature = "tftp")]
    /// Show available networks
    Networks,

    /// Stop autoboot when device connected
    Login,

    /// Get system info
    Info,

    /// Backup environment variables to file
    DumpEnv,

    /// Backup firmware partitions to file
    DumpMtd {
        /// Parts to be dumped (all by default)
        #[structopt(short = "m", long)]
        part: Vec<String>,
    },
}

impl Args {
    pub fn uboot_client(&self) -> Result<UBootClient> {
        let port = self
            .port
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No port is set"))?;
        let baud = self.baud;
        UBootClient::new(port, baud)
    }

    pub fn get_path(&self) -> Result<PathBuf> {
        self.path
            .as_ref()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("No path is set"))
            .or_else(|_| Ok(std::env::current_dir()?))
    }

    //pub fn file_name(&self, name: AsRef<>)

    #[cfg(feature = "tftp")]
    pub fn get_ip(&self) -> Result<std::net::IpAddr> {
        let ip = self
            .ip
            .ok_or_else(|| anyhow::anyhow!("No device IP is set"))?;

        if ip.is_multicast() {
            anyhow::bail!("Device IP address must not be multicast");
        }

        for (_, networks) in UBootClient::networks()? {
            for network in networks {
                if network.ip() == ip {
                    anyhow::bail!("Device IP address must not be same as the host one");
                }
                if network.broadcast() == ip {
                    anyhow::bail!("Device IP address must not be broadcast");
                }
                if network.contains(ip) {
                    return Ok(ip);
                }
            }
        }

        Err(anyhow::anyhow!(
            "Device IP address must be in same network as host"
        ))
    }
}

pub struct ProgressBar {
    out: std::io::Stdout,
    msg: String,
    val: u32,
    max: u32,
    bar_val: u32,
    bar_max: u32,
}

impl ProgressBar {
    pub fn new(msg: impl Into<String>, max: u32) -> Result<Self> {
        let msg = msg.into();
        let bar_max = 80 - b": [] 100%".len() as u32 - msg.len() as u32;
        let mut bar = Self {
            out: std::io::stdout(),
            msg,
            val: 100,
            max: max,
            bar_max,
            bar_val: 0,
        };
        bar.set(0)?;
        Ok(bar)
    }

    pub fn set(&mut self, val: u32) -> Result<()> {
        use std::io::Write;

        let new = val * 100 / self.max;
        let bar = val * self.bar_max / self.max;
        if new != self.val || bar != self.bar_val {
            self.val = new;
            self.bar_val = bar;
            self.out.write_all(self.msg.as_bytes())?;
            self.out.write_all(b": [")?;
            for _ in 0..self.bar_val {
                self.out.write_all(b"#")?;
            }
            for _ in self.bar_val..self.bar_max {
                self.out.write_all(b" ")?;
            }
            self.out.write_all(b"] ")?;
            write!(&mut self.out, "{}%  \r", self.val)?;
            self.out.flush()?;
        }
        Ok(())
    }

    pub fn done(&mut self) -> Result<()> {
        use std::io::Write;

        self.set(self.max)?;
        writeln!(&mut self.out, "")?;
        Ok(())
    }
}

impl Drop for ProgressBar {
    fn drop(&mut self) {
        let _ = self.done();
    }
}

async fn run(args: Args) -> Result<()> {
    match &args.command {
        Cmd::Ports => {
            for port in UBootClient::ports()? {
                println!("{}", port);
            }
        }

        #[cfg(feature = "tftp")]
        Cmd::Networks => {
            for (name, networks) in UBootClient::networks()? {
                println!("{}:", name);
                for network in networks {
                    println!("\t{}/{}", network.ip(), network.prefix());
                }
            }
            if args.ip.is_some() {
                let ip = args.get_ip()?;
                println!("Device IP: {}", ip);
            }
        }

        Cmd::Login => {
            let mut client = args.uboot_client()?;
            let prompt = client.shell_presence().await?;
            let prompt = core::str::from_utf8(&prompt)?;
            println!("prompt: {}", prompt);
        }

        Cmd::Info => {
            let mut client = args.uboot_client()?;
            let _prompt = client.shell_presence().await?;

            let ver = client.get_version().await?;
            println!("U-Boot:\t{}.{}", ver.year, ver.month);
            println!("\trevision:\t{}-{}", ver.revision, ver.suffix);

            let flash = client.get_flash_info().await?;
            println!("Flash {}:", flash.kind.as_str());
            if flash.has_name() {
                println!("\tname:\t{}", flash.name);
            }
            if flash.has_id() {
                println!(
                    "\tid:\t{:#02x} {:#02x} {:#02x}",
                    flash.id[0], flash.id[1], flash.id[2]
                );
            }
            println!("\tsize:\t{:#08x}*{}", flash.size, flash.count);
            println!("\tblock:\t{:#08x}", flash.block);

            let ram = client.get_ram_info().await?;
            println!("RAM:");
            println!("\tbase:\t{:#08x}", ram.base);
            println!("\tsize:\t{:#08x}", ram.size);

            let parts = client.get_mtd_parts().await?;
            if !parts.is_empty() {
                let mut total = 0;
                println!("MTD Parts:");
                for (name, region) in &parts {
                    println!("\t{}:\t{:#08x} {:#08x}", name, region.base, region.size);
                    total += region.size;
                }
                println!("\ttotal=\t\t{:#08x}", total);
            }
        }

        Cmd::DumpEnv => {
            use tokio::io::AsyncWriteExt;

            let path = args.get_path()?.join("env.txt");
            let mut client = args.uboot_client()?;
            let _prompt = client.shell_presence().await?;

            let environ = client.get_environ().await?;
            let mut file = tokio::fs::File::create(path).await?;

            for (key, value) in &*environ {
                file.write_all(key.as_bytes()).await?;
                file.write_all(b"=").await?;
                file.write_all(value.as_bytes()).await?;
                file.write_all(b"\n").await?;
            }
        }

        Cmd::DumpMtd { part } => {
            use tokio::io::AsyncWriteExt;

            let dir = args.get_path()?;
            let mut client = args.uboot_client()?;
            let _prompt = client.shell_presence().await?;

            let ram = client.get_ram_info().await?;
            let address = ram.base + ram.size / 2;
            let parts = client.get_mtd_parts().await?;

            // save parts info
            {
                let path = dir.join(format!("mtd.txt"));
                let mut file = tokio::fs::File::create(&path).await?;
                file.write_all(b"# name size\n").await?;
                for (name, region) in &parts {
                    file.write_all(format!("{} {:#08x}\n", name, region.size).as_bytes())
                        .await?;
                }
            }

            let names = if part.is_empty() {
                Box::new(parts.keys()) as Box<dyn Iterator<Item = &String>>
            } else {
                Box::new(part.iter()) as Box<dyn Iterator<Item = &String>>
            };

            println!("Dumping MTD parts...");

            // save parts contents
            for name in names {
                if let Some(region) = parts.get(name) {
                    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel(10);
                    let path = dir.join(format!("{}.bin", name));
                    let file = tokio::fs::File::create(&path).await?;

                    tokio::task::spawn({
                        let mut client = client.clone();
                        let region = region.clone();
                        async move {
                            if let Err(err) = client
                                .dump_mtd_part(file, &region, address, progress_tx)
                                .await
                            {
                                eprintln!("Error when dumping mtd part: {}", err);
                            }
                        }
                    });

                    let mut bar = ProgressBar::new(name, region.size as _)?;

                    while let Some(progress) = progress_rx.recv().await {
                        bar.set(progress as _)?;
                    }
                } else {
                    eprintln!("Unknown part: {}", name);
                }
            }
        }
    }

    Ok(())
}

#[paw::main]
#[tokio::main(flavor = "current_thread")]
async fn main(args: Args) {
    if let Err(error) = run(args).await {
        eprintln!("Error: {}", error);
    }
}

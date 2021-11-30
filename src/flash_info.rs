use crate::{
    parse_utils::{hex_u8_0x, size_u64},
    Result,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FlashKind {
    Spi,
    Nand,
}

impl Default for FlashKind {
    fn default() -> Self {
        Self::Spi
    }
}

impl FlashKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            FlashKind::Spi => "SPI",
            FlashKind::Nand => "NAND",
        }
    }
}

impl FlashKind {
    pub fn parse(src: impl AsRef<str>) -> Result<Self> {
        use nom::{branch::alt, bytes::complete::tag_no_case as tag, combinator::map, IResult};

        // spi|nand
        fn parse(input: &str) -> IResult<&str, FlashKind> {
            alt((
                map(tag("spi"), |_| FlashKind::Spi),
                map(tag("nand"), |_| FlashKind::Nand),
            ))(input)
        }

        let (_, kind) =
            parse(src.as_ref()).map_err(|err| anyhow::anyhow!("Invalid sequence: {}", err))?;
        Ok(kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FlashInfo {
    /// Chip type
    pub kind: FlashKind,
    /// Block size
    pub block: u32,
    /// Chip size
    pub size: u32,
    /// Number of chips
    pub count: u32,
    /// JEDEC ID
    pub id: [u8; 3],
    /// Name
    pub name: String,
}

impl FlashInfo {
    pub fn has_name(&self) -> bool {
        !self.name.is_empty()
    }

    pub fn has_id(&self) -> bool {
        self.id[0] != 0 && self.id[1] != 0
    }

    pub fn from_kind(kind: FlashKind) -> Self {
        Self {
            kind,
            ..Default::default()
        }
    }

    pub fn fill_parse(&mut self, src: impl AsRef<str>) -> Result<()> {
        use nom::{
            branch::alt,
            bytes::complete::{is_not, tag_no_case as tag},
            character::complete::{char, space1 as space, u16},
            combinator::{map, opt},
            sequence::tuple,
            IResult,
        };

        enum Data {
            Size { block: u32, size: u32, count: u32 },
            Id([u8; 3]),
            Name(String),
        }

        fn parse(input: &str) -> IResult<&str, Data> {
            alt((
                // Block:64KB Chip:8MB*1
                map(
                    tuple((
                        tag("Block:"),
                        size_u64,
                        space,
                        tag("Chip:"),
                        size_u64,
                        opt(tuple((char('*'), u16))),
                    )),
                    |(_, block, _, _, size, count)| {
                        let block = block as u32;
                        let size = size as u32;
                        let count = count.map(|(_, count)| count as u32).unwrap_or(1);
                        Data::Size { block, size, count }
                    },
                ),
                // ID:0xA1 0x40 0x17
                map(
                    tuple((tag("ID:"), hex_u8_0x, space, hex_u8_0x, space, hex_u8_0x)),
                    |(_, a, _, b, _, c)| Data::Id([a, b, c]),
                ),
                // Name:"XM_FM25Q64"
                map(
                    tuple((tag("Name:\""), is_not("\""), char('"'))),
                    |(_, name, _): (_, &str, _)| Data::Name(name.into()),
                ),
            ))(input)
        }

        let (_, data) =
            parse(src.as_ref()).map_err(|err| anyhow::anyhow!("Invalid sequence: {}", err))?;

        match data {
            Data::Size { block, size, count } => {
                self.block = block;
                self.size = size;
                self.count = count;
            }
            Data::Id(id) => {
                self.id = id;
            }
            Data::Name(name) => {
                self.name = name;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn kind_spi() {
        let r = FlashKind::parse("spi\r").unwrap();
        assert_eq!(r, FlashKind::Spi);
    }

    #[test]
    fn kind_nand() {
        let r = FlashKind::parse("nand\r").unwrap();
        assert_eq!(r, FlashKind::Nand);
    }

    #[test]
    fn flash_size() {
        let mut r = FlashInfo::default();
        r.fill_parse("Block:64KB Chip:8MB*1\r").unwrap();
        assert_eq!(
            r,
            FlashInfo {
                block: 64 << 10,
                size: 8 << 20,
                count: 1,
                ..Default::default()
            }
        );
    }

    #[test]
    fn flash_id() {
        let mut r = FlashInfo::default();
        r.fill_parse("ID:0xA1 0x40 0x17\r").unwrap();
        assert_eq!(
            r,
            FlashInfo {
                id: [0xa1, 0x40, 0x17],
                ..Default::default()
            }
        );
    }

    #[test]
    fn flash_name() {
        let mut r = FlashInfo::default();
        r.fill_parse("Name:\"XM_FM25Q64\"\r").unwrap();
        assert_eq!(
            r,
            FlashInfo {
                name: "XM_FM25Q64".into(),
                ..Default::default()
            }
        );
    }
}

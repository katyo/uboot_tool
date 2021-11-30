use crate::Result;

#[derive(Debug, Clone, Default, educe::Educe)]
#[educe(Deref, DerefMut)]
pub struct HexDump {
    #[educe(Deref, DerefMut)]
    data: Vec<u8>,
}

impl HexDump {
    pub fn parse_line(src: impl AsRef<str>) -> Result<Self> {
        use crate::parse_utils::{hex_u64, hex_u8};
        use nom::{
            bytes::complete::take,
            character::complete::{char, space0, space1},
            combinator::map,
            multi::separated_list0,
            sequence::tuple,
            IResult,
        };

        fn parse(input: &str) -> IResult<&str, Vec<u8>> {
            map(
                tuple((
                    hex_u64,
                    char(':'),
                    space0,
                    separated_list0(char(' '), hex_u8),
                )),
                |(_addr, _, _, data)| data,
            )(input)
        }

        let (_, data) = parse(src.as_ref())
            .map_err(|err| anyhow::anyhow!("Unable to parse hexdump line: {}", err))?;

        Ok(Self { data })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_full() {
        let p = HexDump::parse_line(
            "42000000: 15 05 00 ea fe ff ff ea fe ff ff ea fe ff ff ea    ................\r",
        )
        .unwrap();
        assert_eq!(
            &*p,
            &[
                0x15, 0x5, 0x0, 0xea, 0xfe, 0xff, 0xff, 0xea, 0xfe, 0xff, 0xff, 0xea, 0xfe, 0xff,
                0xff, 0xea
            ]
        );
    }

    #[test]
    fn parse_partial() {
        let p = HexDump::parse_line(
            "42000000: 15 05 00 ea fe ff ff ea                            ........\r",
        )
        .unwrap();
        assert_eq!(&*p, &[0x15, 0x5, 0x0, 0xea, 0xfe, 0xff, 0xff, 0xea]);
    }

    #[test]
    fn parse_single() {
        let p =
            HexDump::parse_line("42000000: 15                                                 .\r")
                .unwrap();
        assert_eq!(&*p, &[0x15]);
    }

    #[test]
    fn parse_empty() {
        let p = HexDump::parse_line("42000000:\r").unwrap();
        assert_eq!(&*p, &[]);
    }

    #[test]
    fn parse_overflow() {
        let p = HexDump::parse_line(
            "42000000: 15 05 00 ea fe ff ff ea fe ff ff ea fe ff ff ea    1a .............\r",
        )
        .unwrap();
        assert_eq!(
            &*p,
            &[
                0x15, 0x5, 0x0, 0xea, 0xfe, 0xff, 0xff, 0xea, 0xfe, 0xff, 0xff, 0xea, 0xfe, 0xff,
                0xff, 0xea
            ]
        );
    }
}

use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalKey {
    Any,
    Key(char),
    Ctrl(u8),
}

impl TerminalKey {
    pub fn encode(&self) -> Result<String> {
        Ok(match self {
            Self::Any => 'a'.to_string(),
            Self::Key(key) => key.to_string(),
            Self::Ctrl(code) => {
                unsafe { char::from_u32_unchecked((code - (b'A' - 0x1)) as u32) }.to_string()
            }
        })
    }

    pub fn parse_stop_autoboot(src: impl AsRef<str>) -> Result<Self> {
        use nom::{
            branch::alt,
            bytes::complete::tag_no_case as tag,
            character::complete::{one_of, satisfy, space1 as space},
            combinator::{map, value},
            sequence::tuple,
            IResult,
        };

        fn is_alpha(c: char) -> bool {
            let c = c as u32;
            if c > 255 {
                false
            } else {
                let c = c as u8;
                (c >= b'A' && c <= b'Z') || (c >= b'a' && c <= b'z')
            }
        }

        fn parse(input: &str) -> IResult<&str, TerminalKey> {
            map(
                tuple((
                    value((), tuple((alt((tag("hit"), tag("press"))), space))),
                    alt((
                        map(
                            tuple((tag("ctrl"), one_of("-+"), satisfy(is_alpha))),
                            |(_, _, chr)| {
                                let code = chr as u32 as u8;
                                TerminalKey::Ctrl(if code < b'a' {
                                    code
                                } else {
                                    code - (b'a' - b'A')
                                })
                            },
                        ),
                        map(tuple((tag("any"), space, tag("key"))), |_| TerminalKey::Any),
                        map(satisfy(is_alpha), |chr| TerminalKey::Key(chr)),
                    )),
                    value(
                        (),
                        tuple((space, tag("to"), space, tag("stop"), space, tag("autoboot"))),
                    ),
                )),
                |(_, key, _)| key,
            )(input)
        }

        let (_, key) =
            parse(src.as_ref()).map_err(|err| anyhow::anyhow!("Invalid sequence: {}", err))?;
        Ok(key)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn stop_autoboot_by_any_key() {
        let r = TerminalKey::parse_stop_autoboot("Hit any key to stop autoboot:  1").unwrap();
        assert_eq!(r, TerminalKey::Any);
    }

    #[test]
    fn stop_autoboot_by_key_a() {
        let r = TerminalKey::parse_stop_autoboot("Hit a to stop autoboot:  3").unwrap();
        assert_eq!(r, TerminalKey::Key('a'));
    }

    #[test]
    fn stop_autoboot_by_ctrl_c() {
        let r = TerminalKey::parse_stop_autoboot("Hit ctrl+c to stop autoboot:  0").unwrap();
        assert_eq!(r, TerminalKey::Ctrl(b'C'));
    }

    #[test]
    fn stop_autoboot_by_ctrl_d() {
        let r = TerminalKey::parse_stop_autoboot("Hit Ctrl-D to stop autoboot").unwrap();
        assert_eq!(r, TerminalKey::Ctrl(b'D'));
    }
}

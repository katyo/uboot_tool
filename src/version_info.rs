use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    pub year: u16,
    pub month: u8,
    pub revision: String,
    pub suffix: String,
}

impl VersionInfo {
    pub fn parse(src: impl AsRef<str>) -> Result<Self> {
        use nom::{
            bytes::complete::tag_no_case as tag,
            character::complete::{alphanumeric1, char, space1 as space, u16, u8},
            combinator::{map, opt, value},
            sequence::tuple,
            IResult,
        };

        // version: U-Boot 2016.11-g2fc5f58-dirty
        fn parse(input: &str) -> IResult<&str, VersionInfo> {
            map(
                tuple((
                    value(
                        (),
                        tuple((opt(tuple((tag("version:"), space))), tag("U-Boot"), space)),
                    ),
                    u16,
                    char('.'),
                    u8,
                    map(opt(tuple((char('-'), alphanumeric1))), |sfx| {
                        sfx.map(|(_, sfx): (_, &str)| sfx.into())
                            .unwrap_or_default()
                    }),
                    map(opt(tuple((char('-'), alphanumeric1))), |sfx| {
                        sfx.map(|(_, sfx): (_, &str)| sfx.into())
                            .unwrap_or_default()
                    }),
                )),
                |(_, year, _, month, revision, suffix)| VersionInfo {
                    year,
                    month,
                    revision,
                    suffix,
                },
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
    fn version_full() {
        let r = VersionInfo::parse("version: U-Boot 2016.11-g2fc5f58-dirty\r").unwrap();
        assert_eq!(
            r,
            VersionInfo {
                year: 2016,
                month: 11,
                revision: "g2fc5f58".into(),
                suffix: "dirty".into(),
            }
        );
    }

    #[test]
    fn version_short() {
        let r = VersionInfo::parse("U-Boot 2020.09").unwrap();
        assert_eq!(
            r,
            VersionInfo {
                year: 2020,
                month: 9,
                revision: "".into(),
                suffix: "".into(),
            }
        );
    }
}

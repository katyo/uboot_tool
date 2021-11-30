use crate::{parse_utils::size_u64, Map, Result};

#[derive(Debug, Clone, Default, educe::Educe)]
#[educe(Deref, DerefMut)]
pub struct Variables {
    #[educe(Deref, DerefMut)]
    storage: Map<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct MemRegion {
    pub base: u64,
    pub size: u64,
}

impl Variables {
    pub fn get_u32(&self, key: impl AsRef<str>) -> Result<u32> {
        let value = self.get_u64(key)?;
        if value > u32::MAX as u64 {
            anyhow::bail!("Value out of range u32");
        }
        Ok(value as u32)
    }

    pub fn get_u64(&self, key: impl AsRef<str>) -> Result<u64> {
        let key = key.as_ref();
        let value = self
            .get(key)
            .ok_or_else(|| anyhow::anyhow!("Variable '{}' not found", key))?;
        let (_, value) =
            size_u64(value).map_err(|err| anyhow::anyhow!("Unable to parse u64 value: {}", err))?;
        Ok(value)
    }

    pub fn get_ram_info(&self) -> Result<MemRegion> {
        let base = self.get_u64("-> start")?;
        let size = self.get_u64("-> size")?;
        Ok(MemRegion { base, size })
    }

    pub fn parse_mtd_parts(src: impl AsRef<str>) -> Result<Map<String, MemRegion>> {
        use nom::{
            bytes::complete::take_till,
            character::complete::char,
            combinator::{map, map_res},
            multi::separated_list0,
            sequence::tuple,
            IResult,
        };

        fn parse(input: &str) -> IResult<&str, Vec<(String, u64)>> {
            map(
                tuple((
                    take_till(|c| c == ':'),
                    char(':'),
                    separated_list0(
                        char(','),
                        map_res(
                            tuple((size_u64, char('('), take_till(|c| c == ')'), char(')'))),
                            |(size, _, name, _): (u64, _, &str, _)| -> Result<(String, u64)> {
                                Ok((name.into(), size))
                            },
                        ),
                    ),
                )),
                |(_proto, _, parts)| parts,
            )(input)
        }

        let (_, parts) =
            parse(src.as_ref()).map_err(|err| anyhow::anyhow!("Invalid sequence: {}", err))?;

        Ok(parts
            .into_iter()
            .scan(0, |offset, (name, size)| {
                let region = MemRegion {
                    base: *offset,
                    size,
                };
                *offset += size;
                Some((name, region))
            })
            .collect())
    }

    pub fn extend_parse_arg(&mut self, src: impl AsRef<str>) -> Result<()> {
        self._extend_parse(src, '=', ' ')
    }

    pub fn extend_parse_env(&mut self, src: impl AsRef<str>) -> Result<()> {
        self._extend_parse(src, '=', '\r')
    }

    pub fn _extend_parse(
        &mut self,
        src: impl AsRef<str>,
        kv_sep: char,
        ent_sep: char,
    ) -> Result<()> {
        use nom::{
            bytes::complete::take_till,
            character::complete::{char, space0},
            combinator::map,
            sequence::tuple,
            IResult,
        };

        let mut parse = map(
            tuple((
                space0,
                take_till(|c| c == kv_sep),
                char(kv_sep),
                space0,
                take_till(|c| c == ent_sep),
            )),
            |(_, key, _, _, value): (_, &str, _, _, &str)| (key.trim_end(), value.trim_end()),
        );

        let res: IResult<&str, (&str, &str)> = parse(src.as_ref());
        let (_, (key, value)) = res.map_err(|err| anyhow::anyhow!("Invalid sequence: {}", err))?;

        self.insert(key.into(), value.into());

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_env() {
        let mut r = Variables::default();

        r.extend_parse_env(" baudrate =115200\r").unwrap();
        r.extend_parse_env("bootargs=init=linuxrc mem=${osmem} console=ttyAMA0,115200 root=/dev/mtdblock1 rootfstype=squashfs mtdparts=hi_sfc:0x40000(boot),0x2E0000(romfs),0x420000(user),0x40000(web),0x30000(custom),0x50000(mtd)").unwrap();
        r.extend_parse_env("bootcmd= setenv setargs setenv bootargs ${bootargs};run setargs;sf probe 0;sf read 43000000 40000 550000;squashfsload;bootm 0x42000000\r\n").unwrap();
        r.extend_parse_env("bootdelay=0").unwrap();
        r.extend_parse_env("bootfile=\"uImage\"\r").unwrap();

        assert_eq!(&r["baudrate"], "115200");
        assert_eq!(&r["bootdelay"], "0");
        assert_eq!(&r["bootfile"], "\"uImage\"");
        assert_eq!(&r["bootargs"], "init=linuxrc mem=${osmem} console=ttyAMA0,115200 root=/dev/mtdblock1 rootfstype=squashfs mtdparts=hi_sfc:0x40000(boot),0x2E0000(romfs),0x420000(user),0x40000(web),0x30000(custom),0x50000(mtd)");
        assert_eq!(&r["bootcmd"], "setenv setargs setenv bootargs ${bootargs};run setargs;sf probe 0;sf read 43000000 40000 550000;squashfsload;bootm 0x42000000");
    }

    #[test]
    fn parse_bdinfo() {
        let mut r = Variables::default();

        r.extend_parse_env("arch_number = 0x00001F40\r").unwrap();
        r.extend_parse_env("DRAM bank   = 0x00000000").unwrap();
        r.extend_parse_env("-> start    = 0x40000000\r").unwrap();
        r.extend_parse_env("-> size     = 0x04000000").unwrap();

        assert_eq!(&r["arch_number"], "0x00001F40");
        assert_eq!(&r["DRAM bank"], "0x00000000");
        assert_eq!(&r["-> start"], "0x40000000");
        assert_eq!(&r["-> size"], "0x04000000");
    }
}

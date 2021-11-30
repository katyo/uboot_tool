use nom::{
    branch::alt,
    bytes::complete::tag_no_case as tag,
    character::complete::one_of,
    combinator::{map, opt},
    multi::fold_many1,
    sequence::{preceded, tuple},
    IResult,
};

/// Parse decimal digit
pub fn dec_dig(input: &str) -> IResult<&str, u8> {
    map(one_of("0123456789"), |o: char| o as u8 - b'0')(input)
}

/// Parse hexadecimal digit
pub fn hex_dig(input: &str) -> IResult<&str, u8> {
    map(one_of("0123456789abcdefABCDEF"), |o: char| {
        let o = o as u8;
        o - if o <= b'9' {
            b'0'
        } else if o <= b'Z' {
            b'A' - 10
        } else {
            b'a' - 10
        }
    })(input)
}

/// Parse hexadecimal number as u8
pub fn hex_u8(input: &str) -> IResult<&str, u8> {
    map(tuple((hex_dig, opt(hex_dig))), |(h1, h2)| {
        if let Some(h2) = h2 {
            (h1 << 4) | h2
        } else {
            h1
        }
    })(input)
}

/// Parse hexadecimal number as u8 with 0x prefix
pub fn hex_u8_0x(input: &str) -> IResult<&str, u8> {
    preceded(tag("0x"), hex_u8)(input)
}

/// Parse hexadecimal number as u64
pub fn hex_u64(input: &str) -> IResult<&str, u64> {
    fold_many1(hex_dig, || 0, |val, dig| (val << 4) | dig as u64)(input)
}

/// Parse hexadecimal number as u64 with 0x prefix
pub fn hex_u64_0x(input: &str) -> IResult<&str, u64> {
    preceded(tag("0x"), hex_u64)(input)
}

/// Parse decimal number as u64
pub fn dec_u64(input: &str) -> IResult<&str, u64> {
    fold_many1(hex_dig, || 0, |val, dig| val * 10 + dig as u64)(input)
}

/// Parse optional units to get multiplier
pub fn units_mul(input: &str) -> IResult<&str, u64> {
    map(
        tuple((
            opt(alt((
                map(tag("k"), |_| 1 << 10),
                map(tag("m"), |_| 1 << 20),
            ))),
            opt(tag("b")),
        )),
        |(mul, _)| mul.unwrap_or(1),
    )(input)
}

/// Parse decimal number as u64 with optional units (KB, Mb, k, M, ...)
pub fn dec_u64_units(input: &str) -> IResult<&str, u64> {
    map(tuple((dec_u64, units_mul)), |(val, mul)| val * mul)(input)
}

/// Parse various size value (0x100, 16, 8M, 64KB, etc.)
pub fn size_u64(input: &str) -> IResult<&str, u64> {
    alt((hex_u64_0x, dec_u64_units))(input)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_hex_dig() {
        assert_eq!(hex_dig("0"), Ok(("", 0x0)));
        assert_eq!(hex_dig("5"), Ok(("", 0x5)));
        assert_eq!(hex_dig("a"), Ok(("", 0xa)));
        assert_eq!(hex_dig("A"), Ok(("", 0xa)));
        assert_eq!(hex_dig("f"), Ok(("", 0xf)));
        assert_eq!(hex_dig("F"), Ok(("", 0xf)));
        assert_eq!(hex_dig("Fa"), Ok(("a", 0xf)));
        assert!(hex_dig(".").is_err());
        assert!(hex_dig("g").is_err());
    }

    #[test]
    fn parse_hex_u8() {
        assert_eq!(hex_u8("0"), Ok(("", 0x0)));
        assert_eq!(hex_u8("00"), Ok(("", 0x0)));
        assert_eq!(hex_u8("05"), Ok(("", 0x5)));
        assert_eq!(hex_u8("50"), Ok(("", 0x50)));
        assert_eq!(hex_u8("aA"), Ok(("", 0xaa)));
        assert_eq!(hex_u8("Aa"), Ok(("", 0xaa)));
        assert_eq!(hex_u8("f0"), Ok(("", 0xf0)));
        assert_eq!(hex_u8("0F"), Ok(("", 0xf)));
        assert_eq!(hex_u8("Fa"), Ok(("", 0xfa)));
        assert_eq!(hex_u8("Fab"), Ok(("b", 0xfa)));
        assert!(hex_u8(".").is_err());
        assert!(hex_u8("g0").is_err());
        assert_eq!(hex_u8("0g"), Ok(("g", 0x0)));
    }

    #[test]
    fn parse_hex_u8_0x() {
        assert_eq!(hex_u8_0x("0x1"), Ok(("", 0x1)));
        assert_eq!(hex_u8_0x("0X1"), Ok(("", 0x1)));
        assert_eq!(hex_u8_0x("0x01"), Ok(("", 0x1)));
        assert_eq!(hex_u8_0x("0x012"), Ok(("2", 0x1)));
    }

    #[test]
    fn parse_hex_u64_0x() {
        assert_eq!(hex_u64_0x("0x1"), Ok(("", 0x1)));
        assert_eq!(hex_u64_0x("0x42000000"), Ok(("", 0x42000000)));
    }
}

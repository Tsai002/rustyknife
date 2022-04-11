use nom::{bytes::complete::tag_no_case, sequence::delimited};

use crate::{
    rfc5234::crlf,
    rfc5321::{SMTPString, UTF8Policy, _smtp_string},
    NomResult,
};

/// Parse an SMTP AUTH command.
pub fn command<P: UTF8Policy>(input: &[u8]) -> NomResult<SMTPString> {
    delimited(tag_no_case("AUTH "), _smtp_string::<P>, crlf)(input)
}

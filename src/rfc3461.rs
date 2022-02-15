//! [SMTP DSN] (delivery status notification) extension
//!
//! [SMTP DSN]: https://tools.ietf.org/html/rfc3461

use std::borrow::Cow;
use std::str;

use crate::util::*;

use nom::branch::alt;
use nom::bytes::complete::{tag, tag_no_case, take};
use nom::character::is_hex_digit;
use nom::combinator::{map, map_res, verify};
use nom::multi::many0;
use nom::sequence::{preceded, separated_pair};

use crate::rfc5322::atom;

pub(crate) fn hexpair(input: &[u8]) -> NomResult<u8> {
    map_res(
        verify(take(2usize), |c: &[u8]| c.iter().cloned().all(is_hex_digit)),
        |x| u8::from_str_radix(str::from_utf8(x).unwrap(), 16),
    )(input)
}

fn hexchar(input: &[u8]) -> NomResult<u8> {
    preceded(tag("+"), hexpair)(input)
}

fn xchar(input: &[u8]) -> NomResult<u8> {
    take1_filter(|c| matches!(c, 33..=42 | 44..=60 | 62..=126))(input)
}

pub(crate) fn xtext(input: &[u8]) -> NomResult<Vec<u8>> {
    many0(alt((xchar, hexchar)))(input)
}

fn _printable_xtext(input: &[u8]) -> NomResult<Vec<u8>> {
    verify(xtext, |xtext: &[u8]| {
        xtext.iter().all(|c| matches!(c, 9..=13 | 32..=126))
    })(input)
}

/// Parse the ESMTP ORCPT parameter that may be present on a RCPT TO command.
///
/// Returns the address type and the decoded original recipient address.
/// # Examples
/// ```
/// use rustyknife::rfc3461::orcpt_address;
///
/// let (_, split) = orcpt_address(b"rfc822;bob@example.org").unwrap();
///
/// assert_eq!(split, ("rfc822".into(), "bob@example.org".into()));
/// ```
pub fn orcpt_address(input: &[u8]) -> NomResult<(Cow<str>, Cow<str>)> {
    map(
        separated_pair(atom::<crate::behaviour::Legacy>, tag(";"), _printable_xtext),
        |(a, b)| (ascii_to_string(a), ascii_to_string(b)),
    )(input)
}

/// The DSN return type desired by the sender.
#[derive(Debug, PartialEq)]
pub enum DSNRet {
    /// Return full the full message content.
    Full,
    /// Return only the email headers.
    Hdrs,
}

/// DSN parameters for the MAIL command.
#[derive(Debug, PartialEq)]
pub struct DSNMailParams {
    /// A mail transaction identifier provided by the sender.
    ///
    /// `None` if not specified.
    pub envid: Option<String>,
    /// The DSN return type desired by the sender.
    ///
    /// `None` if not specified.
    pub ret: Option<DSNRet>,
}

type Param<'a> = (&'a str, Option<&'a str>);

/// Parse a list of ESMTP parameters on a MAIL FROM command into a
/// [`DSNMailParams`] option block.
///
/// Returns the option block and a vector of parameters that were not
/// consumed.
/// # Examples
/// ```
/// use rustyknife::rfc3461::{dsn_mail_params, DSNRet, DSNMailParams};
/// let input = &[("RET", Some("HDRS")),
///               ("OTHER", None)];
///
/// let (params, other) = dsn_mail_params(input).unwrap();
///
/// assert_eq!(params, DSNMailParams{ envid: None, ret: Some(DSNRet::Hdrs) });
/// assert_eq!(other, [("OTHER", None)]);
/// ```
pub fn dsn_mail_params<'a>(
    input: &[Param<'a>],
) -> Result<(DSNMailParams, Vec<Param<'a>>), &'static str> {
    let mut out = Vec::new();
    let mut envid_val: Option<String> = None;
    let mut ret_val: Option<DSNRet> = None;

    for (name, value) in input {
        match (name.to_lowercase().as_str(), value) {
            ("ret", Some(value)) => {
                if ret_val.is_some() {
                    return Err("Duplicate RET");
                }

                ret_val = match value.to_lowercase().as_str() {
                    "full" => Some(DSNRet::Full),
                    "hdrs" => Some(DSNRet::Hdrs),
                    _ => return Err("Invalid RET"),
                }
            }

            ("envid", Some(value)) => {
                if envid_val.is_some() {
                    return Err("Duplicate ENVID");
                }
                let value = value.as_bytes();
                if value.len() > 100 {
                    return Err("ENVID over 100 bytes");
                }
                if let Ok((_, parsed)) = exact!(value, _printable_xtext) {
                    envid_val = Some(ascii_to_string(parsed).into());
                } else {
                    return Err("Invalid ENVID");
                }
            }
            ("ret", None) => return Err("RET without value"),
            ("envid", None) => return Err("ENVID without value"),
            _ => out.push((*name, *value)),
        }
    }

    Ok((
        DSNMailParams {
            envid: envid_val,
            ret: ret_val,
        },
        out,
    ))
}

pub struct Notify {
    pub on_success: bool,
    pub on_failure: bool,
    pub delay: bool,
}

fn convert_notify_list(input: Vec<&str>) -> Notify {
    let mut on_success = false;
    let mut on_failure = false;
    let mut delay = false;

    for item in input {
        if item.eq_ignore_ascii_case("success") {
            on_success = true
        } else if item.eq_ignore_ascii_case("failure") {
            on_failure = true
        } else if item.eq_ignore_ascii_case("delay") {
            delay = true
        }
    }

    Notify {
        on_success,
        on_failure,
        delay,
    }
}

fn notify_item(input: &str) -> Result<(&str, &str), nom::Err<()>> {
    alt((
        tag_no_case("success"),
        tag_no_case("failure"),
        tag_no_case("delay"),
    ))(input)
}

pub fn dsn_notify(input: &str) -> Result<(&str, Notify), nom::Err<()>> {
    alt((
        map(tag_no_case("never"), |_| Notify {
            on_success: false,
            on_failure: false,
            delay: false,
        }),
        map(
            fold_prefix0(notify_item, preceded(tag(","), notify_item)),
            convert_notify_list,
        ),
    ))(input)
}

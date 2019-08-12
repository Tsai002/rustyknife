//! Parsers for [Internet Message Format] messages.
//!
//! Comments are ignored. [RFC 2047] decoding is applied where appropriate.
//!
//! [Internet Message Format]: https://tools.ietf.org/html/rfc5322
//! [RFC 2047]: https://tools.ietf.org/html/rfc2047

use std::borrow::Cow;
use std::str;
use std::mem;

use nom::branch::alt;
use nom::bytes::complete::{tag, take};
use nom::combinator::{map, map_opt, opt, recognize};
use nom::multi::{fold_many0, many0, many1};
use nom::sequence::{delimited, pair, preceded, separated_pair, terminated};

use crate::rfc2047::encoded_word;
use crate::rfc5234::*;
use crate::types::{self, *};
use crate::util::*;

fn quoted_pair(input: &[u8]) -> NomResult<char> {
    preceded(tag("\\"), alt((vchar, map(wsp, char::from))))(input)
}

fn ctext(input: &[u8]) -> NomResult<char> {
    map(take1_filter(|c| (33..=39).contains(&c) || (42..=91).contains(&c) || (93..=126).contains(&c)), char::from)(input)
}

#[derive(Clone, Debug)]
enum CommentContent<'a> {
    Text(Cow<'a, str>),
    Comment(Vec<CommentContent<'a>>),
    QP(char),
}

fn ccontent(input: &[u8]) -> NomResult<CommentContent> {
    alt((alt((map(recognize_many1(ctext), |ct| CommentContent::Text(str::from_utf8(ct).unwrap().into())),
              map(quoted_pair, CommentContent::QP))),
         map(comment, CommentContent::Comment)))(input)
}

fn fws(input: &[u8]) -> NomResult<Cow<str>> {
    //CRLF is "semantically invisible"
    map(pair(opt(terminated(recognize_many0(wsp), crlf)),
             recognize_many1(wsp)),
        |(a, b)| {
            match a {
                Some(a) => {
                    let mut out = String::from(str::from_utf8(a).unwrap());
                    out.push_str(str::from_utf8(b).unwrap());
                    Cow::from(out)
                },
                None => Cow::from(str::from_utf8(b).unwrap())
            }
        })(input)
}

pub(crate) fn ofws(input: &[u8]) -> NomResult<Cow<str>> {
    map(opt(fws), |i| i.unwrap_or_else(|| Cow::from("")))(input)
}

fn _concat_comment<'a, I: IntoIterator<Item=CommentContent<'a>>>(comments: I) -> Vec<CommentContent<'a>> {
    let mut out = Vec::new();
    let mut acc_text = String::new();

    let push_text = |bytes: &mut String, out: &mut Vec<CommentContent>| {
        if !bytes.is_empty() {
            out.push(CommentContent::Text(mem::replace(bytes, String::new()).into()))
        }
    };

    for comment in comments.into_iter() {
        match comment {
            CommentContent::Text(text) => acc_text.push_str(&text),
            CommentContent::QP(qp) => acc_text.push(qp),
            _ => { push_text(&mut acc_text, &mut out); out.push(comment) }
        }
    }
    push_text(&mut acc_text, &mut out);

    out
}

fn comment(input: &[u8]) -> NomResult<Vec<CommentContent>> {
    map(delimited(tag("("),
                  pair(fold_many0(pair(ofws, ccontent), Vec::new(), |mut acc, (fws, cc)| {
                      acc.push(CommentContent::Text(fws));
                      acc.push(cc);
                      acc
                  }), ofws),
                  tag(")")),
        |(a, b)| _concat_comment(a.into_iter().chain(std::iter::once(CommentContent::Text(b)))))(input)
}

fn cfws(input: &[u8]) -> NomResult<&[u8]> {
    alt((recognize(pair(many1(pair(ofws, comment)), ofws)), recognize(fws)))(input)
}

fn qtext(input: &[u8]) -> NomResult<char> {
    alt((map(take1_filter(|c| c == 33 || (35..=91).contains(&c) || (93..=126).contains(&c)), char::from),
         _8bit_char))(input)
}

#[cfg(feature = "quoted-string-rfc2047")]
fn qcontent(input: &[u8]) -> NomResult<QContent> {
    alt((map(encoded_word, QContent::EncodedWord),
         map(many1_char(qtext), |q| QContent::Literal(q.into())),
         map(quoted_pair, QContent::QP))
    )(input)
}

#[cfg(not(feature = "quoted-string-rfc2047"))]
fn qcontent(input: &[u8]) -> NomResult<QContent> {
    alt((map(many1_char(qtext), |q| QContent::Literal(q.into())),
         map(quoted_pair, QContent::QP))
    )(input)
}

// quoted-string not surrounded by CFWS
fn _inner_quoted_string(input: &[u8]) -> NomResult<Vec<QContent>> {
    map(delimited(tag("\""),
                  pair(many0(pair(opt(fws), qcontent)), opt(fws)),
                  tag("\"")),
        |(a, b)| {
            let mut out = Vec::with_capacity(a.len()*2+1);
            for (ws, cont) in a {
                match (ws, &cont, out.last()) {
                    #[cfg(feature = "quoted-string-rfc2047")]
                    (_, QContent::EncodedWord(_), Some(QContent::EncodedWord(_))) => (),
                    (Some(ws),_, _) => { out.push(QContent::Literal(ws)); },
                    _ => (),
                }
                out.push(cont);
            }
            if let Some(x) = b { out.push(QContent::Literal(x)) }
            out
        })(input)
}

pub(crate) fn quoted_string(input: &[u8]) -> NomResult<QuotedString> {
    map(delimited(opt(cfws), _inner_quoted_string, opt(cfws)),
        |qc| QuotedString(concat_qs(qc.into_iter())))(input)
}

/// A single mailbox with an optional display name.
#[derive(Clone, Debug, PartialEq)]
pub struct Mailbox {
    /// The display name.
    pub dname: Option<String>,
    /// The address of this mailbox.
    pub address: types::Mailbox,
}

/// A group of many [`Mailbox`].
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// This group's display name.
    pub dname: String,
    /// The members of this group. May be empty.
    pub members: Vec<Mailbox>,
}

/// An address is either a single [`Mailbox`] or a [`Group`].
#[derive(Clone, Debug, PartialEq)]
pub enum Address {
    /// Single [`Mailbox`].
    Mailbox(Mailbox),
    /// [`Group`] of many [`Mailbox`].
    Group(Group),
}

#[derive(Clone, Debug)]
enum QContent<'a> {
    Literal(Cow<'a, str>),
    #[cfg(feature = "quoted-string-rfc2047")]
    EncodedWord(String),
    QP(char),
}

#[derive(Clone, Debug)]
enum Text<'a> {
    Literal(String),
    Atom(&'a str),
}

impl <'a> From<&'a Text<'a>> for &'a str {
    fn from(t: &'a Text<'a>) -> &'a str {
        match t {
            Text::Literal(s) => s,
            Text::Atom(s) => s,
        }
    }
}

fn concat_qs<'a, A: Iterator<Item=QContent<'a>>>(input: A) -> String {
    let mut out = String::new();

    for qc in input {
        match qc {
            QContent::Literal(lit) => out.push_str(&lit),
            #[cfg(feature = "quoted-string-rfc2047")]
            QContent::EncodedWord(ew) => out.push_str(&ew),
            QContent::QP(c) => out.push(c),
        }
    }
    out
}

pub(crate) fn atext(input: &[u8]) -> NomResult<char> {
    map(take1_filter(|c| b"!#$%&'*+-/=?^_`{|}~".contains(&c) || (b'0'..=b'9').contains(&c) || (b'A'..=b'Z').contains(&c) || (b'a'..=b'z').contains(&c)), char::from)(input)
}

pub(crate) fn dot_atom(input: &[u8]) -> NomResult<DotAtom> {
    map(delimited(opt(cfws), recognize(pair(recognize_many1(atext), recognize_many0(pair(tag("."), recognize_many1(atext))))), opt(cfws)),
        |a| (DotAtom(str::from_utf8(a).unwrap().into())))(input)
}

pub(crate) fn atom(input: &[u8]) -> NomResult<&[u8]> {
    delimited(opt(cfws), recognize_many1(atext), opt(cfws))(input)
}

pub(crate) fn _padded_encoded_word(input: &[u8]) -> NomResult<String> {
    delimited(opt(cfws), encoded_word, opt(cfws))(input)
}

fn word(input: &[u8]) -> NomResult<Text> {
    alt((
        map(_padded_encoded_word, Text::Literal),
        map(atom, |x| Text::Atom(str::from_utf8(&x).unwrap())),
        map(quoted_string, |qs| Text::Literal(qs.0))
    ))(input)
}

fn _concat_atom_and_qs<'a, A>(input: A) -> String
    where A: Iterator<Item=Text<'a>>,
{
    let mut iter = input.peekable();
    let mut out = String::new();

    while let Some(cur) = iter.next() {
        match (cur, iter.peek()) {
            (Text::Atom(v), Some(_)) => {out.push_str(&v); out.push(' ')},
            (_, Some(Text::Atom(v))) => {out.push_str(&v); out.push(' ')},
            (ref t1, _) => out.push_str(t1.into()),
        };
    };

    out
}

fn display_name(input: &[u8]) -> NomResult<String> {
    map(many1(word), |words| _concat_atom_and_qs(words.into_iter().map(Into::into)))(input)
}

pub(crate) fn local_part(input: &[u8]) -> NomResult<LocalPart> {
    alt((map(dot_atom, |a| a.into()),
         map(quoted_string, LocalPart::Quoted)))(input)
}

fn dtext(input: &[u8]) -> NomResult<char> {
    map(take1_filter(|c| (33..=90).contains(&c) || (94..=126).contains(&c)), char::from)(input)
}

pub(crate) fn domain_literal(input: &[u8]) -> NomResult<AddressLiteral> {
    map(delimited(pair(opt(cfws), tag("[")),
                  pair(many0(pair(ofws, recognize_many1(dtext))), ofws),
                  pair(tag("]"), opt(cfws))),
        |(a, b)| {
            let mut out: String = a.iter().flat_map(|(x, y)| x.chars().chain(str::from_utf8(y).unwrap().chars())).collect();
            out.push_str(&b);
            let literal = AddressLiteral::FreeForm(out);
            literal.upgrade().unwrap_or(literal)
        })(input)
}

pub(crate) fn _domain(input: &[u8]) -> NomResult<Domain> {
    map(dot_atom, |a| Domain(a.0))(input)
}

pub(crate) fn domain(input: &[u8]) -> NomResult<DomainPart> {
    alt((map(_domain, DomainPart::Domain),
         map(domain_literal, DomainPart::Address)))(input)
}

pub(crate) fn addr_spec(input: &[u8]) -> NomResult<types::Mailbox> {
    map(separated_pair(local_part, tag("@"), domain),
        |(lp, domain)| types::Mailbox(lp, domain))(input)
}

fn angle_addr(input: &[u8]) -> NomResult<types::Mailbox> {
    delimited(pair(opt(cfws), tag("<")),
              addr_spec,
              pair(tag(">"), opt(cfws)))(input)
}

fn name_addr(input: &[u8]) -> NomResult<Mailbox> {
    map(pair(opt(display_name), angle_addr),
        |(dname, address)| Mailbox{dname, address})(input)
}

fn mailbox(input: &[u8]) -> NomResult<Mailbox> {
    alt((name_addr,
         map(addr_spec, |a| Mailbox{dname: None, address: a})))(input)
}

fn mailbox_list(input: &[u8]) -> NomResult<Vec<Mailbox>> {
    fold_prefix0(mailbox, preceded(tag(","), mailbox))(input)
}

fn group_list(input: &[u8]) -> NomResult<Vec<Mailbox>> {
    alt((mailbox_list, map(cfws, |_| vec![])))(input)
}

fn group(input: &[u8]) -> NomResult<Group> {
    map(pair(terminated(display_name, tag(":")),
             terminated(opt(group_list), pair(tag(";"), opt(cfws)))),
        |(dname, members)| Group{dname, members: members.unwrap_or_default()})(input)
}

fn address(input: &[u8]) -> NomResult<Address> {
    alt((map(mailbox, Address::Mailbox),
         map(group, Address::Group)))(input)
}

fn address_list(input: &[u8]) -> NomResult<Vec<Address>> {
    fold_prefix0(address, preceded(tag(","), address))(input)
}

fn address_list_crlf(input: &[u8]) -> NomResult<Vec<Address>> {
    terminated(address_list, opt(crlf))(input)
}

fn address_crlf(input: &[u8]) -> NomResult<Address> {
    terminated(address, opt(crlf))(input)
}

fn _8bit_char(input: &[u8]) -> NomResult<char> {
    map(take1_filter(|c| (0x80..=0xff).contains(&c)), |_| '\u{fffd}')(input)
}

/// Parse an unstructured header such as `"Subject:"`.
///
/// Returns a fully decoded string.
pub fn unstructured(input: &[u8]) -> NomResult<String> {
    map(pair(
        many0(alt((
            pair(ofws, map(fold_prefix0(encoded_word, preceded(fws, encoded_word)), |ew| ew.into_iter().collect())),
            pair(ofws, map(many1(alt((vchar, _8bit_char))), |c| c.iter().collect::<String>()))
        ))),
        many0(wsp)),
        |(words, ws)| {
            let mut out = String::new();
            for (word_ws, word) in words {
                out.push_str(&word_ws);
                out.push_str(&word);
            }
            out.push_str(str::from_utf8(&ws).unwrap());
            out
        })(input)
}

/// Parse the content of a `"From:"` header.
///
/// Returns a list of addresses, since [RFC 6854] allows multiple mail
/// authors.
///
/// [RFC 6854]: https://tools.ietf.org/html/rfc6854
pub fn from(i: &[u8]) -> NomResult<Vec<Address>> {
    address_list_crlf(i)
}

/// Parse the content of a `"Sender:"` header.
///
/// Returns a single address.
pub fn sender(i: &[u8]) -> NomResult<Address> {
    address_crlf(i)
}

/// Parse the content of a `"Reply-To:"` header.
///
/// Returns a list of addresses.
pub fn reply_to(i: &[u8]) -> NomResult<Vec<Address>> {
    address_list_crlf(i)
}

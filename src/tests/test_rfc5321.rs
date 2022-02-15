use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;

use crate::behaviour::*;
use crate::rfc5321::*;
use crate::types::*;

fn dp<T: Into<String>>(value: T) -> DomainPart {
    DomainPart::Domain(Domain(value.into()))
}

#[test]
fn empty_from() {
    let (_, (path, params)) = mail_command::<Intl>(b"MAIL FROM:<>\r\n").unwrap();
    assert_eq!(path, ReversePath::Null);
    assert_eq!(params, []);
}

#[test]
#[should_panic]
fn empty_rcpt() {
    rcpt_command::<Intl>(b"RCPT TO:<>\r\n").unwrap();
}

#[test]
#[should_panic]
fn invalid_from() {
    mail_command::<Intl>(b"MAIL FROM:<pa^^&*(sarobas@example.org>\r\n").unwrap();
}

#[test]
#[should_panic]
fn invalid_rcpt() {
    rcpt_command::<Intl>(b"RCPT TO:<pa^^&*(sarobas@example.org>\r\n").unwrap();
}

#[test]
fn esmtp_param() {
    let (_, (path, params)) =
        rcpt_command::<Intl>(b"RCPT TO:<mrbob?@example.org> ORCPT=rfc822;mrbob+AD@example.org\r\n")
            .unwrap();
    assert_eq!(
        path,
        ForwardPath::Path(Path(
            Mailbox(DotAtom("mrbob?".into()).into(), dp("example.org")),
            vec![]
        ))
    );
    assert_eq!(
        params,
        [Param::new("ORCPT", Some("rfc822;mrbob+AD@example.org")).unwrap()]
    );
}

#[test]
fn address_literal_domain() {
    let (_, (path, params)) = rcpt_command::<Intl>(b"RCPT TO:<bob@[127.0.0.1]>\r\n").unwrap();
    assert_eq!(
        path,
        ForwardPath::Path(Path(
            Mailbox(
                DotAtom("bob".into()).into(),
                DomainPart::Address(AddressLiteral::IP(IpAddr::V4(
                    Ipv4Addr::from_str("127.0.0.1").unwrap()
                )))
            ),
            vec![]
        ))
    );
    assert_eq!(params, []);
}

#[test]
fn esmtp_from() {
    let (_, (path, params)) =
        mail_command::<Intl>(b"MAIL FROM:<bob@example.com> RET=FULL ENVID=abc123\r\n").unwrap();
    assert_eq!(
        path,
        ReversePath::Path(Path(
            Mailbox(DotAtom("bob".into()).into(), dp("example.com")),
            vec![]
        ))
    );
    assert_eq!(
        params,
        [
            Param::new("RET", Some("FULL")).unwrap(),
            Param::new("ENVID", Some("abc123")).unwrap()
        ]
    );
}

#[test]
fn quoted_from() {
    let (_, (path, params)) = mail_command::<Intl>(
        b"MAIL FROM:<\"bob the \\\"great \\\\ powerful\\\"\"@example.com>\r\n",
    )
    .unwrap();
    assert_eq!(
        path,
        ReversePath::Path(Path(
            Mailbox(
                QuotedString("bob the \"great \\ powerful\"".into()).into(),
                dp("example.com")
            ),
            vec![]
        ))
    );
    assert_eq!(params, []);
}

#[test]
fn postmaster_rcpt() {
    let (_, (path, params)) = rcpt_command::<Intl>(b"RCPT TO:<pOstmaster>\r\n").unwrap();
    assert_eq!(path, ForwardPath::PostMaster(None));
    assert_eq!(params, []);

    let (_, (path, params)) =
        rcpt_command::<Intl>(b"RCPT TO:<pOstmaster@Domain.example.org>\r\n").unwrap();
    assert_eq!(
        path,
        ForwardPath::PostMaster(Some(Domain::from_smtp(b"Domain.example.org").unwrap()))
    );
    assert_eq!(params, []);
}

#[test]
fn validate() {
    assert_eq!(validate_address::<Intl>(b"mrbob@example.org"), true);
    assert_eq!(validate_address::<Intl>(b"mrbob\"@example.org"), false);
}

#[test]
fn overquoted_lp() {
    let mut lp = LocalPart::Quoted(QuotedString("a.b".into()));
    lp.smtp_try_unquote();
    assert_eq!(lp, LocalPart::DotAtom(DotAtom("a.b".into())));
}

#[test]
fn normal_quoted_lp() {
    let mut lp = LocalPart::Quoted(QuotedString("a b".into()));
    lp.smtp_try_unquote();
    assert_eq!(lp, LocalPart::Quoted(QuotedString("a b".into())));
}

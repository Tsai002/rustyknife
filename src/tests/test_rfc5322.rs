use crate::behaviour::{Intl, Legacy};
use crate::rfc5322::{from, reply_to, sender, unstructured, Address, Group, Mailbox};
use crate::types::{Mailbox as SMTPMailbox, *};

fn dp<T: Into<String>>(value: T) -> DomainPart {
    DomainPart::Domain(Domain(value.into()))
}

fn parse_single<'a, E, F>(syntax: F, input: &'a [u8]) -> Mailbox
where
    F: Fn(&'a [u8]) -> Result<(&'a [u8], Vec<Address>), E>,
    E: std::fmt::Debug,
{
    let (rem, mut parsed) = syntax(input).unwrap();
    assert_eq!(rem.len(), 0);
    assert_eq!(parsed.len(), 1);

    match parsed.remove(0) {
        Address::Mailbox(mbox) => mbox,
        _ => unreachable!(),
    }
}

#[test]
fn concat_atom() {
    assert_eq!(
        parse_single(from::<Intl>, b" atom                 <ignored@example>").dname,
        Some("atom".into())
    );
    assert_eq!(
        parse_single(from::<Intl>, b" atom  atom           <ignored@example>").dname,
        Some("atom atom".into())
    );
    assert_eq!(
        parse_single(from::<Intl>, b" atom  atom   atom    <ignored@example>").dname,
        Some("atom atom atom".into())
    );
}

#[test]
fn concat_qs() {
    let parsed = parse_single(
        from::<Intl>,
        b"\"no\"   \"space\"   space space \"two  space\" \"end space \" <ignored@example>\r\n",
    );
    assert_eq!(
        parsed.dname,
        Some("nospace space space two  spaceend space ".into())
    );
}

#[test]
fn simple_from() {
    let parsed = parse_single(from::<Intl>, b"John Doe <jdoe@machine.example>\r\n");
    assert_eq!(parsed.dname, Some("John Doe".into()));
    assert_eq!(
        parsed.address,
        SMTPMailbox(DotAtom("jdoe".into()).into(), dp("machine.example"))
    )
}

#[test]
fn simple_sender() {
    let (rem, parsed) = sender::<Intl>(b"Michael Jones <mjones@machine.example>\r\n").unwrap();
    assert_eq!(rem.len(), 0);
    if let Address::Mailbox(Mailbox { dname, address }) = parsed {
        assert_eq!(dname, Some("Michael Jones".into()));
        assert_eq!(
            address,
            SMTPMailbox(DotAtom("mjones".into()).into(), dp("machine.example"))
        )
    } else {
        unreachable!();
    }
}

#[test]
fn simple_reply_to() {
    let parsed = parse_single(
        reply_to::<Intl>,
        b"\"Mary Smith: Personal Account\" <smith@home.example>\r\n",
    );
    assert_eq!(parsed.dname, Some("Mary Smith: Personal Account".into()));
    assert_eq!(
        parsed.address,
        SMTPMailbox(DotAtom("smith".into()).into(), dp("home.example"))
    )
}

#[test]
fn group_reply_to() {
    let (rem, parsed) = reply_to::<Intl>(b"  A Group(Some people)\r\n    :Chris Jones <c@(Chris's host.)public.example>,\r\n        joe@example.org,\r\n John <jdoe@one.test> (my dear friend); (the end of the group)\r\n").unwrap();
    assert_eq!(rem.len(), 0);
    assert_eq!(
        parsed,
        [Address::Group(Group {
            dname: "A Group".into(),
            members: vec![
                Mailbox {
                    dname: Some("Chris Jones".into()),
                    address: SMTPMailbox(DotAtom("c".into()).into(), dp("public.example"))
                },
                Mailbox {
                    dname: None,
                    address: SMTPMailbox(DotAtom("joe".into()).into(), dp("example.org"))
                },
                Mailbox {
                    dname: Some("John".into()),
                    address: SMTPMailbox(DotAtom("jdoe".into()).into(), dp("one.test"))
                },
            ]
        })]
    );
}

#[test]
fn multi_reply_to() {
    let (rem, parsed) =
        reply_to::<Intl>(b"Mary Smith <mary@x.test>, jdoe@example.org, Who? <one@y.test>\r\n")
            .unwrap();
    assert_eq!(rem.len(), 0);
    assert_eq!(
        parsed,
        [
            Address::Mailbox(Mailbox {
                dname: Some("Mary Smith".into()),
                address: SMTPMailbox(DotAtom("mary".into()).into(), dp("x.test"))
            }),
            Address::Mailbox(Mailbox {
                dname: None,
                address: SMTPMailbox(DotAtom("jdoe".into()).into(), dp("example.org"))
            }),
            Address::Mailbox(Mailbox {
                dname: Some("Who?".into()),
                address: SMTPMailbox(DotAtom("one".into()).into(), dp("y.test"))
            }),
        ]
    );
}

#[test]
fn folded_qs() {
    let parsed = parse_single(
        reply_to::<Intl>,
        b"\"Mary\r\n Smith\"\r\n <mary@\r\n x.test(comment)>\r\n",
    );
    assert_eq!(parsed.dname, Some("Mary Smith".into()));
    assert_eq!(
        parsed.address,
        SMTPMailbox(DotAtom("mary".into()).into(), dp("x.test"))
    );
}

#[test]
fn intl_subject() {
    let (rem, parsed) = unstructured::<Intl>(b"=?x-sjis?B?lEWWQI7Kg4GM9ZTygs6CtSiPzik=?=").unwrap();
    assert_eq!(rem.len(), 0);
    assert_eq!(parsed, "忍法写メ光飛ばし(笑)");
}

#[test]
fn direct_utf8() {
    let input = b"\xc3\xa9";
    let (rem, parsed) = unstructured::<Intl>(input).unwrap();
    assert_eq!(rem.len(), 0);
    assert_eq!(parsed, "é");

    let (rem, parsed) = unstructured::<Legacy>(input).unwrap();
    assert_eq!(rem.len(), 0);
    assert_eq!(parsed, "\u{fffd}\u{fffd}");
}

#[test]
fn invalid_latin1() {
    let input = b"\xe9";

    let (rem, parsed) = unstructured::<Intl>(input).unwrap();
    assert_eq!(rem.len(), 0);
    assert_eq!(parsed, "\u{fffd}");

    let (rem, parsed) = unstructured::<Legacy>(input).unwrap();
    assert_eq!(rem.len(), 0);
    assert_eq!(parsed, "\u{fffd}");
}

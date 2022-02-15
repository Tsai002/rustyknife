use std::fmt::Debug;
use std::fs::File;

use crate::behaviour::{Intl, Legacy};
use crate::headersection::header_section;
use crate::rfc2231::{content_disposition, content_transfer_encoding, content_type};
use crate::rfc3461::{dsn_mail_params, orcpt_address, DSNMailParams, DSNRet};
use crate::rfc5321::{
    mail_command, rcpt_command, validate_address, ForwardPath, Param as ESMTPParam, ReversePath,
};
use crate::rfc5322::{from, reply_to, sender, unstructured, Address, Group, Mailbox};
use crate::util::NomResult;
use crate::xforward::{xforward_params, Param as XFORWARDParam};

use memmap::Mmap;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyTuple};
use pyo3::{self, PyErr, PyObject, PyResult, Python, ToPyObject};

impl IntoPy<PyObject> for Address {
    fn into_py(self, py: Python) -> PyObject {
        match self {
            Address::Mailbox(m) => m.into_py(py),
            Address::Group(g) => g.into_py(py),
        }
    }
}

impl IntoPy<PyObject> for Group {
    fn into_py(self, py: Python) -> PyObject {
        PyTuple::new(py, &[self.dname.to_object(py), self.members.into_py(py)]).to_object(py)
    }
}
impl IntoPy<PyObject> for Mailbox {
    fn into_py(self, py: Python) -> PyObject {
        PyTuple::new(
            py,
            &[
                self.dname.to_object(py),
                self.address.to_string().to_object(py),
            ],
        )
        .to_object(py)
    }
}

impl IntoPy<PyObject> for XFORWARDParam {
    fn into_py(self, py: Python) -> PyObject {
        PyTuple::new(py, &[self.0.to_object(py), self.1.to_object(py)]).to_object(py)
    }
}

impl IntoPy<PyObject> for ESMTPParam {
    fn into_py(self, py: Python) -> PyObject {
        PyTuple::new(
            py,
            &[
                self.0.to_object(py),
                self.1.as_ref().map(|v| &v.0).to_object(py),
            ],
        )
        .to_object(py)
    }
}

impl IntoPy<PyObject> for DSNMailParams {
    fn into_py(self, py: Python) -> PyObject {
        let out = PyDict::new(py);

        out.set_item("envid", self.envid.clone()).unwrap();
        out.set_item(
            "ret",
            match self.ret {
                Some(DSNRet::Hdrs) => Some("HDRS"),
                Some(DSNRet::Full) => Some("FULL"),
                None => None,
            },
        )
        .unwrap();
        out.to_object(py)
    }
}

impl IntoPy<PyObject> for ForwardPath {
    fn into_py(self, py: Python) -> PyObject {
        match self {
            ForwardPath::Path(p) => p.0.to_string().to_object(py),
            ForwardPath::PostMaster(None) => "postmaster".to_object(py),
            ForwardPath::PostMaster(Some(d)) => format!("postmaster@{}", d).to_object(py),
        }
    }
}

impl IntoPy<PyObject> for ReversePath {
    fn into_py(self, py: Python) -> PyObject {
        match self {
            ReversePath::Path(p) => p.0.to_string().to_object(py),
            ReversePath::Null => "".to_object(py),
        }
    }
}

fn convert_result<O, E: Debug>(input: NomResult<O, E>, match_all: bool) -> PyResult<O> {
    match input {
        Ok((rem, out)) => {
            if match_all && !rem.is_empty() {
                Err(PyErr::new::<PyValueError, _>("Whole input did not match"))
            } else {
                Ok(out)
            }
        }
        Err(err) => Err(PyErr::new::<PyValueError, _>(format!("{:?}.", err))),
    }
}

fn header_section_slice(py: Python, input: &[u8]) -> PyResult<PyObject> {
    let (rem, out) = header_section(input)
        .map_err(|err| PyErr::new::<PyValueError, _>(format!("{:?}.", err)))?;

    let header_end = input.len().checked_sub(rem.len()).unwrap();
    let headers: Vec<_> = out
        .into_iter()
        .map(|h| match h {
            Ok((name, value)) => (PyBytes::new(py, name), PyBytes::new(py, value)).to_object(py),
            Err(invalid) => (py.None(), PyBytes::new(py, invalid)).to_object(py),
        })
        .collect();

    Ok((headers, header_end).to_object(py))
}

#[pymodule]
fn rustyknife(_py: Python, m: &PyModule) -> PyResult<()> {
    /// from_(input)
    #[pyfn(m, "from_")]
    fn py_from(input: &PyBytes) -> PyResult<Vec<Address>> {
        convert_result(from::<Intl>(input.as_bytes()), true)
    }

    /// sender(input)
    #[pyfn(m, "sender")]
    fn py_sender(input: &PyBytes) -> PyResult<Address> {
        convert_result(sender::<Intl>(input.as_bytes()), true)
    }

    /// reply_to(input)
    #[pyfn(m, "reply_to")]
    fn py_reply_to(input: &PyBytes) -> PyResult<Vec<Address>> {
        convert_result(reply_to::<Intl>(input.as_bytes()), true)
    }

    /// header_section(input) -> ([headers...], end of headers position)
    ///
    /// :param input: Input string.
    /// :type input: bytes
    /// :return: A list of separated header (name, value) tuples with
    ///  the exact byte position of the end of headers.
    /// :rtype: list of byte string tuples
    #[pyfn(m, "header_section")]
    fn py_header_section(py2: Python, input: &PyBytes) -> PyResult<PyObject> {
        header_section_slice(py2, input.as_bytes())
    }

    /// header_section_file(fname) -> ([headers...], end of headers position)
    ///
    /// :param fname: File name to read.
    /// :type fname: str
    /// :return: Same as :meth:`header_section`
    #[pyfn(m, "header_section_file")]
    fn py_header_section_file(py2: Python, fname: &str) -> PyResult<PyObject> {
        let file = File::open(fname)?;
        let fmap = unsafe { Mmap::map(&file)? };

        header_section_slice(py2, &fmap)
    }

    /// xforward_params(input)
    #[pyfn(m, "xforward_params")]
    fn py_xforward_params(input: &PyBytes) -> PyResult<Vec<XFORWARDParam>> {
        convert_result(xforward_params(input.as_bytes()), true)
    }

    /// orcpt_address(input)
    #[pyfn(m, "orcpt_address")]
    fn py_orcpt_address(input: &str) -> PyResult<(String, String)> {
        convert_result(
            orcpt_address(input.as_bytes()).map(|(rem, a)| (rem, (a.0.into(), a.1.into()))),
            true,
        )
    }

    /// dsn_mail_params(input)
    #[pyfn(m, "dsn_mail_params")]
    fn py_dsn_mail_params(
        py2: Python,
        input: Vec<(&str, Option<&str>)>,
    ) -> PyResult<(PyObject, PyObject)> {
        dsn_mail_params(&input)
            .map(|(parsed, rem)| (parsed.into_py(py2), rem.to_object(py2)))
            .map_err(PyErr::new::<PyValueError, _>)
    }

    /// mail_command(input)
    ///
    /// :param input: Full SMTP MAIL command
    ///
    ///  b'MAIL FROM:<user@example.org> BODY=7BIT\\\\r\\\\n'
    /// :type input: bytes
    /// :return: (address, [(param, param_value), ...])
    #[pyfn(m, "mail_command")]
    pub fn py_mail_command(input: &PyBytes) -> PyResult<(ReversePath, Vec<ESMTPParam>)> {
        convert_result(mail_command::<Legacy>(input.as_bytes()), true)
    }

    /// rcpt_command(input)
    ///
    /// :param input: Full SMTP RCPT command
    ///
    ///  b'RCPT TO:<user@example.org> ORCPT=rfc822;user@example.org\\\\r\\\\n'
    /// :type input: bytes
    /// :return: (address, [(param, param_value), ...])
    #[pyfn(m, "rcpt_command")]
    pub fn py_rcpt_command(input: &PyBytes) -> PyResult<(ForwardPath, Vec<ESMTPParam>)> {
        convert_result(rcpt_command::<Legacy>(input.as_bytes()), true)
    }

    /// validate_address(address)
    ///
    /// :param address: Non-empty address without <> brackets.
    /// :type address: str
    /// :rtype: bool
    #[pyfn(m, "validate_address")]
    pub fn py_validate_address(input: &str) -> bool {
        validate_address::<Legacy>(input.as_bytes())
    }

    /// unstructured(input)
    ///
    /// Decode an unstructured email header.
    ///
    /// Useful for converting subject lines.
    ///
    /// :param input: Input string
    /// :type input: bytes
    /// :return: Decoded header
    /// :rtype: str
    #[pyfn(m, "unstructured")]
    fn py_unstructured(input: &PyBytes) -> PyResult<String> {
        convert_result(unstructured::<Intl>(input.as_bytes()), true)
    }

    /// content_type(input, all=False)
    #[pyfn(m, "content_type", input, all = false)]
    fn py_content_type(input: &PyBytes, all: bool) -> PyResult<(String, Vec<(String, String)>)> {
        convert_result(content_type(input.as_bytes()), all)
    }

    /// content_disposition(input, all=False)
    #[pyfn(m, "content_disposition", input, all = false)]
    fn py_content_disposition(
        input: &PyBytes,
        all: bool,
    ) -> PyResult<(String, Vec<(String, String)>)> {
        convert_result(content_disposition(input.as_bytes()), all)
            .map(|(cd, params)| (cd.to_string().to_lowercase(), params))
    }

    /// content_transfer_encoding(input, all=False)
    ///
    /// Parse a MIME Content-Transfer-Encoding header.
    ///
    /// Standard encodings such as 7bit, 8bit are accepted. Extended
    /// encodings starting with a 'x-' prefix are also accepted.
    ///
    /// :param input: Input string.
    /// :type input: bytes
    /// :return: Validated Content-Transfer-Encoding
    /// :rtype: str
    ///
    #[pyfn(m, "content_transfer_encoding", input, all = false)]
    fn py_content_transfer_encoding(input: &PyBytes, all: bool) -> PyResult<String> {
        convert_result(content_transfer_encoding(input.as_bytes()), all)
            .map(|cte| cte.to_string().to_lowercase())
    }

    Ok(())
}

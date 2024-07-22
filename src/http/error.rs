use super::*;

#[derive(Debug)]
pub enum HeaderRepr {
	String(String),
	Bytes(Vec<u8>)
}

impl From<&str> for HeaderRepr {
	fn from(value: &str) -> Self {
		Self::String(value.to_string())
	}
}

impl From<String> for HeaderRepr {
	fn from(value: String) -> Self {
		Self::String(value)
	}
}

impl From<&[u8]> for HeaderRepr {
	fn from(value: &[u8]) -> Self {
		Self::Bytes(value.to_vec())
	}
}

impl From<Vec<u8>> for HeaderRepr {
	fn from(value: Vec<u8>) -> Self {
		Self::Bytes(value)
	}
}

#[errors]
pub enum HttpError {
	#[display("Headers too long")]
	#[kind = ErrorKind::InvalidData]
	HeadersTooLong,

	#[display("Invalid HTTP status line: {}", f0)]
	#[kind = ErrorKind::InvalidData]
	InvalidStatusLine(String),

	#[display("Invalid header name {:?}", f0)]
	#[kind = ErrorKind::InvalidData]
	InvalidHeaderName(HeaderRepr),

	#[display("Invalid header value {:?}", f0)]
	#[kind = ErrorKind::InvalidData]
	InvalidHeaderValue(HeaderRepr),

	#[display("Invalid value for header '{}': {}", f0, f1)]
	#[kind = ErrorKind::InvalidData]
	InvalidHeader(HeaderName, String),

	#[display("Chunk too large")]
	#[kind = ErrorKind::Overflow]
	ChunkTooLarge,

	#[display("Unexpected version {}", f0)]
	#[kind = ErrorKind::InvalidData]
	UnexpectedVersion(Version)
}

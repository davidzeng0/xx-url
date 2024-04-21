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
	#[error("Headers too long")]
	HeadersTooLong,

	#[error("Invalid HTTP status line: {0}")]
	InvalidStatusLine(String),

	#[error("Invalid header name {0:?}")]
	InvalidHeaderName(HeaderRepr),

	#[error("Invalid header value {0:?}")]
	InvalidHeaderValue(HeaderRepr),

	#[error("Invalid value for header '{0}': {1}")]
	InvalidHeader(HeaderName, String),

	#[error("Chunk too large")]
	ChunkTooLarge,

	#[error("Unexpected version {0}")]
	UnexpectedVersion(Version)
}

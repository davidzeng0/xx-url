use xx_core::error::*;

#[compact_error]
pub enum HttpError {
	HeadersTooLong    = (ErrorKind::Other, "Headers too long"),
	InvalidStatusLine = (ErrorKind::InvalidData, "Invalid HTTP status line")
}

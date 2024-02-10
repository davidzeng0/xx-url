use xx_core::error::*;

#[compact_error]
pub enum UrlError {
	InvalidScheme     = (
		ErrorKind::InvalidInput,
		"URL Scheme is invalid for this request"
	),
	PartialFile       = (ErrorKind::UnexpectedEof, "Partial file"),
	RedirectForbidden = (
		ErrorKind::InvalidData,
		"Redirect forbidden due to change in url scheme"
	)
}

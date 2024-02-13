use xx_core::error::*;

#[compact_error]
pub enum UrlError {
	InvalidUrl         = (ErrorKind::InvalidInput, "Invalid url"),
	InvalidScheme      = (
		ErrorKind::InvalidInput,
		"URL Scheme is invalid for this request"
	),
	PartialFile        = (ErrorKind::UnexpectedEof, "Partial file"),
	InvalidRedirectUrl = (ErrorKind::InvalidData, "Invalid redirect url"),
	RedirectForbidden  = (
		ErrorKind::InvalidData,
		"Redirect forbidden due to change in url scheme"
	),
	DnsTimedOut        = (ErrorKind::TimedOut, "DNS query timed out")
}

use xx_core::error::*;

#[errors]
pub enum UrlError {
	#[error("Request with an error has been consumed")]
	InvalidRequest,

	#[error("{}", f0)]
	#[kind = ErrorKind::InvalidInput]
	InvalidUrl(
		#[from]
		#[source]
		url::ParseError
	),

	#[error("Scheme \"{}\" is invalid for this request", f0)]
	#[kind = ErrorKind::InvalidData]
	InvalidScheme(String),

	#[error("Partial file")]
	#[kind = ErrorKind::UnexpectedEof]
	PartialFile,

	#[error("Invalid redirect URL \"{}\"", f0)]
	#[kind = ErrorKind::InvalidData]
	InvalidRedirectUrl(String),

	#[error("Redirect forbidden due to change in url scheme to {}", f0)]
	#[kind = ErrorKind::InvalidData]
	RedirectForbidden(String),

	#[error("DNS query timed out")]
	#[kind = ErrorKind::TimedOut]
	DnsTimedOut
}

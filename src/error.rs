use xx_core::error::*;

#[errors]
pub enum UrlError {
	#[display("Request with an error has been consumed")]
	InvalidRequest,

	#[display("{}", f0)]
	#[kind = ErrorKind::InvalidInput]
	InvalidUrl(
		#[from]
		#[source]
		url::ParseError
	),

	#[display("Scheme \"{}\" is invalid for this request", f0)]
	#[kind = ErrorKind::InvalidData]
	InvalidScheme(String),

	#[display("Partial file")]
	#[kind = ErrorKind::UnexpectedEof]
	PartialFile,

	#[display("Invalid redirect URL \"{}\"", f0)]
	#[kind = ErrorKind::InvalidData]
	InvalidRedirectUrl(String),

	#[display("Redirect forbidden due to change in url scheme to {}", f0)]
	#[kind = ErrorKind::InvalidData]
	RedirectForbidden(String),

	#[display("DNS query timed out")]
	#[kind = ErrorKind::TimedOut]
	DnsTimedOut
}

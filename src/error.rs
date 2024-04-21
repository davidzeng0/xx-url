use xx_core::error::*;

#[errors]
pub enum UrlError {
	#[error("Request with an error has been consumed")]
	InvalidRequest,

	#[error("{0}")]
	InvalidUrl(#[from] url::ParseError),

	#[error("Scheme \"{0}\" is invalid for this request")]
	InvalidScheme(String),

	#[error("Partial file")]
	PartialFile,

	#[error("Invalid redirect URL {0}")]
	InvalidRedirectUrl(String),

	#[error("Redirect forbidden due to change in url scheme to {0}")]
	RedirectForbidden(String),

	#[error("DNS query timed out")]
	DnsTimedOut
}

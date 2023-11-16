pub mod body;
pub mod request;
pub mod response;
pub mod stats;
pub(crate) mod stream;
pub(crate) mod transfer;

use num_derive::FromPrimitive;

#[derive(FromPrimitive, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub enum Version {
	Http09 = 9,
	Http10 = 10,
	Http11 = 11,
	Http20 = 20,
	Http30 = 30
}

impl Version {
	pub fn as_str(&self) -> &'static str {
		match self {
			Version::Http09 => "HTTP/0.9",
			Version::Http10 => "HTTP/1.0",
			Version::Http11 => "HTTP/1.1",
			Version::Http20 => "HTTP/2.0",
			Version::Http30 => "HTTP/3.0"
		}
	}
}

pub use body::*;
pub use request::*;
pub use response::*;
pub use stats::*;

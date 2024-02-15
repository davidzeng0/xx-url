use ::http::{Method, StatusCode};
use num_derive::FromPrimitive;
use xx_core::{async_std::io::*, opt::hint::*};

use super::*;

pub mod body;
pub use body::*;
pub mod error;
pub use error::*;
pub mod request;
pub use request::*;
pub mod response;
pub use response::*;
pub mod stats;
pub use stats::*;

pub(crate) mod stream;
use stream::*;
pub(crate) mod transfer;
use transfer::*;

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

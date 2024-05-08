use std::{
	fmt,
	time::{Duration, Instant}
};

use ::http::{header::*, Method, StatusCode};
use memchr::memchr;
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use xx_core::{
	async_std::io::{typed::*, *},
	debug,
	opt::hint::*,
	trace, warn
};

use super::*;

pub mod body;
pub mod error;
pub mod request;
pub mod response;
pub mod stats;
pub(crate) mod stream;
pub(crate) mod transfer;

pub use body::*;
pub use error::*;
pub use request::*;
pub use response::*;
pub use stats::*;
use stream::*;
use transfer::*;

#[derive(FromPrimitive, PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Debug)]
pub enum Version {
	Http09 = 9,
	Http10 = 10,
	Http11 = 11,
	Http20 = 20,
	Http30 = 30
}

impl Version {
	#[must_use]
	pub const fn as_str(&self) -> &'static str {
		match self {
			Self::Http09 => "HTTP/0.9",
			Self::Http10 => "HTTP/1.0",
			Self::Http11 => "HTTP/1.1",
			Self::Http20 => "HTTP/2.0",
			Self::Http30 => "HTTP/3.0"
		}
	}
}

impl fmt::Display for Version {
	fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.as_str().fmt(fmt)
	}
}

#[derive(Clone, Debug, Default)]
pub struct Headers(HeaderMap);

pub trait TryIntoHeaderName {
	fn try_into_name(self) -> Result<HeaderName>;
}

impl TryIntoHeaderName for &str {
	fn try_into_name(self) -> Result<HeaderName> {
		HeaderName::try_from(self).map_err(|_| HttpError::InvalidHeaderName(self.into()).into())
	}
}

impl TryIntoHeaderName for &[u8] {
	fn try_into_name(self) -> Result<HeaderName> {
		HeaderName::try_from(self).map_err(|_| HttpError::InvalidHeaderName(self.into()).into())
	}
}

impl TryIntoHeaderName for HeaderName {
	fn try_into_name(self) -> Result<HeaderName> {
		Ok(self)
	}
}

pub trait TryIntoHeaderValue {
	fn try_into_value(self) -> Result<HeaderValue>;
}

impl TryIntoHeaderValue for &str {
	fn try_into_value(self) -> Result<HeaderValue> {
		HeaderValue::try_from(self).map_err(|_| HttpError::InvalidHeaderValue(self.into()).into())
	}
}

impl TryIntoHeaderValue for &[u8] {
	fn try_into_value(self) -> Result<HeaderValue> {
		HeaderValue::try_from(self).map_err(|_| HttpError::InvalidHeaderValue(self.into()).into())
	}
}

impl TryIntoHeaderValue for HeaderValue {
	fn try_into_value(self) -> Result<HeaderValue> {
		Ok(self)
	}
}

#[allow(clippy::impl_trait_in_params)]
impl Headers {
	wrapper_functions! {
		inner = self.0;

		pub fn capacity(&self) -> usize;
		pub fn reserve(&mut self, additional: usize);
		pub fn iter(&self) -> Iter<'_, HeaderValue>;
		pub fn iter_mut(&mut self) -> IterMut<'_, HeaderValue>;
		pub fn keys(&self) -> Keys<'_, HeaderValue>;
		pub fn values(&self) -> Values<'_, HeaderValue>;
		pub fn values_mut(&mut self) -> ValuesMut<'_, HeaderValue>;
		pub fn drain(&mut self) -> Drain<'_, HeaderValue>;
		pub fn clear(&mut self);
	}

	#[must_use]
	pub fn new() -> Self {
		Self(HeaderMap::new())
	}

	pub fn contains_key(&self, key: impl TryIntoHeaderName) -> bool {
		let Ok(key) = key.try_into_name() else {
			return false;
		};

		self.0.contains_key(key)
	}

	pub fn get(&self, key: impl TryIntoHeaderName) -> Option<&HeaderValue> {
		let Ok(key) = key.try_into_name() else {
			return None;
		};

		self.0.get(key)
	}

	pub fn get_str(&self, key: impl TryIntoHeaderName) -> Result<Option<&str>> {
		let Ok(key) = key.try_into_name() else {
			return Ok(None);
		};

		Ok(match self.0.get(key) {
			Some(value) => Some(value.to_str().map_err(|_| Core::InvalidUtf8)?),
			None => None
		})
	}

	/// # Panics
	/// if `key` cannot be converted into a `HeaderName`
	pub fn entry(&mut self, key: impl TryIntoHeaderName) -> Entry<'_, HeaderValue> {
		let key = key.try_into_name().unwrap();

		self.0.entry(key)
	}

	pub fn insert(
		&mut self, key: impl TryIntoHeaderName, value: impl TryIntoHeaderValue
	) -> Result<()> {
		self.0.insert(key.try_into_name()?, value.try_into_value()?);

		Ok(())
	}

	pub fn remove(&mut self, key: impl TryIntoHeaderName) -> Option<HeaderValue> {
		let Ok(key) = key.try_into_name() else {
			return None;
		};

		self.0.remove(key)
	}
}

impl<'a> IntoIterator for &'a Headers {
	type IntoIter = Iter<'a, HeaderValue>;
	type Item = (&'a HeaderName, &'a HeaderValue);

	fn into_iter(self) -> Self::IntoIter {
		self.iter()
	}
}

impl<'a> IntoIterator for &'a mut Headers {
	type IntoIter = IterMut<'a, HeaderValue>;
	type Item = (&'a HeaderName, &'a mut HeaderValue);

	fn into_iter(self) -> Self::IntoIter {
		self.iter_mut()
	}
}

enum PayloadRepr {
	Bytes(Box<[u8]>),
	Stream(Box<dyn Read>)
}

pub struct Payload(PayloadRepr);

impl From<&[u8]> for Payload {
	fn from(value: &[u8]) -> Self {
		Self(PayloadRepr::Bytes(value.into()))
	}
}

impl From<Vec<u8>> for Payload {
	fn from(value: Vec<u8>) -> Self {
		Self(PayloadRepr::Bytes(value.into()))
	}
}

impl From<Box<dyn Read>> for Payload {
	fn from(value: Box<dyn Read>) -> Self {
		Self(PayloadRepr::Stream(value))
	}
}

impl<T: Read + 'static> From<Box<T>> for Payload {
	fn from(value: Box<T>) -> Self {
		Self(PayloadRepr::Stream(value))
	}
}

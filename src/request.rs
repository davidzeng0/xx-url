#![allow(unreachable_pub)]

use std::mem::replace;

use super::*;

pub struct RequestBase {
	url: Result<Url>
}

impl RequestBase {
	#[allow(clippy::impl_trait_in_params)]
	pub fn new<F>(url: impl AsRef<str>, scheme_allowed: F) -> Self
	where
		F: FnOnce(&str) -> bool
	{
		let mut this = Self {
			url: Url::parse(url.as_ref()).map_err(|err| UrlError::InvalidUrl(err).into())
		};

		if let Ok(url) = &this.url {
			if !scheme_allowed(url.scheme()) {
				this.fail(UrlError::InvalidScheme(url.scheme().to_string()));
			}
		}

		this
	}

	#[allow(clippy::impl_trait_in_params)]
	pub fn fail(&mut self, error: impl Into<Error>) {
		self.url = Err(error.into());
	}

	pub fn url(&self) -> Option<&Url> {
		self.url.as_ref().ok()
	}

	#[allow(clippy::missing_panics_doc)]
	pub fn finalize(&mut self) -> Result<&Url> {
		if self.url.is_ok() {
			Ok(self.url.as_ref().unwrap())
		} else {
			let url = replace(&mut self.url, Err(UrlError::InvalidRequest.into()));

			Err(url.unwrap_err())
		}
	}
}

use super::*;

pub struct Response {
	response: RawResponse,
	body: Body
}

#[asynchronous]
impl Response {
	pub async fn fetch(request: &mut HttpRequest) -> Result<Self> {
		let (response, reader) = transfer(&mut request.inner, None).await?;
		let body = Body::new(reader, &request.inner, &response)?;

		Ok(Self { response, body })
	}

	#[must_use]
	pub const fn stats(&self) -> &Stats {
		&self.response.stats
	}

	#[must_use]
	pub const fn version(&self) -> Version {
		self.response.version
	}

	#[must_use]
	pub const fn status(&self) -> StatusCode {
		self.response.status
	}

	#[must_use]
	pub const fn headers(&self) -> &Headers {
		&self.response.headers
	}

	#[must_use]
	pub const fn url(&self) -> Option<&Url> {
		self.response.url.as_ref()
	}

	#[must_use]
	pub fn into_body(self) -> Body {
		self.body
	}

	pub fn body(&mut self) -> &mut Body {
		&mut self.body
	}

	pub async fn bytes(&mut self) -> Result<Vec<u8>> {
		let mut bytes = Vec::new();

		self.body().read_to_end(&mut bytes).await?;

		check_interrupt().await?;

		Ok(bytes)
	}

	pub async fn text(&mut self) -> Result<String> {
		let mut string = String::new();

		self.body().read_to_string(&mut string).await?;

		check_interrupt().await?;

		Ok(string)
	}
}

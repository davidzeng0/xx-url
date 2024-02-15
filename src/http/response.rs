use std::collections::HashMap;

use xx_core::coroutines::check_interrupt;

use super::*;

pub struct Response {
	response: RawResponse,
	body: Body
}

#[asynchronous]
impl Response {
	pub async fn fetch(request: &HttpRequest) -> Result<Response> {
		let (response, reader) = transfer(&request.inner, None).await?;
		let body = Body::new(reader, &request.inner, &response)?;

		Ok(Self { response, body })
	}

	pub fn stats(&self) -> &Stats {
		&self.response.stats
	}

	pub fn version(&self) -> Version {
		self.response.version
	}

	pub fn status(&self) -> StatusCode {
		self.response.status
	}

	pub fn headers(&self) -> &HashMap<String, String> {
		&self.response.headers
	}

	pub fn url(&self) -> Option<&Url> {
		self.response.url.as_ref()
	}

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

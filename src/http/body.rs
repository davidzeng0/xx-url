use std::mem::size_of;
use std::str::from_utf8;

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ChunkedState {
	Size,
	Extension(u64),
	Data(u64),
	Trailer
}

#[derive(PartialEq, Eq)]
enum Transfer {
	/// No more data left to read
	Empty,

	/// EOF when the underlying connection EOFs
	Connection,

	/// Content-Length header
	Length(u64),

	/// Transfer-Encoding: Chunked
	Chunks(ChunkedState),

	/// Chunked trailers
	Trailers
}

pub struct Body {
	reader: BufReader<HttpConn>,
	transfer: Transfer,
	reusable: bool
}

#[asynchronous]
impl Body {
	pub(super) fn new(
		reader: BufReader<HttpConn>, request: &Request, response: &RawResponse
	) -> Result<Self> {
		let mut body = Self {
			reader,
			transfer: Transfer::Connection,
			reusable: false
		};

		let bodyless = match (&request.method, response.status.as_u16()) {
			(&Method::HEAD, _) => true,
			(_, 204 | 304) => true,
			(_, code) => (100..200).contains(&code)
		};

		if bodyless {
			body.transfer = Transfer::Empty;
		} else if let Some(encoding) = response.headers.get_str(header::TRANSFER_ENCODING)? {
			#[allow(clippy::redundant_closure_for_method_calls)]
			for encoding in encoding.split(',').map(|e| e.trim()) {
				if encoding.eq_ignore_ascii_case("chunked") {
					body.transfer = Transfer::Chunks(ChunkedState::Size);

					break;
				}
			}
		} else if let Some(length) = response.headers.get_str(header::CONTENT_LENGTH)? {
			let len = length.parse().map_err(|_| {
				HttpError::InvalidHeader(header::CONTENT_LENGTH, length.to_string())
			})?;

			body.transfer = Transfer::Length(len);
		}

		if let Some(conn) = response.headers.get_str(header::CONNECTION)? {
			if conn.eq_ignore_ascii_case("keep-alive") {
				body.reusable = true;
			}
		}

		Ok(body)
	}

	async fn read_bytes(&mut self, buf: &mut [u8]) -> Result<usize> {
		if !self.reader.buffer().is_empty() {
			return self.reader.read(buf).await;
		}

		self.reader.inner_mut().read(buf).await
	}

	async fn read_chunk_size(&mut self) -> Result<()> {
		/* double the size of u64, double again for hex */
		#[allow(clippy::arithmetic_side_effects)]
		let max_hex = size_of::<u64>() * 2 * 2 + 1;
		let mut index;

		loop {
			let len = self.reader.buffer().len().min(max_hex);
			let buf = &self.reader.buffer()[..len];

			index = buf.iter().position(|x| !x.is_ascii_hexdigit());

			if index.is_some() {
				break;
			}

			if len >= max_hex {
				break;
			}

			/* fill does not discard unconsumed bytes */
			if unlikely(self.reader.fill().await? == 0) {
				return Err(UrlError::PartialFile.into());
			}
		}

		let chunk_size = index
			.and_then(|index| {
				let str = from_utf8(&self.reader.buffer()[0..index]).unwrap();
				let size = u64::from_str_radix(str, 16).ok();

				self.reader.consume(index);

				size
			})
			.ok_or(HttpError::ChunkTooLarge)?;

		self.transfer = Transfer::Chunks(ChunkedState::Extension(chunk_size));

		Ok(())
	}

	async fn read_until_newline(&mut self) -> Result<()> {
		loop {
			match memchr(b'\n', self.reader.buffer()) {
				Some(index) => {
					#[allow(clippy::arithmetic_side_effects)]
					self.reader.consume(index + 1);

					break;
				}

				None => self.reader.discard()
			};

			if unlikely(self.reader.fill().await? == 0) {
				return Err(UrlError::PartialFile.into());
			}
		}

		if let Transfer::Chunks(ChunkedState::Extension(size)) = self.transfer {
			self.transfer = if size == 0 {
				Transfer::Trailers
			} else {
				Transfer::Chunks(ChunkedState::Data(size))
			};
		} else {
			self.transfer = Transfer::Chunks(ChunkedState::Size);
		}

		Ok(())
	}

	async fn read_chunks(&mut self, mut state: ChunkedState, buf: &mut [u8]) -> Result<usize> {
		loop {
			match state {
				ChunkedState::Size => {
					self.read_chunk_size().await?;
				}

				ChunkedState::Data(mut remaining) => {
					read_into!(buf, remaining.try_into().unwrap_or(usize::MAX));

					let read = self.read_bytes(buf).await?;

					if unlikely(read == 0) {
						return Err(UrlError::PartialFile.into());
					}

					#[allow(clippy::arithmetic_side_effects)]
					(remaining -= read as u64);

					self.transfer = if remaining == 0 {
						Transfer::Chunks(ChunkedState::Trailer)
					} else {
						Transfer::Chunks(ChunkedState::Data(remaining))
					};

					return Ok(read);
				}

				_ => self.read_until_newline().await?
			}

			if let Transfer::Chunks(st) = self.transfer {
				state = st;
			} else {
				break Ok(0);
			}
		}
	}

	pub async fn read_trailer(
		&mut self
	) -> Result<Option<(HeaderName, Option<HeaderValue>, usize)>> {
		assert!(
			self.transfer == Transfer::Trailers,
			"There is either is data left in the body or the stream has been exhausted"
		);

		let header = read_header_line_limited(&mut self.reader).await?;

		if header.is_none() {
			self.transfer = Transfer::Empty;
		}

		Ok(header)
	}

	pub async fn read_trailers(&mut self) -> Result<Headers> {
		let mut headers = Headers::new();

		while let Some((key, value, _)) = self.read_trailer().await? {
			let value = value.unwrap_or_else(|| {
				warn!(target: &*self, "== Header separator not found");

				HeaderValue::from_static("")
			});

			headers.insert(key, value)?;
		}

		Ok(headers)
	}

	#[must_use]
	pub const fn remaining(&self) -> Option<u64> {
		match self.transfer {
			Transfer::Empty => Some(0),
			Transfer::Length(remaining) => Some(remaining),
			_ => None
		}
	}
}

#[asynchronous]
impl Read for Body {
	async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
		/* don't do read_into! here as it's done after calculating remaining bytes */
		match &self.transfer {
			Transfer::Empty | Transfer::Trailers => Ok(0),

			Transfer::Chunks(state) => self.read_chunks(*state, buf).await,

			Transfer::Connection => {
				read_into!(buf);

				let read = self.read_bytes(buf).await?;

				if unlikely(read == 0) {
					self.transfer = Transfer::Empty;
				}

				Ok(read)
			}

			Transfer::Length(remaining) => {
				let mut remaining = *remaining;

				read_into!(buf, remaining.try_into().unwrap_or(usize::MAX));

				let read = self.read_bytes(buf).await?;

				if unlikely(read == 0) {
					return Err(UrlError::PartialFile.into());
				}

				#[allow(clippy::arithmetic_side_effects)]
				(remaining -= read as u64);

				self.transfer = if remaining > 0 {
					Transfer::Length(remaining)
				} else {
					Transfer::Empty
				};

				Ok(read)
			}
		}
	}
}

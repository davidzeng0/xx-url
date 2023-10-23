use std::{collections::HashMap, mem::size_of, str::from_utf8_unchecked};

use http::Method;
use memchr::memchr;
use xx_async_runtime::Context;
use xx_core::{
	async_std::io::*, coroutines::async_trait_fn, error::*, opt::hint::*, read_into, warn
};
use xx_pulse::*;

use super::{stream::HttpStream, transfer::*};

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
	Trailers
}

pub struct Body {
	reader: BufReader<Context, HttpStream>,
	transfer: Transfer,
	reusable: bool
}

#[async_fn]
impl Body {
	pub(crate) fn new(
		reader: BufReader<Context, HttpStream>, request: &Request, response: &Response
	) -> Result<Self> {
		let mut body = Self {
			reader,
			transfer: Transfer::Connection,
			reusable: false
		};

		let bodyless = request.method == Method::HEAD ||
			match response.status.as_u16() {
				204 | 304 => true,
				code => code >= 100 && code < 200
			};

		if bodyless {
			body.transfer = Transfer::Empty;
		} else if let Some(encoding) = response.headers.get("transfer-encoding") {
			for encoding in encoding.split(',').map(|e| e.trim()) {
				if encoding == "chunked" {
					body.transfer = Transfer::Chunks(ChunkedState::Size);

					break;
				}
			}
		} else if let Some(length) = response.headers.get("content-length") {
			match u64::from_str_radix(&length, 10) {
				Ok(len) => body.transfer = Transfer::Length(len),
				Err(err) => {
					return Err(Error::new(
						ErrorKind::InvalidData,
						format!("Invalid content length: {}", err.to_string())
					))
				}
			}
		}

		if let Some(conn) = response.headers.get("connection") {
			if conn.eq_ignore_ascii_case("keep-alive") {
				body.reusable = true;
			}
		}

		Ok(body)
	}

	async fn read_bytes(&mut self, buf: &mut [u8]) -> Result<usize> {
		if unlikely(self.reader.buffer().len() > 0) {
			return self.reader.read(buf).await;
		}

		self.reader.inner().read(buf).await
	}

	fn eof_error() -> Error {
		Error::new(ErrorKind::UnexpectedEof, "Partial file")
	}

	async fn read_chunk_size(&mut self) -> Result<()> {
		/* double the size of u64, double again for hex */
		let amount = size_of::<u64>() * 2 * 2;

		let index = loop {
			match self
				.reader
				.buffer()
				.iter()
				.position(|x| !x.is_ascii_hexdigit())
			{
				Some(index) => break index as i32,
				None => ()
			}

			if self.reader.buffer().len() >= amount {
				break -1;
			}

			if unlikely(self.reader.fill_amount(amount).await? == 0) {
				return Err(Self::eof_error());
			}
		};

		let chunk_size = if index != -1 {
			/* safe because all characters are ascii hexdigits */
			let str = unsafe { from_utf8_unchecked(&self.reader.buffer()[0..index as usize]) };

			u64::from_str_radix(str, 16).ok()
		} else {
			None
		}
		.ok_or(Error::new(ErrorKind::InvalidData, "Chunk size too large"))?;

		/* index can't be negative here */
		self.reader.consume(index as usize);
		self.transfer = Transfer::Chunks(ChunkedState::Extension(chunk_size));

		Ok(())
	}

	async fn read_until_newline(&mut self) -> Result<()> {
		let mut amount = 32;

		loop {
			match memchr(b'\n', self.reader.buffer()) {
				None => self.reader.discard(),
				Some(index) => {
					self.reader.consume(index + 1);

					break;
				}
			};

			if unlikely(self.reader.fill_amount(amount).await? == 0) {
				return Err(Self::eof_error());
			}

			amount *= 2;
			amount = amount.max(self.reader.capacity());
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
			if self.reader.buffer().len() == 0 {
				/* do this so that future fills will start from the beginning */
				self.reader.discard();
			}

			match state {
				ChunkedState::Size => {
					self.read_chunk_size().await?;
				}

				ChunkedState::Data(mut remaining) => {
					read_into!(buf, remaining as usize);

					let read = self.read_bytes(buf).await?;

					if unlikely(read == 0) {
						return Err(Self::eof_error());
					}

					remaining -= read as u64;

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
		&mut self, out_key: &mut String, out_val: &mut String
	) -> Result<Option<usize>> {
		if self.transfer != Transfer::Trailers {
			return Err(Error::new(
				ErrorKind::Other,
				"Invalid state: either there is data left in the body or the stream has been \
				 exhausted"
			));
		}

		Ok(match read_header_line_limited(&mut self.reader).await? {
			None => {
				self.transfer = Transfer::Empty;

				None
			}

			Some((key, value, read)) => {
				*out_key = key;
				*out_val = value.unwrap_or_else(|| {
					warn!(target: self, "Header separator not found");

					"".to_string()
				});

				Some(read)
			}
		})
	}

	pub async fn read_trailers(&mut self) -> Result<HashMap<String, String>> {
		let mut headers = HashMap::new();

		loop {
			let mut key = String::new();
			let mut value = String::new();

			match self.read_trailer(&mut key, &mut value).await? {
				None => break,
				Some(_) => ()
			}

			headers.insert(key, value);
		}

		Ok(headers)
	}

	pub fn remaining(&self) -> Option<u64> {
		match self.transfer {
			Transfer::Empty => Some(0),
			Transfer::Length(remaining) => Some(remaining),
			_ => None
		}
	}
}

impl Read<Context> for Body {
	#[async_trait_fn]
	async fn async_read(&mut self, buf: &mut [u8]) -> Result<usize> {
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

				read_into!(buf, remaining as usize);

				let read = self.read_bytes(buf).await?;

				if unlikely(read == 0) {
					return Err(Self::eof_error());
				}

				remaining -= read as u64;

				self.transfer = if remaining == 0 {
					Transfer::Empty
				} else {
					Transfer::Length(remaining)
				};

				Ok(read)
			}
		}
	}
}

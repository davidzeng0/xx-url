#![allow(unreachable_pub)]

use std::str::{from_utf8, FromStr};

use url::Position;

use super::*;
use crate::net::conn::*;
use crate::tls::conn::TlsConn;

/* maximum allowed Content-Length header if we want to reuse a connection for
 * redirect instead of closing it and opening a new one */
const REDIRECT_REUSE_THRESHOLD: u64 = 4 * 1024;

pub const DEFAULT_MAXIMUM_HEADER_SIZE: u32 = 128 * 1024;

#[derive(Clone)]
pub struct Options {
	/* connect options */
	pub port: u16,
	pub strategy: IpStrategy,
	pub timeout: Option<Duration>,
	pub recvbuf_size: Option<i32>,
	pub sendbuf_size: Option<i32>,
	pub secure: bool,

	/* http options */
	pub min_version: Version,
	pub max_version: Version,
	pub follow_redirect: u32,
	pub maximum_header_size: u32
}

impl Options {
	#[must_use]
	pub const fn new() -> Self {
		Self {
			port: 0,
			strategy: IpStrategy::Default,
			timeout: None,
			recvbuf_size: None,
			sendbuf_size: None,
			secure: false,

			min_version: Version::Http10,
			max_version: Version::Http11,
			follow_redirect: 5,
			maximum_header_size: DEFAULT_MAXIMUM_HEADER_SIZE
		}
	}
}

pub struct Request {
	pub(crate) options: Options,
	pub(crate) request: RequestBase,
	pub(crate) method: Method,
	pub(crate) headers: Headers,
	pub(crate) body: Option<Payload>
}

impl Request {
	pub(crate) fn new(request: RequestBase, method: Method) -> Self {
		Self {
			options: Options::new(),
			request,
			method,
			headers: Headers::new(),
			body: None
		}
	}

	#[allow(clippy::impl_trait_in_params, clippy::needless_pass_by_value)]
	pub fn header(
		&mut self, key: impl TryIntoHeaderName, value: impl TryIntoHeaderValue
	) -> &mut Self {
		if let Err(err) = self.headers.insert(key, value) {
			self.request.fail(err);
		}

		self
	}

	pub fn set_port(&mut self, port: u16) -> &mut Self {
		self.options.port = port;
		self
	}

	pub fn set_strategy(&mut self, strategy: IpStrategy) -> &mut Self {
		self.options.strategy = strategy;
		self
	}

	pub fn set_timeout(&mut self, timeout: Duration) -> &mut Self {
		self.options.timeout = Some(timeout);
		self
	}

	pub fn set_recvbuf_size(&mut self, size: i32) -> &mut Self {
		self.options.recvbuf_size = Some(size);
		self
	}

	pub fn set_sendbuf_size(&mut self, size: i32) -> &mut Self {
		self.options.sendbuf_size = Some(size);
		self
	}

	#[allow(clippy::impl_trait_in_params)]
	pub fn payload(&mut self, payload: impl Into<Payload>) -> &mut Self {
		self.body = Some(payload.into());
		self
	}
}

#[asynchronous]
async fn get_connection_for(
	request: &Request, url: &Url, _connection_pool: /* TOOD */ Option<()>
) -> Result<(HttpConn, Option<Stats>)> {
	let mut options = ConnectOptions::new(
		url.host_str().unwrap(),
		url.port().unwrap_or(request.options.port)
	)
	.await;

	options.set_strategy(request.options.strategy);
	options.set_timeout(request.options.timeout);
	options.set_tcp_nodelay(true);
	options.set_tcp_keepalive(60);

	if let Some(size) = request.options.recvbuf_size {
		options.set_recvbuf_size(size);
	}

	if let Some(size) = request.options.sendbuf_size {
		options.set_sendbuf_size(size);
	}

	if options.port() == 0 {
		let default = if request.options.secure { 443 } else { 80 };

		options.set_port(default);

		debug!(target: request, "== Using default port {}", default);
	}

	let (stream, stats) = if request.options.secure {
		let (conn, stats) = TlsConn::connect_stats(&options).await?;

		(HttpConn::new(conn), stats.into())
	} else {
		let (conn, stats) = Conn::connect_stats(&options).await?;

		(HttpConn::new(conn), stats.into())
	};

	Ok((stream, Some(stats)))
}

#[asynchronous]
#[allow(clippy::impl_trait_in_params)]
async fn send_request(
	writer: &mut BufWriter<impl Write>, request: &Request, version: Version, url: &Url,
	body: &mut Option<Payload>
) -> Result<()> {
	macro_rules! http_write {
		($writer: expr, $($arg: tt)*) => {{
			trace!(target: &*request, "<< {}", format_args!($($arg)*));

			$writer.write_fmt(format_args!("{}\r\n", format_args!($($arg)*)))
		}};
	}

	let path = &url[Position::BeforePath..Position::AfterQuery];

	match version {
		Version::Http09 => http_write!(writer, "{} {}", request.method.as_str(), path).await,

		ver => {
			http_write!(
				writer,
				"{} {} {}",
				request.method.as_str(),
				path,
				ver.as_str()
			)
			.await
		}
	}?;

	for (key, value) in &request.headers {
		trace!(target: request, "<< {}: {}", key.as_str(), value.to_str().unwrap_or("<binary>"));

		writer.write_fmt(format_args!("{}: ", key.as_str())).await?;
		writer.write_all(value.as_bytes()).await?;
		writer.write_string("\r\n").await?;
	}

	writer.write_string("\r\n").await?;

	if let Some(Payload(body)) = body {
		let _ = match body {
			PayloadRepr::Bytes(bytes) => writer.write_all(bytes).await?,
			PayloadRepr::Stream(stream) => writer.pipe_from(stream.as_mut()).await?
		};

		check_interrupt().await?;
	}

	writer.flush().await?;

	Ok(())
}

#[asynchronous]
#[allow(clippy::impl_trait_in_params)]
pub async fn read_line_in_place(reader: &mut impl BufRead) -> Result<(&str, usize)> {
	let mut offset = 0;

	loop {
		let available = reader.buffer();

		#[allow(clippy::arithmetic_side_effects)]
		let (used, done) = match memchr(b'\n', &available[offset..]) {
			Some(index) => (index + 1, true),
			None => (available.len(), false)
		};

		#[allow(clippy::arithmetic_side_effects)]
		(offset += used);

		if done {
			break;
		}

		if reader.fill().await? != 0 {
			continue;
		}

		return if offset == reader.capacity() {
			Err(HttpError::HeadersTooLong.into())
		} else {
			Err(ErrorKind::UnexpectedEof.into())
		};
	}

	let mut line = from_utf8(&(*reader).buffer()[0..offset])?;

	if let Some(ln) = line.strip_suffix('\n') {
		line = ln;
	}

	if let Some(ln) = line.strip_suffix('\r') {
		line = ln;
	}

	Ok((line, offset))
}

pub fn parse_version(version: &str) -> Option<Version> {
	if version.len() != "HTTP/0.0".len() || version.as_bytes()[6] != b'.' {
		return None;
	}

	let major = (version.as_bytes()[5] as char).to_digit(10)?;
	let minor = (version.as_bytes()[7] as char).to_digit(10)?;

	#[allow(clippy::arithmetic_side_effects)]
	Version::from_u32(major * 10 + minor)
}

fn parse_status_line(line: &str) -> Option<(Version, StatusCode)> {
	let mut split = line.split(' ');
	let version = parse_version(split.next()?)?;

	Some((version, StatusCode::from_str(split.next()?).ok()?))
}

#[asynchronous]
#[allow(clippy::impl_trait_in_params)]
pub async fn read_header_line_limited(
	reader: &mut impl BufRead
) -> Result<Option<(HeaderName, Option<HeaderValue>, usize)>> {
	let (line, offset) = read_line_in_place(reader).await?;

	let result = if line.is_empty() {
		None
	} else {
		let (key, value) = match line.split_once(':') {
			Some((key, value)) => (key, Some(value)),
			None => (line, None)
		};

		let key = key.trim().try_into_name()?;
		let value = if let Some(value) = value {
			Some(value.trim_start().try_into_value()?)
		} else {
			None
		};

		Some((key, value, offset))
	};

	reader.consume(offset);

	Ok(result)
}

#[asynchronous]
#[allow(clippy::impl_trait_in_params)]
pub async fn read_headers_limited<T>(
	reader: &mut impl BufRead, headers: &mut Headers, mut size_limit: usize, log: &T
) -> Result<()> {
	loop {
		let (key, value, read) = match read_header_line_limited(reader).await? {
			None => break Ok(()),
			Some(header) => header
		};

		match size_limit.checked_sub(read) {
			Some(new_limit) => size_limit = new_limit,
			None => break Err(HttpError::HeadersTooLong.into())
		}

		let value = value.unwrap_or_else(|| {
			warn!(target: log, "== Header separator not found");

			HeaderValue::from_static("")
		});

		if let Ok(str) = value.to_str() {
			trace!(target: log, ">> {}: {}", key.as_str(), str);
		} else {
			trace!(target: log, ">> {}: {:?}", key.as_str(), value);
		}

		headers.insert(key, value)?;
	}
}

#[asynchronous]
#[allow(clippy::impl_trait_in_params)]
pub async fn parse_response(
	reader: &mut impl BufRead, request: &Request, headers: &mut Headers
) -> Result<(StatusCode, Version)> {
	let mut total_size = 0;

	let prefix_matches = {
		let prefix = b"HTTP/";

		while reader.buffer().len() < prefix.len() {
			if reader.fill().await? == 0 {
				return Err(ErrorKind::UnexpectedEof.into());
			}
		}

		prefix == &reader.buffer()[0..prefix.len()]
	};

	let (version, status) = if !prefix_matches {
		warn!(target: request, "Invalid status line, assuming HTTP 0.9");

		(Version::Http09, StatusCode::OK)
	} else {
		let (line, offset) = read_line_in_place(reader).await?;
		let result =
			parse_status_line(line).ok_or_else(|| HttpError::InvalidStatusLine(line.to_string()));

		#[allow(clippy::arithmetic_side_effects)]
		(total_size += offset);
		reader.consume(offset);

		result?
	};

	trace!(target: request, ">> {} {}", version.as_str(), status);

	if version < request.options.min_version || version > request.options.max_version {
		return Err(HttpError::UnexpectedVersion(version).into());
	}

	if version == Version::Http09 {
		return Ok((status, version));
	}

	match (request.options.maximum_header_size as usize).checked_sub(total_size) {
		Some(limit) => read_headers_limited(reader, headers, limit, request).await?,
		None => return Err(HttpError::HeadersTooLong.into())
	}

	Ok((status, version))
}

pub struct RawResponse {
	pub stats: Stats,
	pub version: Version,
	pub status: StatusCode,
	pub headers: Headers,
	pub url: Option<Url>
}

#[asynchronous]
pub async fn transfer(
	request: &mut Request, connection_pool: Option<()>
) -> Result<(RawResponse, BufReader<HttpConn>)> {
	let version = Version::Http11;
	let req_url = request.request.finalize()?;

	if version <= Version::Http11 && !request.headers.contains_key(header::HOST) {
		request
			.headers
			.insert(header::HOST, req_url.host_str().unwrap())?;
	}

	let req_url = request.request.url().unwrap();

	let mut body = request.body.take();
	let mut url = req_url;

	let mut redirected_url = None;
	let mut redirects_remaining = request.options.follow_redirect;

	let mut response_headers = Headers::new();

	loop {
		debug!(target: &*request, "== Starting request for '{}'", url.as_str());

		response_headers.clear();

		let (conn, stats) = get_connection_for(request, url, connection_pool).await?;
		let mut stats = stats.unwrap_or_default();

		let conn = {
			let mut writer = BufWriter::new(conn);
			let stall = Instant::now();

			send_request(&mut writer, request, version, url, &mut body).await?;

			stats.stall = stall.elapsed();
			writer.into_parts().0
		};

		let (mut response, reader) = {
			let start = Instant::now();
			let mut reader = BufReader::new(conn);

			reader.fill().await?;
			stats.wait = start.elapsed();

			let (status, version) =
				parse_response(&mut reader, request, &mut response_headers).await?;

			stats.response = start.elapsed();

			(
				RawResponse {
					stats,
					version,
					status,
					headers: response_headers,
					url: None
				},
				reader
			)
		};

		if redirects_remaining > 0 && response.status.is_redirection() {
			if let Some(location) = response.headers.get_str(header::LOCATION)? {
				#[allow(clippy::arithmetic_side_effects)]
				(redirects_remaining -= 1);

				let body = Body::new(reader, request, &response)?;

				if body
					.remaining()
					.is_some_and(|len| len < REDIRECT_REUSE_THRESHOLD)
				{
					// TODO store connection for reuse later
				}

				let new_url = url
					.join(location)
					.map_err(|_| UrlError::InvalidRedirectUrl(location.to_string()))?;

				url = redirected_url.insert(new_url);

				if url.scheme() != req_url.scheme() {
					return Err(UrlError::RedirectForbidden(url.scheme().to_string()).into());
				}

				response_headers = response.headers;

				continue;
			}
		}

		request.body = body;
		response.url = redirected_url;

		break Ok((response, reader));
	}
}

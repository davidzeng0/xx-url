use std::{
	collections::HashMap,
	str::{from_utf8, from_utf8_unchecked, FromStr},
	time::{Duration, Instant}
};

use http::{Method, StatusCode};
use memchr::memchr;
use num_traits::FromPrimitive;
use url::{Position, Url};
use xx_core::{async_std::io::*, debug, error::*, trace, warn};
use xx_pulse::*;

use super::{stream::HttpStream, *};
use crate::{net::connection::*, tls::connection::TlsConn};

/* maximum allowed Content-Length header if we want to reuse a connection for
 * redirect instead of closing it and opening a new one */
const REDIRECT_REUSE_THRESHOLD: u64 = 4 * 1024;

#[derive(Clone)]
pub(crate) struct Options {
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
	pub fn new() -> Self {
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
			maximum_header_size: 128 * 1024
		}
	}
}

#[derive(Clone)]
pub struct Request {
	pub(crate) url: Url,
	pub(crate) method: Method,
	pub(crate) headers: HashMap<String, String>,

	pub(crate) options: Options
}

impl Request {
	pub(crate) fn new(url: Url, method: Method) -> Self {
		Self {
			url,
			method,
			headers: HashMap::new(),
			options: Options::new()
		}
	}

	pub fn header(&mut self, key: impl ToString, value: impl ToString) -> &mut Self {
		let mut key = key.to_string();

		key.make_ascii_lowercase();

		self.headers.insert(key, value.to_string());
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
}

struct HttpConnection {
	stream: HttpStream,
	buf: Vec<u8>
}

impl HttpConnection {
	pub fn new(stream: HttpStream) -> Self {
		Self {
			stream,
			buf: Vec::with_capacity(DEFAULT_BUFFER_SIZE)
		}
	}
}

#[async_fn]
async fn get_connection_for(
	request: &Request, url: &Url, _connection_pool: /* TOOD */ Option<()>
) -> Result<(HttpConnection, Option<Stats>)> {
	let mut options = ConnectOptions::new(
		url.host_str().unwrap(),
		url.port().unwrap_or(request.options.port)
	);

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

		(HttpStream::new(conn), stats.into())
	} else {
		let (conn, stats) = Connection::connect_stats(&options).await?;

		(HttpStream::new(conn), stats.into())
	};

	Ok((HttpConnection::new(stream), Some(stats)))
}

#[async_fn]
pub async fn send_request(
	writer: &mut impl Write, request: &Request, url: &Url, version: Version
) -> Result<()> {
	macro_rules! http_write {
		($writer: expr, $($arg: tt)*) => {
			{
				trace!(target: request, "<< {}", format_args!($($arg)*));

				$writer.write_fmt(format_args!("{}\r\n", format_args!($($arg)*)))
			}
		};
	}

	let mut writer = TypedWriter::new(writer.as_ref());
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

	if version <= Version::Http11 {
		if !request.headers.contains_key("host") {
			http_write!(writer, "host: {}", url.host_str().unwrap()).await?;
		}
	}

	for (key, value) in &request.headers {
		http_write!(writer, "{}: {}", key, value).await?;
	}

	writer.write_string("\r\n").await?;
	writer.flush().await?;

	Ok(())
}

#[async_fn]
async fn read_line_in_place(reader: &mut impl BufRead) -> Result<(&str, usize)> {
	let mut offset = 0;

	loop {
		let available = reader.buffer();

		let (used, done) = match memchr(b'\n', &available[offset..]) {
			Some(index) => (index + 1, true),
			None => (available.len(), false)
		};

		offset += used;

		if done {
			break;
		}

		if reader.fill().await? != 0 {
			continue;
		}

		return if offset == reader.capacity() {
			Err(Error::new(
				ErrorKind::Other,
				"Single header line exceeded buffer length"
			))
		} else {
			Err(Error::new(
				ErrorKind::UnexpectedEof,
				"Stream ended on a header line"
			))
		};
	}

	let mut line = from_utf8(&reader.buffer()[0..offset]).map_err(|_| invalid_utf8_error())?;

	if let Some(ln) = line.strip_suffix('\n') {
		line = ln;
	}

	if let Some(ln) = line.strip_suffix('\r') {
		line = ln;
	}

	Ok((line, offset))
}

fn parse_status_line(line: &str) -> Option<(Version, StatusCode)> {
	let mut split = line.split(" ");
	let version = split.next()?;

	if version.len() != "HTTP/0.0".len() || version.as_bytes()[6] != b'.' {
		return None;
	}

	let major = (version.as_bytes()[5] as char).to_digit(10)?;
	let minor = (version.as_bytes()[7] as char).to_digit(10)?;

	Some((
		Version::from_u32(major * 10 + minor)?,
		StatusCode::from_str(split.next()?).ok()?
	))
}

#[async_fn]
pub async fn read_header_line_limited(
	reader: &mut impl BufRead
) -> Result<Option<(String, Option<String>, usize)>> {
	let (line, offset) = read_line_in_place(reader).await?;

	let result = if line.is_empty() {
		None
	} else {
		let (key, value) = match line.split_once(":") {
			Some((key, value)) => (key, Some(value)),
			None => (line, None)
		};

		let key = key.trim().to_ascii_lowercase();
		let value = value.map(|value| value.trim_start().to_string());

		Some((key, value, offset))
	};

	reader.consume(offset);

	Ok(result)
}

#[async_fn]
pub async fn read_headers_limited<T>(
	reader: &mut impl BufRead, headers: &mut HashMap<String, String>, mut size_limit: usize,
	log: &T
) -> Result<()> {
	loop {
		let (key, value, read) = match read_header_line_limited(reader).await? {
			None => break Ok(()),
			Some(header) => header
		};

		match size_limit.checked_sub(read) {
			Some(new_limit) => size_limit = new_limit,
			None => {
				break Err(Error::new(
					ErrorKind::InvalidData,
					"Exceeded maximum header size"
				))
			}
		}

		let value = value.unwrap_or_else(|| {
			warn!(target: log, "== Header separator not found");

			"".to_string()
		});

		trace!(target: log, ">> {}: {}", key, value);

		headers.insert(key, value);
	}
}

#[async_fn]
pub async fn parse_response(
	reader: &mut impl BufRead, request: &Request, headers: &mut HashMap<String, String>
) -> Result<(StatusCode, Version)> {
	let mut total_size = 0;

	let prefix_matches = {
		let prefix = "HTTP/";

		while reader.buffer().len() < prefix.len() {
			if reader.fill().await? != 0 {
				continue;
			}

			return Err(Error::new(
				ErrorKind::UnexpectedEof,
				"End of file before a response could be read"
			));
		}

		/* unchecked because if it's binary, that's still valid for HTTP/0.9 */
		let actual = unsafe { from_utf8_unchecked(&reader.buffer()[0..prefix.len()]) };

		prefix.eq_ignore_ascii_case(actual)
	};

	let (version, status) = if !prefix_matches {
		warn!(target: request, "Invalid header line, assuming HTTP 0.9");

		Some((Version::Http09, StatusCode::OK))
	} else {
		let (line, offset) = read_line_in_place(reader).await?;
		let result = parse_status_line(line);

		total_size += offset;
		reader.consume(offset);
		result
	}
	.ok_or_else(|| Error::new(ErrorKind::InvalidData, "Invalid header line"))?;

	trace!(target: request, ">> {} {}", version.as_str(), status);

	if version < request.options.min_version || version > request.options.max_version {
		return Err(Error::new(
			ErrorKind::InvalidData,
			format!("Unexpected version {}", version.as_str())
		));
	}

	if version == Version::Http09 {
		return Ok((status, version));
	}

	match (request.options.maximum_header_size as usize).checked_sub(total_size) {
		Some(limit) => read_headers_limited(reader, headers, limit, request).await?,
		None => {
			return Err(Error::new(
				ErrorKind::InvalidData,
				"Exceeded maximum header size"
			))
		}
	}

	Ok((status, version))
}

pub struct Response {
	pub stats: Stats,
	pub version: Version,
	pub status: StatusCode,
	pub headers: HashMap<String, String>,
	pub url: Option<Url>
}

#[async_fn]
pub async fn transfer(
	request: &Request, connection_pool: Option<()>
) -> Result<(Response, BufReader<HttpStream>)> {
	let mut url = &request.url;

	let mut redirected_url = None;
	let mut redirects_remaining = request.options.follow_redirect;

	let mut response_headers = HashMap::new();

	loop {
		debug!(target: request, "== Starting request for '{}'", url.as_str());

		let (conn, stats) = get_connection_for(request, url, connection_pool).await?;
		let mut stats = stats.unwrap_or_else(|| Stats::default());

		let (stream, mut buf, _) = {
			let mut writer = BufWriter::from_parts(conn.stream, conn.buf);
			let stall = Instant::now();

			send_request(&mut writer, request, url, Version::Http11).await?;

			stats.stall = stall.elapsed();
			writer.into_parts()
		};

		buf.clear();
		response_headers.clear();

		let (response, reader) = {
			let start = Instant::now();
			let mut reader = BufReader::from_parts(stream, buf, 0);

			reader.fill().await?;
			stats.wait = start.elapsed();

			let (status, version) =
				parse_response(&mut reader, request, &mut response_headers).await?;

			stats.response = start.elapsed();

			(
				Response {
					stats,
					version,
					status,
					headers: response_headers,
					url: redirected_url
				},
				reader
			)
		};

		if redirects_remaining > 0 && response.status.is_redirection() {
			if let Some(location) = response.headers.get("location") {
				redirects_remaining -= 1;

				let body = Body::new(reader, request, &response)?;

				if body
					.remaining()
					.is_some_and(|len| len < REDIRECT_REUSE_THRESHOLD)
				{
					// TODO store connection for reuse later
				}

				redirected_url = response.url;

				/* unborrow to keep compiler happy */
				url = &request.url;
				url = redirected_url.insert(
					url.join(location)
						.map_err(|_| Error::new(ErrorKind::InvalidData, "Invalid redirect url"))?
				);

				if url.scheme() != request.url.scheme() {
					return Err(Error::new(
						ErrorKind::Other,
						"Redirect forbidden due to change in url scheme"
					));
				}

				response_headers = response.headers;

				continue;
			}
		}

		break Ok((response, reader));
	}
}

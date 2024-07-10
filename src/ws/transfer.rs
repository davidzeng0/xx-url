use xx_core::trace;

use super::*;
use crate::http::transfer::*;

macro_rules! check_header {
	($headers:expr, $header:literal, $value:expr, $error:expr) => {
		if !$headers
			.get_str($header)?
			.is_some_and(|val| val.eq_ignore_ascii_case($value))
		{
			return Err($error);
		}
	};
}

#[asynchronous]
pub async fn connect(request: &mut WsRequest) -> Result<BufReader<HttpConn>> {
	let mut key_bytes = [0u8; 24];
	let mut accept_bytes = [0u8; 28];

	let (key, accept) = {
		let rand = Key::new();

		rand.encode(&mut key_bytes);
		rand.accept(&mut accept_bytes);

		(
			from_utf8(&key_bytes).unwrap(),
			from_utf8(&accept_bytes).unwrap()
		)
	};

	let timeout = request.options.handshake_timeout;

	if let Some(connect_timeout) = &mut request.inner.options.timeout {
		*connect_timeout = (*connect_timeout).min(timeout);
	}

	request.header("Connection", "Upgrade");
	request.header("Upgrade", "websocket");
	request.header("Sec-WebSocket-Version", WEB_SOCKET_VERSION);
	request.header("Sec-WebSocket-Key", key);

	let (response, reader) = transfer(&mut request.inner, None)
		.timeout(timeout)
		.await
		.ok_or(WebSocketError::HandshakeTimeout)??;

	if response.status != StatusCode::SWITCHING_PROTOCOLS {
		return Err(WebSocketError::ServerRejected.into());
	}

	check_header!(
		response.headers,
		"Connection",
		"Upgrade",
		WebSocketError::ServerRejected.into()
	);

	check_header!(
		response.headers,
		"Upgrade",
		"websocket",
		WebSocketError::ServerRejected.into()
	);

	check_header!(
		response.headers,
		"Sec-WebSocket-Accept",
		accept,
		WebSocketError::ServerRejected.into()
	);

	Ok(reader)
}

fn parse_request_line(line: &str) -> Option<(Version, String)> {
	let mut split = line.split(' ');
	let method = split.next()?;

	if method != Method::GET {
		return None;
	}

	let url = split.next()?.to_string();

	Some((parse_version(split.next()?)?, url))
}

#[asynchronous]
async fn handle_request<T>(reader: &mut impl BufRead, log: &T) -> Result<Headers> {
	let mut total_size = 0;

	let (line, offset) = read_line_in_place(reader).await?;
	let (version, url) =
		parse_request_line(line).ok_or_else(|| HttpError::InvalidStatusLine(line.to_string()))?;

	#[allow(clippy::arithmetic_side_effects)]
	(total_size += offset);
	reader.consume(offset);

	trace!(target: log, ">> GET {} {}", url, version.as_str());

	if version != Version::Http11 {
		return Err(HttpError::UnexpectedVersion(version).into());
	}

	let mut headers = Headers::new();

	match (DEFAULT_MAXIMUM_HEADER_SIZE as usize).checked_sub(total_size) {
		Some(limit) => read_headers_limited(reader, &mut headers, limit, log).await?,
		None => return Err(HttpError::HeadersTooLong.into())
	}

	Ok(headers)
}

#[asynchronous]
pub async fn handle_upgrade<T>(stream: HttpConn, log: &T) -> Result<BufReader<HttpConn>> {
	let mut reader = BufReader::new(stream);
	let headers = handle_request(&mut reader, log).await?;
	let (stream, buf, pos) = reader.into_parts();
	let mut writer = BufWriter::new(stream);

	check_header!(
		headers,
		"Connection",
		"Upgrade",
		WebSocketError::InvalidClientRequest.into()
	);

	check_header!(
		headers,
		"Upgrade",
		"websocket",
		WebSocketError::InvalidClientRequest.into()
	);

	check_header!(
		headers,
		"Sec-WebSocket-Version",
		WEB_SOCKET_VERSION,
		WebSocketError::InvalidClientRequest.into()
	);

	let key = match headers.get_str("Sec-WebSocket-Key")? {
		Some(key) => key,
		None => return Err(WebSocketError::InvalidClientRequest.into())
	};

	let mut accept_bytes = [0u8; 28];

	Key::from(key)?.accept(&mut accept_bytes);

	macro_rules! http_write {
		($writer: expr, $($arg: tt)*) => {{
			trace!(target: log, "<< {}", format_args!($($arg)*));

			$writer.write_fmt(format_args!("{}\r\n", format_args!($($arg)*)))
		}};
	}

	http_write!(
		writer,
		"{} {} {}",
		Version::Http11.as_str(),
		StatusCode::SWITCHING_PROTOCOLS.as_u16(),
		StatusCode::SWITCHING_PROTOCOLS.canonical_reason().unwrap()
	)
	.await?;

	http_write!(writer, "Connection: Upgrade").await?;
	http_write!(writer, "Upgrade: websocket").await?;
	http_write!(
		writer,
		"Sec-WebSocket-Accept: {}",
		from_utf8(&accept_bytes).unwrap()
	)
	.await?;

	writer.write_string("\r\n").await?;
	writer.flush().await?;

	let (stream, ..) = writer.into_parts();

	Ok(BufReader::from_parts(stream, buf, pos))
}

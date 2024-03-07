use std::{collections::HashMap, str::from_utf8};

use xx_core::trace;

use super::*;
use crate::http::transfer::*;

macro_rules! check_header {
	($headers: expr, $header: literal, $value: expr, $error: expr) => {
		if !$headers
			.get($header)
			.is_some_and(|val| val.eq_ignore_ascii_case($value))
		{
			return Err($error);
		}
	};
}

#[asynchronous]
pub async fn connect(request: &WsRequest) -> Result<BufReader<HttpStream>> {
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

	let mut request = request.inner.clone();

	if let Some(connect_timeout) = &mut request.options.timeout {
		*connect_timeout = (*connect_timeout).min(timeout);
	}

	request.header("Connection", "Upgrade");
	request.header("Upgrade", "websocket");
	request.header("Sec-WebSocket-Version", WEB_SOCKET_VERSION);
	request.header("Sec-WebSocket-Key", key);

	let (response, reader) = transfer(&request, None)
		.timeout(timeout)
		.await
		.ok_or_else(|| WebSocketError::HandshakeTimeout.as_err())??;

	if response.status != StatusCode::SWITCHING_PROTOCOLS {
		return Err(WebSocketError::ServerRejected.as_err());
	}

	check_header!(
		response.headers,
		"connection",
		"upgrade",
		WebSocketError::ServerRejected.as_err()
	);

	check_header!(
		response.headers,
		"upgrade",
		"websocket",
		WebSocketError::ServerRejected.as_err()
	);

	check_header!(
		response.headers,
		"sec-websocket-accept",
		accept,
		WebSocketError::ServerRejected.as_err()
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
async fn handle_request<T>(reader: &mut impl BufRead, log: &T) -> Result<HashMap<String, String>> {
	let mut total_size = 0;

	let (line, offset) = read_line_in_place(reader).await?;
	let (version, url) = parse_request_line(line).ok_or_else(|| HttpError::InvalidStatusLine)?;

	total_size += offset;
	reader.consume(offset);

	trace!(target: log, ">> GET {} {}", url, version.as_str());

	if version != Version::Http11 {
		return Err(Error::simple(
			ErrorKind::InvalidData,
			Some(format!("Unexpected version {}", version.as_str()))
		));
	}

	let mut headers = HashMap::new();

	match (DEFAULT_MAXIMUM_HEADER_SIZE as usize).checked_sub(total_size) {
		Some(limit) => read_headers_limited(reader, &mut headers, limit, log).await?,
		None => return Err(HttpError::HeadersTooLong.as_err())
	}

	Ok(headers)
}

#[asynchronous]
pub async fn handle_upgrade<T>(mut stream: HttpStream, log: &T) -> Result<BufReader<HttpStream>> {
	let (read, write) = stream.split();
	let mut reader = BufReader::new(read);
	let mut writer = BufWriter::new(write);

	let headers = handle_request(&mut reader, log).await?;

	check_header!(
		headers,
		"connection",
		"upgrade",
		WebSocketError::InvalidClientRequest.as_err()
	);

	check_header!(
		headers,
		"upgrade",
		"websocket",
		WebSocketError::InvalidClientRequest.as_err()
	);

	check_header!(
		headers,
		"sec-websocket-version",
		WEB_SOCKET_VERSION,
		WebSocketError::InvalidClientRequest.as_err()
	);

	let key = match headers.get("sec-websocket-key") {
		Some(key) => key,
		None => return Err(WebSocketError::InvalidClientRequest.as_err())
	};

	let mut accept_bytes = [0u8; 28];

	Key::from(key)?.accept(&mut accept_bytes);

	macro_rules! http_write {
		($writer: expr, $($arg: tt)*) => {
			{
				trace!(target: log, "<< {}", format_args!($($arg)*));

				$writer.write_fmt(format_args!("{}\r\n", format_args!($($arg)*)))
			}
		};
	}

	http_write!(
		writer,
		"{} {} {}",
		Version::Http11.as_str(),
		StatusCode::SWITCHING_PROTOCOLS.as_u16(),
		StatusCode::SWITCHING_PROTOCOLS.canonical_reason().unwrap()
	)
	.await?;

	http_write!(writer, "connection: Upgrade").await?;
	http_write!(writer, "upgrade: websocket").await?;
	http_write!(
		writer,
		"sec-websocket-accept: {}",
		from_utf8(&accept_bytes).unwrap()
	)
	.await?;

	writer.write_string("\r\n").await?;
	writer.flush().await?;

	let (_, buf, pos) = reader.into_parts();

	Ok(BufReader::from_parts(stream, buf, pos))
}

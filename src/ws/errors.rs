use super::*;

#[errors]
pub enum WebSocketError {
	#[error("WebSocket server rejected connection")]
	ServerRejected,

	#[error("Invalid headers set on incoming client")]
	#[kind = ErrorKind::InvalidData]
	InvalidClientRequest,

	#[error("Handshake timed out")]
	#[kind = ErrorKind::TimedOut]
	HandshakeTimeout,

	#[error("Invalid WebSocket key")]
	#[kind = ErrorKind::InvalidData]
	InvalidKey,

	#[error("Received an invalid opcode")]
	#[kind = ErrorKind::InvalidData]
	InvalidOpcode,

	#[error(transparent)]
	#[kind = ErrorKind::InvalidData]
	InvalidControlFrame(&'static str),

	#[error("Expected a continuation frame")]
	#[kind = ErrorKind::InvalidData]
	ExpectedContinuation,

	#[error("Unexpected continuation frame")]
	#[kind = ErrorKind::InvalidData]
	UnexpectedContinuation,

	#[error("Received masked frame from server")]
	#[kind = ErrorKind::InvalidData]
	ServerMasked,

	#[error("Maximum message length exceeded")]
	#[kind = ErrorKind::InvalidData]
	MessageTooLong,

	#[error("Control frame too large")]
	#[kind = ErrorKind::InvalidInput]
	UserInvalidControlFrame,

	#[error("Cannot send mismatching data types in chunks")]
	#[kind = ErrorKind::InvalidInput]
	DataTypeMismatch
}

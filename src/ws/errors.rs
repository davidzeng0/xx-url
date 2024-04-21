use super::*;

#[errors]
pub enum WebSocketError {
	#[error("WebSocket server rejected connection")]
	ServerRejected,

	#[error("Invalid headers set on incoming client")]
	InvalidClientRequest,

	#[error("Handshake timed out")]
	HandshakeTimeout,

	#[error("Invalid WebSocket key")]
	InvalidKey,

	#[error("Received an invalid opcode")]
	InvalidOpcode,

	#[error("{0}")]
	InvalidControlFrame(&'static str),

	#[error("Expected a continuation frame")]
	ExpectedContinuation,

	#[error("Unexpected continuation frame")]
	UnexpectedContinuation,

	#[error("Received masked frame from server")]
	ServerMasked,

	#[error("Maximum message length exceeded")]
	MessageTooLong,

	#[error("Control frame too large")]
	UserInvalidControlFrame,

	#[error("Cannot send mismatching data types in chunks")]
	DataTypeMismatch
}

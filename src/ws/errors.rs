use super::*;

#[errors]
pub enum WebSocketError {
	#[display("WebSocket server rejected connection")]
	ServerRejected,

	#[display("Invalid headers set on incoming client")]
	#[kind = ErrorKind::InvalidData]
	InvalidClientRequest,

	#[display("Handshake timed out")]
	#[kind = ErrorKind::TimedOut]
	HandshakeTimeout,

	#[display("Invalid WebSocket key")]
	#[kind = ErrorKind::InvalidData]
	InvalidKey,

	#[display("Received an invalid opcode")]
	#[kind = ErrorKind::InvalidData]
	InvalidOpcode,

	#[display(transparent)]
	#[kind = ErrorKind::InvalidData]
	InvalidControlFrame(&'static str),

	#[display("Expected a continuation frame")]
	#[kind = ErrorKind::InvalidData]
	ExpectedContinuation,

	#[display("Unexpected continuation frame")]
	#[kind = ErrorKind::InvalidData]
	UnexpectedContinuation,

	#[display("Received masked frame from server")]
	#[kind = ErrorKind::InvalidData]
	ServerMasked,

	#[display("Maximum message length exceeded")]
	#[kind = ErrorKind::InvalidData]
	MessageTooLong,

	#[display("Control frame too large")]
	#[kind = ErrorKind::InvalidInput]
	UserInvalidControlFrame,

	#[display("Cannot send mismatching data types in chunks")]
	#[kind = ErrorKind::InvalidInput]
	DataTypeMismatch
}

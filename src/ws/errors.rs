use super::*;

#[compact_error]
pub enum WebSocketError {
	ServerRejected          = (
		ErrorKind::ConnectionRefused,
		"WebSocket server rejected connection"
	),
	InvalidClientRequest    = (
		ErrorKind::InvalidData,
		"Invalid headers set on incoming client"
	),
	HandshakeTimeout        = (ErrorKind::TimedOut, "Handshake timed out"),
	InvalidKey              = (ErrorKind::InvalidData, "Invalid WebSocket key"),

	InvalidOpcode           = (ErrorKind::InvalidData, "Received an invalid opcode"),
	InvalidControlFrame     = (ErrorKind::InvalidData, "Received an invalid control frame"),
	ExpectedContinuation    = (ErrorKind::InvalidData, "Expected a continuation frame"),
	UnexpectedContinuation  = (ErrorKind::InvalidData, "Unexpected continuation frame"),
	ServerMasked            = (ErrorKind::InvalidData, "Received masked frame from server"),
	MessageTooLong          = (ErrorKind::Other, "Maximum message length exceeded"),

	UserInvalidControlFrame = (ErrorKind::InvalidInput, "Control frame too large"),
	DataTypeMismatch        = (
		ErrorKind::InvalidInput,
		"Cannot send mismatching data types in chunks"
	)
}

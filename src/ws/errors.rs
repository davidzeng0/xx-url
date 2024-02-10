use xx_core::error::*;

#[compact_error]
pub enum WebSocketError {
	InvalidKey             = (ErrorKind::InvalidData, "Invalid WebSocket key"),
	InvalidOpcode          = (ErrorKind::InvalidData, "Invalid opcode"),
	ExpectedContinuation   = (ErrorKind::InvalidData, "Expected a continuation frame"),
	UnexpectedContinuation = (ErrorKind::InvalidData, "Unexpected continuation frame"),
	ServerMasked           = (ErrorKind::InvalidData, "Received masked frame from server"),
	MessageTooLong         = (ErrorKind::Other, "Maximum message length exceeded"),
	DataTypeMismatch       = (
		ErrorKind::InvalidInput,
		"Cannot send mismatching data types in chunks"
	)
}

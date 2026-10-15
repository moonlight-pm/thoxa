//! Content-Length framed JSON-RPC over byte streams (LSP base protocol).

use serde_json::Value;
use std::io::{self, BufRead, Read, Write};

/// Largest message body accepted. A larger `Content-Length` is skipped (not
/// allocated), so a bad header cannot exhaust memory.
pub const MAX_MESSAGE_BYTES: u64 = 64 * 1024 * 1024;

/// One read from the client.
pub enum Incoming {
    Message(Value),
    /// Framing was intact but the body was not a JSON-RPC message, or it was
    /// over [`MAX_MESSAGE_BYTES`]. The server answers with a parse error and
    /// keeps reading.
    Malformed(String),
    Eof,
}

/// Write one LSP message (headers + JSON body).
pub fn write_message<W: Write>(stream: &mut W, message: &Value) -> io::Result<()> {
    let body =
        serde_json::to_vec(message).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write!(stream, "Content-Length: {}\r\n\r\n", body.len())?;
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}

/// Read one LSP message, or `None` at end-of-stream.
pub fn read_message<R: BufRead>(stream: &mut R) -> io::Result<Option<Value>> {
    match read_incoming(stream)? {
        Incoming::Message(value) => Ok(Some(value)),
        Incoming::Eof => Ok(None),
        Incoming::Malformed(reason) => Err(io::Error::new(io::ErrorKind::InvalidData, reason)),
    }
}

/// Read one message. Only a broken stream is an `Err`; a bad body is
/// [`Incoming::Malformed`] so the server can reply and continue.
pub fn read_incoming<R: BufRead>(stream: &mut R) -> io::Result<Incoming> {
    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        let mut line = String::new();
        let n = stream.read_line(&mut line)?;
        if n == 0 {
            return Ok(Incoming::Eof);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            headers.push((key.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }

    let length: u64 = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);

    if length == 0 {
        return Ok(Incoming::Message(Value::Object(serde_json::Map::new())));
    }
    if length > MAX_MESSAGE_BYTES {
        // Skip the body without buffering it; a stream that ends first is EOF.
        let skipped = io::copy(&mut stream.take(length), &mut io::sink())?;
        if skipped < length {
            return Ok(Incoming::Eof);
        }
        return Ok(Incoming::Malformed(format!(
            "message of {length} bytes is over the {MAX_MESSAGE_BYTES}-byte limit"
        )));
    }

    let mut body = vec![0u8; length as usize];
    match stream.read_exact(&mut body) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(Incoming::Eof),
        Err(error) => return Err(error),
    }
    match serde_json::from_slice::<Value>(&body) {
        Ok(value) if value.is_object() => Ok(Incoming::Message(value)),
        Ok(_) => Ok(Incoming::Malformed("message is not a JSON object".into())),
        Err(error) => Ok(Incoming::Malformed(format!("invalid JSON: {error}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    #[test]
    fn write_read_roundtrip() {
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });
        let mut buf = Vec::new();
        write_message(&mut buf, &msg).unwrap();
        assert!(buf.starts_with(b"Content-Length:"));
        assert!(buf.windows(4).any(|w| w == b"\r\n\r\n"));

        let mut cursor = Cursor::new(buf);
        let got = read_message(&mut cursor).unwrap().unwrap();
        assert_eq!(got["method"], "initialize");
        assert_eq!(got["id"], 1);
    }

    #[test]
    fn malformed_body_is_reported_and_reading_continues() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"Content-Length: 5\r\n\r\n{nope");
        write_message(&mut buf, &json!({"id": 7})).unwrap();
        let mut cursor = Cursor::new(buf);
        assert!(matches!(
            read_incoming(&mut cursor).unwrap(),
            Incoming::Malformed(_)
        ));
        match read_incoming(&mut cursor).unwrap() {
            Incoming::Message(value) => assert_eq!(value["id"], 7),
            _ => panic!("next message should still parse"),
        }
    }

    #[test]
    fn huge_content_length_is_not_allocated() {
        let buf = b"Content-Length: 9223372036854775807\r\n\r\n{}".to_vec();
        let mut cursor = Cursor::new(buf);
        assert!(matches!(read_incoming(&mut cursor).unwrap(), Incoming::Eof));
    }

    #[test]
    fn eof_returns_none() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        assert!(read_message(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn multiple_messages() {
        let mut buf = Vec::new();
        write_message(&mut buf, &json!({"id": 1})).unwrap();
        write_message(&mut buf, &json!({"id": 2})).unwrap();
        let mut cursor = Cursor::new(buf);
        assert_eq!(read_message(&mut cursor).unwrap().unwrap()["id"], 1);
        assert_eq!(read_message(&mut cursor).unwrap().unwrap()["id"], 2);
        assert!(read_message(&mut cursor).unwrap().is_none());
    }
}

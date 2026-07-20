//! Content-Length framed JSON-RPC over byte streams (LSP base protocol).

use serde_json::Value;
use std::io::{self, BufRead, Write};

/// Write one LSP message (headers + JSON body).
pub fn write_message<W: Write>(stream: &mut W, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, e)
    })?;
    write!(stream, "Content-Length: {}\r\n\r\n", body.len())?;
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}

/// Read one LSP message, or `None` at end-of-stream.
pub fn read_message<R: BufRead>(stream: &mut R) -> io::Result<Option<Value>> {
    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        let mut line = String::new();
        let n = stream.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            headers.push((key.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }

    let length: usize = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);

    if length == 0 {
        return Ok(Some(Value::Object(serde_json::Map::new())));
    }

    let mut body = vec![0u8; length];
    stream.read_exact(&mut body)?;
    let value: Value = serde_json::from_slice(&body).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, e)
    })?;
    Ok(Some(value))
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

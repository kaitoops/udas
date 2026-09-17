//! Length-delimited frame codec for the embedding service protocol.
//!
//! Frame format: 4-byte big-endian unsigned length + JSON payload (UTF-8).
//! Maximum payload size: [`MAX_FRAME_SIZE`] (32 MB).
//!
//! This module provides async functions for reading/writing frames over
//! any `AsyncRead + AsyncWrite + Unpin` stream, without requiring
//! `tokio_util::codec`.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::protocol::{MAX_FRAME_SIZE, Request, Response};

const HEADER_LEN: usize = 4;

/// Read a single length-delimited frame from an async reader.
///
/// Returns the payload bytes (without the 4-byte header).
///
/// # Errors
/// - `UnexpectedEof` if the stream closes before a complete frame is read.
/// - `InvalidData` if the frame exceeds `MAX_FRAME_SIZE`.
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut header = [0u8; HEADER_LEN];
    reader.read_exact(&mut header).await?;
    let len = u32::from_be_bytes(header) as usize;

    if len == 0 {
        return Ok(Vec::new());
    }

    if len > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame payload too large: {len} bytes (max {MAX_FRAME_SIZE})"),
        ));
    }

    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).await?;
    Ok(payload)
}

/// Write a single length-delimited frame to an async writer.
///
/// Prepends a 4-byte big-endian length header before the payload,
/// then flushes.
///
/// # Errors
/// - `InvalidData` if the payload exceeds `MAX_FRAME_SIZE`.
pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, payload: &[u8]) -> io::Result<()> {
    let len = payload.len();
    if len > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame payload too large: {len} bytes (max {MAX_FRAME_SIZE})"),
        ));
    }

    let header = (len as u32).to_be_bytes();
    writer.write_all(&header).await?;
    if len > 0 {
        writer.write_all(payload).await?;
    }
    writer.flush().await?;
    Ok(())
}

/// Read a JSON-RPC request from a framed stream.
///
/// Convenience wrapper: reads a frame, then deserializes as [`Request`].
pub async fn read_request<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Request> {
    let payload = read_frame(reader).await?;
    if payload.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "received empty frame",
        ));
    }
    serde_json::from_slice(&payload).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to parse request JSON: {e}"),
        )
    })
}

/// Write a JSON-RPC response to a framed stream.
///
/// Convenience wrapper: serializes the response to JSON, then writes a frame.
pub async fn write_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    response: &Response,
) -> io::Result<()> {
    let payload = serde_json::to_vec(response).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to serialize response: {e}"),
        )
    })?;
    write_frame(writer, &payload).await
}

/// Write a JSON-RPC request to a framed stream.
pub async fn write_request<W: AsyncWrite + Unpin>(
    writer: &mut W,
    request: &Request,
) -> io::Result<()> {
    let payload = serde_json::to_vec(request).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to serialize request: {e}"),
        )
    })?;
    write_frame(writer, &payload).await
}

/// Read a JSON-RPC response from a framed stream.
pub async fn read_response<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Response> {
    let payload = read_frame(reader).await?;
    if payload.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "received empty frame",
        ));
    }
    serde_json::from_slice(&payload).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to parse response JSON: {e}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{RequestId, ResponseResult};
    use tokio::io::duplex;

    #[tokio::test]
    async fn test_frame_roundtrip() {
        let (mut client, mut server) = duplex(1024);

        let payload = b"hello world";
        write_frame(&mut client, payload).await.unwrap();

        let received = read_frame(&mut server).await.unwrap();
        assert_eq!(received, payload);
    }

    #[tokio::test]
    async fn test_empty_frame() {
        let (mut client, mut server) = duplex(64);
        write_frame(&mut client, b"").await.unwrap();
        let received = read_frame(&mut server).await.unwrap();
        assert!(received.is_empty());
    }

    #[tokio::test]
    async fn test_large_frame_rejected() {
        let (mut client, _server) = duplex(1024);
        let huge = vec![0u8; MAX_FRAME_SIZE + 1];
        let result = write_frame(&mut client, &huge).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_request_response_roundtrip() {
        let (mut client, mut server) = duplex(4096);

        // Client sends request
        let req = Request::embed(1, "test text");
        write_request(&mut client, &req).await.unwrap();

        // Server reads request
        let received_req = read_request(&mut server).await.unwrap();
        assert_eq!(received_req.method, "embed");
        assert_eq!(received_req.id, RequestId::Int(1));

        // Server sends response
        let resp = Response::success(
            received_req.id,
            ResponseResult::Embed {
                embedding: vec![0.1, 0.2, 0.3],
            },
        );
        write_response(&mut server, &resp).await.unwrap();

        // Client reads response
        let received_resp = read_response(&mut client).await.unwrap();
        assert!(received_resp.result.is_some());
        assert!(!received_resp.is_error());
    }

    #[tokio::test]
    async fn test_partial_frame_eof() {
        // When the header indicates more bytes than available and the
        // stream closes, read_frame should return an error.
        let (mut client, mut server) = duplex(1024);

        // Write header only (length = 10)
        client.write_all(&10u32.to_be_bytes()).await.unwrap();
        client.write_all(b"hello").await.unwrap(); // only 5 of 10 bytes
        drop(client); // close write side

        let result = read_frame(&mut server).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_multiple_frames_in_sequence() {
        let (mut client, mut server) = duplex(8192);

        // Write three frames
        write_frame(&mut client, b"first").await.unwrap();
        write_frame(&mut client, b"second").await.unwrap();
        write_frame(&mut client, b"third").await.unwrap();

        // Read them back in order
        assert_eq!(read_frame(&mut server).await.unwrap(), b"first");
        assert_eq!(read_frame(&mut server).await.unwrap(), b"second");
        assert_eq!(read_frame(&mut server).await.unwrap(), b"third");
    }

    #[tokio::test]
    async fn test_ping_roundtrip() {
        let (mut client, mut server) = duplex(1024);

        let req = Request::ping(1);
        write_request(&mut client, &req).await.unwrap();

        let received = read_request(&mut server).await.unwrap();
        assert_eq!(received.method, "ping");

        let resp = Response::success(
            RequestId::Int(1),
            ResponseResult::Ping {
                pong: true,
                uptime_secs: 42,
            },
        );
        write_response(&mut server, &resp).await.unwrap();

        let result = read_response(&mut client).await.unwrap();
        assert!(!result.is_error());
    }
}

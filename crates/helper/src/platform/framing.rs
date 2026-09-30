//! Bounded length-prefixed JSON framing, shared with Unix transport tests.
use crate::{HelperError, Result, protocol::MAX_FRAME_BYTES};
use serde::Serialize;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
pub(super) async fn read_body<R: AsyncRead + Unpin>(
    reader: &mut R,
    length: usize,
    deadline: Duration,
) -> Result<Vec<u8>> {
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(HelperError::Protocol(
            "Frame length must be 1 byte through 1 MiB".into(),
        ));
    }
    let mut bytes = vec![0; length];
    tokio::time::timeout(deadline, reader.read_exact(&mut bytes))
        .await
        .map_err(|_| HelperError::Protocol("Frame body timed out".into()))?
        .map_err(|e| HelperError::transport("read frame body", e))?;
    Ok(bytes)
}
pub(super) async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    value: &T,
    deadline: Duration,
) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|e| HelperError::Protocol(e.to_string()))?;
    if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES {
        return Err(HelperError::Protocol("Response exceeds 1 MiB".into()));
    }
    tokio::time::timeout(deadline, async {
        writer.write_u32(bytes.len() as u32).await?;
        writer.write_all(&bytes).await?;
        writer.flush().await
    })
    .await
    .map_err(|_| HelperError::Protocol("Frame write timed out".into()))?
    .map_err(|e| HelperError::transport("write frame", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    const DEADLINE: Duration = Duration::from_millis(20);

    #[tokio::test]
    async fn validates_lengths_before_reading_or_allocating() {
        let (_writer, mut reader) = tokio::io::duplex(8);
        for size in [0, MAX_FRAME_BYTES + 1, usize::MAX] {
            assert!(matches!(
                read_body(&mut reader, size, DEADLINE).await,
                Err(HelperError::Protocol(_))
            ));
        }
    }
    #[tokio::test]
    async fn truncated_and_stalled_frames_fail() {
        let (mut writer, mut reader) = tokio::io::duplex(8);
        writer.write_all(b"abc").await.unwrap();
        drop(writer);
        assert!(read_body(&mut reader, 4, DEADLINE).await.is_err());
        let (_writer, mut reader) = tokio::io::duplex(8);
        assert!(matches!(
            read_body(&mut reader, 1, DEADLINE).await,
            Err(HelperError::Protocol(_))
        ));
    }
    #[tokio::test]
    async fn framing_round_trip_preserves_payload() {
        let (mut writer, mut reader) = tokio::io::duplex(128);
        let value = serde_json::json!({"kind":"status"});
        write_frame(&mut writer, &value, DEADLINE).await.unwrap();
        let size = reader.read_u32().await.unwrap() as usize;
        let decoded: serde_json::Value =
            serde_json::from_slice(&read_body(&mut reader, size, DEADLINE).await.unwrap()).unwrap();
        assert_eq!(value, decoded);
    }
    #[tokio::test]
    async fn broken_and_backpressured_writers_fail_within_deadline() {
        let (mut writer, reader) = tokio::io::duplex(1);
        drop(reader);
        assert!(write_frame(&mut writer, &true, DEADLINE).await.is_err());
        let (mut writer, _reader) = tokio::io::duplex(1);
        assert!(matches!(
            write_frame(&mut writer, &true, DEADLINE).await,
            Err(HelperError::Protocol(_))
        ));
    }
}

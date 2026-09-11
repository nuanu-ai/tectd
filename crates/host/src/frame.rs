use std::io;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

pub(crate) const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

pub(crate) enum Frame {
    Data(Vec<u8>),
    TooLarge,
}

pub(crate) struct FrameReader<R> {
    inner: BufReader<R>,
    maximum: usize,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    pub(crate) fn new(reader: R) -> Self {
        Self {
            inner: BufReader::new(reader),
            maximum: MAX_FRAME_BYTES,
        }
    }

    pub(crate) async fn next(&mut self) -> io::Result<Option<Frame>> {
        let mut output = Vec::new();
        let mut too_large = false;

        loop {
            let available = self.inner.fill_buf().await?;
            if available.is_empty() {
                if output.is_empty() && !too_large {
                    return Ok(None);
                }
                return Ok(Some(if too_large {
                    Frame::TooLarge
                } else {
                    Frame::Data(output)
                }));
            }

            let newline = available.iter().position(|byte| *byte == b'\n');
            let payload_len = newline.unwrap_or(available.len());
            if !too_large {
                if output.len().saturating_add(payload_len) > self.maximum {
                    output.clear();
                    too_large = true;
                } else {
                    output.extend_from_slice(&available[..payload_len]);
                }
            }

            let consumed = payload_len + usize::from(newline.is_some());
            self.inner.consume(consumed);
            if newline.is_some() {
                if too_large {
                    return Ok(Some(Frame::TooLarge));
                }
                if output.last() == Some(&b'\r') {
                    output.pop();
                }
                return Ok(Some(Frame::Data(output)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn oversized_frame_is_drained_without_unbounded_storage() {
        let bytes = vec![b'x'; MAX_FRAME_BYTES + 1];
        let mut input = bytes;
        input.extend_from_slice(b"\nnext\n");
        let mut reader = FrameReader::new(input.as_slice());
        assert!(matches!(
            reader.next().await.unwrap(),
            Some(Frame::TooLarge)
        ));
        match reader.next().await.unwrap() {
            Some(Frame::Data(value)) => assert_eq!(value, b"next"),
            _ => panic!("expected the frame following the oversized input"),
        }
    }
}

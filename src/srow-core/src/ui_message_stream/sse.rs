// INPUT:  bytes, futures, serde_json, crate::error::StreamError, super::UIMessageChunk
// OUTPUT: parse_sse_stream, chunk_to_sse, sse_done
// POS:    SSE (Server-Sent Events) wire-format parser and serializer for UIMessageChunk streams.

use bytes::Bytes;
use futures::stream::unfold;
use futures::{Stream, StreamExt};

use crate::error::StreamError;
use super::UIMessageChunk;

/// Try to extract a data payload from an SSE event block (text between `\n\n` separators).
/// Returns `None` if the block contains no data lines (e.g. only comments or empty).
fn extract_data_from_event(event_block: &str) -> Option<String> {
    let mut data_parts: Vec<&str> = Vec::new();
    for line in event_block.lines() {
        if line.starts_with(':') {
            // Comment line, skip
            continue;
        }
        if let Some(rest) = line.strip_prefix("data: ") {
            data_parts.push(rest);
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_parts.push(rest);
        }
        // Other SSE fields (event:, id:, retry:) are ignored for our purposes
    }
    if data_parts.is_empty() {
        None
    } else {
        Some(data_parts.join("\n"))
    }
}

/// Parse an SSE byte stream into UIMessageChunk items.
///
/// Format: `"data: {json}\n\n"` per event, terminated by `"data: [DONE]\n\n"`.
/// Lines starting with `:` are treated as comments and ignored.
/// Empty lines within an event block are skipped.
pub fn parse_sse_stream(
    byte_stream: impl Stream<Item = Result<Bytes, StreamError>> + Send + 'static,
) -> impl Stream<Item = Result<UIMessageChunk, StreamError>> + Send {
    // State: (byte_stream, text_buffer, queue of parsed chunks ready to emit)
    unfold(
        (
            Box::pin(byte_stream)
                as std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, StreamError>> + Send>>,
            String::new(),
            std::collections::VecDeque::<Result<UIMessageChunk, StreamError>>::new(),
            false, // done flag
        ),
        |(mut stream, mut buffer, mut queue, done)| async move {
            // Always drain the queue first, even if done
            if let Some(item) = queue.pop_front() {
                return Some((item, (stream, buffer, queue, done)));
            }

            if done {
                return None;
            }

            loop {

                // Try to extract complete events from the buffer (delimited by \n\n)
                let mut found_done = false;
                while let Some(pos) = buffer.find("\n\n") {
                    let event_block = buffer[..pos].to_string();
                    buffer = buffer[pos + 2..].to_string();

                    if let Some(data) = extract_data_from_event(&event_block) {
                        if data == "[DONE]" {
                            found_done = true;
                            break;
                        }
                        match serde_json::from_str::<UIMessageChunk>(&data) {
                            Ok(chunk) => queue.push_back(Ok(chunk)),
                            Err(e) => queue.push_back(Err(StreamError::InvalidChunk(
                                format!("Failed to parse SSE data as UIMessageChunk: {e}"),
                            ))),
                        }
                    }
                    // If no data lines in the block, just skip it
                }

                if found_done {
                    // Emit any remaining queued items, then signal end
                    if let Some(item) = queue.pop_front() {
                        return Some((item, (stream, buffer, queue, true)));
                    }
                    return None;
                }

                // Emit any parsed items before reading more data
                if let Some(item) = queue.pop_front() {
                    return Some((item, (stream, buffer, queue, false)));
                }

                // Need more data from the underlying byte stream
                match stream.next().await {
                    Some(Ok(bytes)) => {
                        match std::str::from_utf8(&bytes) {
                            Ok(s) => buffer.push_str(s),
                            Err(e) => {
                                return Some((
                                    Err(StreamError::InvalidSse(format!(
                                        "Invalid UTF-8 in SSE stream: {e}"
                                    ))),
                                    (stream, buffer, queue, false),
                                ));
                            }
                        }
                        // Loop back to try parsing the updated buffer
                    }
                    Some(Err(e)) => {
                        return Some((Err(e), (stream, buffer, queue, false)));
                    }
                    None => {
                        // Underlying stream ended. Try to parse any leftover in buffer.
                        let remaining = buffer.trim().to_string();
                        buffer.clear();

                        if !remaining.is_empty() {
                            if let Some(data) = extract_data_from_event(&remaining) {
                                if data != "[DONE]" {
                                    match serde_json::from_str::<UIMessageChunk>(&data) {
                                        Ok(chunk) => {
                                            return Some((
                                                Ok(chunk),
                                                (stream, buffer, queue, true),
                                            ));
                                        }
                                        Err(e) => {
                                            return Some((
                                                Err(StreamError::InvalidChunk(format!(
                                                    "Failed to parse final SSE data: {e}"
                                                ))),
                                                (stream, buffer, queue, true),
                                            ));
                                        }
                                    }
                                }
                            }
                        }

                        // Drain remaining queue items
                        if let Some(item) = queue.pop_front() {
                            return Some((item, (stream, buffer, queue, true)));
                        }

                        return None;
                    }
                }
            }
        },
    )
}

/// Serialize a UIMessageChunk to SSE wire format.
pub fn chunk_to_sse(chunk: &UIMessageChunk) -> String {
    format!("data: {}\n\n", serde_json::to_string(chunk).unwrap())
}

/// SSE stream terminator.
pub fn sse_done() -> &'static str {
    "data: [DONE]\n\n"
}

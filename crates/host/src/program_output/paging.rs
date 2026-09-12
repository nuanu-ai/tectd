use crate::responses::encoded_len;
use serde::Serialize;
use serde_json::Value;
use tect_domain::{Error, Result};

/// A JSON fragment appears inside the JSON text content block of an MCP response.
/// Count both escaping layers once, without repeatedly encoding the full history.
fn fragment_bytes(value: &impl Serialize) -> Result<usize> {
    let raw = serde_json::to_string(value).map_err(|_| Error::TransportUnavailable)?;
    serde_json::to_vec(&raw)
        .map(|bytes| bytes.len() - 2)
        .map_err(|_| Error::TransportUnavailable)
}

pub(crate) fn fitting_prefix<T: Serialize>(
    items: &[T],
    first_page: &Value,
    items_key: &str,
    next_key: &str,
    capacity: usize,
    context: impl Fn(&[T], bool) -> Result<(Vec<Value>, Value)>,
) -> Result<usize> {
    let fixed = encoded_len(first_page)?
        - fragment_bytes(&first_page[items_key])?
        - fragment_bytes(&first_page[next_key])?
        - fragment_bytes(&first_page["actions"])?;
    let mut entries = 2; // Array brackets; commas are added between whole entries.
    let mut accepted = 0;
    for (index, item) in items.iter().enumerate() {
        entries += fragment_bytes(item)? + usize::from(index > 0);
        let (actions, next) = context(&items[..=index], index + 1 < items.len())?;
        let bytes = fixed + entries + fragment_bytes(&actions)? + fragment_bytes(&next)?;
        if bytes > capacity {
            break;
        }
        accepted = index + 1;
    }
    if accepted == 0 {
        Err(Error::RequestTooLarge)
    } else {
        Ok(accepted)
    }
}

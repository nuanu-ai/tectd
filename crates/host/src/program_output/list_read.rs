//! Complete Program list entries selected before stateless JSON byte windows.
use super::{list_actions, summary_next, summary_prefix};
use crate::responses::{encoded_len, with_actions};
use serde_json::{Value, json};
use tect_domain::{Error, ProgramList, Result};
use uuid::Uuid;

pub(crate) fn read(
    mut list: ProgramList,
    workspace_id: Uuid,
    after: Option<&str>,
    limit: u32,
    window: &crate::planning_read::Window,
    capacity: usize,
) -> Result<Value> {
    let budget = capacity.min(crate::json_fragment::READ_BUDGET);
    if !list.programs.is_empty() {
        let original_next = list.next_after.clone();
        let first = ProgramList {
            programs: vec![list.programs[0].clone()],
            next_after: summary_next(&list.programs[..1], list.programs.len() > 1, &original_next),
        };
        let first_value = with_actions(
            json!(&first),
            list_actions(&first.programs, &first.next_after)?,
            Some(0),
        );
        let count = match summary_prefix(&list.programs, &first_value, budget, &original_next) {
            Ok(count) => count,
            // Preserve the full first summary, including its unbounded name.
            Err(Error::RequestTooLarge) => 1,
            Err(error) => return Err(error),
        };
        list.next_after = summary_next(
            &list.programs[..count],
            count < list.programs.len(),
            &original_next,
        );
        list.programs.truncate(count);
    }
    let actions = list_actions(&list.programs, &list.next_after)?;
    let full = with_actions(json!(&list), actions.clone(), Some(0));
    if window.offset_bytes.is_none()
        && window.limit_bytes.is_none()
        && window.representation_digest.is_none()
        && encoded_len(&full)? <= budget
    {
        return Ok(full);
    }
    let mut params = json!({"workspace_id":workspace_id,"limit":limit});
    if let Some(after) = after {
        params["after"] = json!(after);
    }
    crate::json_fragment::encode(
        &list,
        actions,
        capacity,
        window.borrowed(),
        json!({"workspace_id":workspace_id}),
        "list_programs",
        params,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning_read::Window;
    use tect_domain::{ProgramStatus, ProgramStep, ProgramSummary};

    fn summary(id: u128, name: String) -> ProgramSummary {
        ProgramSummary {
            id: Uuid::from_u128(id),
            status: ProgramStatus::Draft,
            revision: 1,
            name: Some(name),
            current_step: ProgramStep::Compose,
        }
    }

    // Begin controls intentionally contain fresh request IDs; compare their contract.
    fn stable_controls(mut value: Value) -> Value {
        if let Some(actions) = value["actions"].as_array_mut() {
            for action in actions {
                if let Some(params) = action["arguments"]["params"].as_object_mut() {
                    params.remove("request_id");
                }
            }
        }
        value
    }

    fn terminal_actions(list: &ProgramList) -> Value {
        stable_controls(json!({"actions":list_actions(&list.programs, &list.next_after).unwrap()}))
            ["actions"]
            .clone()
    }

    #[test]
    fn ordinary_list_and_empty_list_preserve_legacy_shape() {
        for programs in [vec![], vec![summary(1, "small".into())]] {
            let list = ProgramList {
                programs,
                next_after: None,
            };
            let old = super::super::list(list.clone(), 8192).unwrap();
            assert_eq!(
                stable_controls(
                    read(list, Uuid::from_u128(9), None, 25, &Window::default(), 8192).unwrap()
                ),
                stable_controls(old)
            );
        }
    }

    #[test]
    fn complete_first_name_reassembles_with_original_cursor_and_terminal_actions() {
        let workspace_id = Uuid::from_u128(9);
        let first = summary(1, "\"🙂漢".repeat(4000));
        let next = first.cursor().encode();
        let list = ProgramList {
            programs: vec![first.clone(), summary(2, "second".into())],
            next_after: Some("w:00000000-0000-0000-0000-000000000003".into()),
        };
        let selected = ProgramList {
            programs: vec![first],
            next_after: Some(next),
        };
        let expected = serde_json::to_vec(&serde_json::to_value(&selected).unwrap()).unwrap();
        let after = "w:00000000-0000-0000-0000-00000000AABB";
        assert!(tect_domain::ProgramCursor::parse(after).is_ok());
        let mut window = Window {
            limit_bytes: Some(257),
            ..Window::default()
        };
        let mut bytes = Vec::new();
        loop {
            let page = read(list.clone(), workspace_id, Some(after), 2, &window, 8192).unwrap();
            assert!(encoded_len(&page).unwrap() <= 8192);
            assert_eq!(
                stable_controls(page.clone()),
                stable_controls(
                    read(list.clone(), workspace_id, Some(after), 2, &window, 8192).unwrap()
                )
            );
            bytes.extend_from_slice(page["text"].as_str().unwrap().as_bytes());
            window.representation_digest =
                Some(page["representation_digest"].as_str().unwrap().into());
            let Some(offset) = page["next_offset_bytes"].as_u64() else {
                assert_eq!(
                    stable_controls(page.clone())["actions"],
                    terminal_actions(&selected)
                );
                break;
            };
            let params = &page["actions"][0]["arguments"]["params"];
            assert_eq!(params["workspace_id"], json!(workspace_id));
            assert_eq!(params["limit"], 2);
            assert_eq!(params["after"], after);
            assert_eq!(params["offset_bytes"], offset);
            window.offset_bytes = Some(offset);
        }
        assert_eq!(bytes, expected);
        window.offset_bytes = Some(bytes.len() as u64);
        let eof = read(list.clone(), workspace_id, Some(after), 2, &window, 8192).unwrap();
        assert_eq!(eof["text"], "");
        assert_eq!(stable_controls(eof)["actions"], terminal_actions(&selected));
        window.representation_digest = Some("a".repeat(64));
        assert!(
            read(list, workspace_id, Some(after), 2, &window, 8192)
                .unwrap_err()
                .refusal()
                .is_some()
        );
    }

    #[test]
    fn collection_prefix_is_independent_of_byte_window_and_keeps_last_cursor() {
        let list = ProgramList {
            programs: vec![
                summary(1, "x".repeat(2500)),
                summary(2, "y".repeat(2500)),
                summary(3, "z".repeat(2500)),
            ],
            next_after: None,
        };
        let ordinary = read(
            list.clone(),
            Uuid::from_u128(9),
            None,
            3,
            &Window::default(),
            8192,
        )
        .unwrap();
        let count = ordinary["programs"].as_array().unwrap().len();
        assert!(count > 0 && count < 3);
        assert_eq!(
            ordinary["next_after"],
            list.programs[count - 1].cursor().encode()
        );
        let first = read(
            list.clone(),
            Uuid::from_u128(9),
            None,
            3,
            &Window {
                limit_bytes: Some(128),
                ..Window::default()
            },
            8192,
        )
        .unwrap();
        let second = read(
            list,
            Uuid::from_u128(9),
            None,
            3,
            &Window {
                limit_bytes: Some(256),
                ..Window::default()
            },
            8192,
        )
        .unwrap();
        assert_eq!(
            first["representation_digest"],
            second["representation_digest"]
        );
        assert_eq!(first["total_bytes"], second["total_bytes"]);
        assert!(
            first["actions"][0]["arguments"]["params"]
                .get("after")
                .is_none()
        );
    }
}

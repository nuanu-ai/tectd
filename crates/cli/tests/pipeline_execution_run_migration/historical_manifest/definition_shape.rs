use serde_json::Value;
use tect_domain::PipelineDefinitionSnapshot;

pub(super) fn parse_and_compare(raw: &Value) -> PipelineDefinitionSnapshot {
    let definition = serde_json::from_value::<PipelineDefinitionSnapshot>(raw.clone()).unwrap();
    let mut comparable = raw.clone();
    let phases = comparable["phases"]
        .as_array_mut()
        .expect("typed definition has a phase array");
    for phase in phases {
        let has_empty_resources = phase
            .get("resources")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty);
        if has_empty_resources {
            phase
                .as_object_mut()
                .expect("typed phase is an object")
                .remove("resources");
        }
    }
    assert_eq!(comparable, serde_json::to_value(&definition).unwrap());
    definition
}

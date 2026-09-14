use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::{fs, path::Path, process::Command};

pub fn sha256(path: &Path) -> String {
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

pub fn tree_bytes(path: &Path) -> u64 {
    fn visit(path: &Path, total: &mut u64) {
        let metadata = fs::symlink_metadata(path).unwrap();
        assert!(
            !metadata.file_type().is_symlink(),
            "asset trees must be immutable files"
        );
        if metadata.is_file() {
            *total = total.checked_add(metadata.len()).unwrap();
        } else {
            for entry in fs::read_dir(path).unwrap() {
                visit(&entry.unwrap().path(), total);
            }
        }
    }
    let mut total = 0;
    visit(path, &mut total);
    total
}

pub fn process_sample(counter: &Path) -> Value {
    let counts: Value = serde_json::from_slice(&fs::read(counter).unwrap()).unwrap();
    let proxy = counts["proxy_pid"].as_u64().unwrap();
    let worker = counts["worker_pid"].as_u64().unwrap();
    let output = Command::new("ps")
        .args([
            "-o",
            "pid=,ppid=,rss=,%cpu=",
            "-p",
            &format!("{proxy},{worker}"),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let mut rows = Vec::new();
    for line in String::from_utf8(output.stdout).unwrap().lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        assert_eq!(fields.len(), 4);
        rows.push(json!({
            "pid": fields[0].parse::<u64>().unwrap(),
            "parent_pid": fields[1].parse::<u64>().unwrap(),
            "rss_kib": fields[2].parse::<u64>().unwrap(),
            "cpu_percent": fields[3].parse::<f64>().unwrap()
        }));
    }
    assert_eq!(rows.len(), 2);
    let proxy_row = rows.iter().find(|row| row["pid"] == proxy).unwrap();
    let worker_row = rows.iter().find(|row| row["pid"] == worker).unwrap();
    assert_eq!(worker_row["parent_pid"], proxy_row["pid"]);
    json!({"counts":counts,"processes":rows})
}

pub async fn relation_sizes(pool: &PgPool) -> Value {
    let rows = sqlx::query(
        "SELECT c.relname,pg_catalog.pg_relation_size(c.oid)::bigint,\
         pg_catalog.pg_total_relation_size(c.oid)::bigint FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' \
         AND (c.relname LIKE 'knowledge_search_%') ORDER BY c.relname",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    Value::Array(
        rows.into_iter()
            .map(|row| {
                json!({"relation":row.get::<String,_>(0),"table_or_index_bytes":row.get::<i64,_>(1),
                    "total_bytes":row.get::<i64,_>(2)})
            })
            .collect(),
    )
}

//! Append-only JSON Lines activity logger with a UTC timestamp on every event.
use crate::durability::sync_parent;
use anyhow::Context;
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
};

pub struct Logger {
    file: File,
}
#[derive(Serialize)]
struct Event<'a, T: Serialize> {
    timestamp: String,
    run_id: &'a str,
    event: &'a str,
    details: T,
}

impl Logger {
    pub fn create(output: &Path, run_id: &str) -> anyhow::Result<Self> {
        let filename = format!(
            "activity_{}_{}.jsonl",
            Utc::now().format("%Y-%m-%dT%H-%M-%S%.3fZ"),
            run_id
        );
        let path = output.join(filename);
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)
            .context("creating activity log")?;
        sync_parent(&path).context("persisting activity log directory entry")?;
        Ok(Self { file })
    }
    pub fn event<T: Serialize>(
        &mut self,
        run_id: &str,
        event: &str,
        details: T,
    ) -> anyhow::Result<()> {
        let entry = Event {
            timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            run_id,
            event,
            details,
        };
        serde_json::to_writer(&mut self.file, &entry)?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        Ok(())
    }
}

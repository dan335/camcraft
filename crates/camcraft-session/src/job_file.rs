//! Job files: how a job is read and written.

use std::path::Path;

use camcraft_core::job::{JOB_VERSION, Job, JobError};

use crate::{SessionError, fs};

/// Read a job file.
pub fn read_job(path: &Path) -> Result<Job, SessionError> {
    let name = path.display().to_string();
    let text = std::fs::read_to_string(path).map_err(|source| SessionError::JobIo {
        path: name.clone(),
        source,
    })?;
    parse_job(&text, &name)
}

/// Read a job from its text; `name` says where it came from, for errors.
pub fn parse_job(text: &str, name: &str) -> Result<Job, SessionError> {
    // The version first, so a file of another format is named for what it is
    // rather than refused over whichever field it lacks.
    #[derive(serde::Deserialize)]
    struct Probe {
        version: Option<u32>,
    }
    let parse = |source| SessionError::JobParse {
        path: name.to_string(),
        source,
    };
    if let Some(v) = toml::from_str::<Probe>(text).map_err(parse)?.version
        && v != JOB_VERSION
    {
        return Err(JobError::UnsupportedVersion(v).into());
    }
    toml::from_str(text).map_err(|source| match misplaced(text, &source) {
        Some((key, section)) => SessionError::JobMisplaced {
            path: name.to_string(),
            key,
            section,
            source: Box::new(source),
        },
        None => parse(source),
    })
}

/// The sections of an operation a setting may belong in.
const SECTIONS: [&str; 3] = ["passes", "heights", "linking"];

/// Where a setting refused as unknown in an operation belongs, if it is one of
/// that operation's sections': found by moving it there and reading the job again,
/// so the answer follows the format rather than a list kept beside it.
fn misplaced(text: &str, error: &toml::de::Error) -> Option<(String, &'static str)> {
    let key = error
        .message()
        .strip_prefix("unknown field `")?
        .split('`')
        .next()?
        .to_string();
    let table: toml::Table = toml::from_str(text).ok()?;
    SECTIONS.into_iter().find_map(|section| {
        let mut moved = table.clone();
        let mut found = false;
        for setup in setups_mut(&mut moved) {
            for op in operations_mut(setup) {
                if let Some(value) = op.remove(&key) {
                    found = true;
                    let into = op
                        .entry(section)
                        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
                    into.as_table_mut()?.insert(key.clone(), value);
                }
            }
        }
        if !found {
            return None;
        }
        let refused_again = match toml::Value::Table(moved).try_into::<Job>() {
            Ok(_) => false,
            Err(e) => e.message().contains(&format!("`{key}`")),
        };
        (!refused_again).then(|| (key.clone(), section))
    })
}

fn setups_mut(job: &mut toml::Table) -> impl Iterator<Item = &mut toml::Table> {
    tables_mut(job.get_mut("setup"))
}

fn operations_mut(setup: &mut toml::Table) -> impl Iterator<Item = &mut toml::Table> {
    tables_mut(setup.get_mut("operation"))
}

/// The tables of an array of tables, if `value` is one.
fn tables_mut(value: Option<&mut toml::Value>) -> impl Iterator<Item = &mut toml::Table> {
    value
        .and_then(toml::Value::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table_mut)
}

/// A job as the text of its file.
pub fn job_text(job: &Job) -> Result<String, SessionError> {
    let body = toml::to_string(job)?;
    Ok(format!("# camcraft job. Written by camcraft.\n\n{body}"))
}

/// Write a job file, whole or not at all.
pub fn write_job(path: &Path, job: &Job) -> Result<(), SessionError> {
    let text = job_text(job)?;
    fs::write_atomically(path, &text).map_err(|source| SessionError::JobWrite {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    /// The example job reads, and a job written and read back is the same job.
    #[test]
    fn jobs_read_and_write() {
        let example =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/rail-jig.camcraft");
        let job = read_job(&example).unwrap();
        job.validate().unwrap();
        let dir = std::env::temp_dir().join(format!("camcraft-session-job-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("again.camcraft");
        write_job(&path, &job).unwrap();
        assert_eq!(read_job(&path).unwrap(), job);
    }

    /// A section's setting written directly in an operation is refused, and
    /// named with the section it belongs in; a setting no section has is refused
    /// as unknown.
    #[test]
    fn a_misplaced_setting_is_named_with_its_section() {
        let job = |line: &str| {
            format!(
                "version = 1\n[[setup]]\nname = \"s\"\n[[setup.operation]]\nname = \"o\"\n\
                 strategy = \"profile\"\n{line}\n"
            )
        };
        for (line, key, section) in [
            ("stepdown = 6.35", "stepdown", "passes"),
            ("safe_distance = 1.0", "safe_distance", "linking"),
            (
                "clearance = { ref = \"stock-top\" }",
                "clearance",
                "heights",
            ),
        ] {
            match parse_job(&job(line), "lego.camcraft") {
                Err(SessionError::JobMisplaced {
                    key: k, section: s, ..
                }) => {
                    assert_eq!((k.as_str(), s), (key, section));
                }
                other => panic!("{line}: {other:?}"),
            }
        }
        assert!(matches!(
            parse_job(&job("stepdwon = 6.35"), "lego.camcraft"),
            Err(SessionError::JobParse { .. })
        ));
    }

    /// A file of another format is named as one, not refused over a field.
    #[test]
    fn another_format_is_named_as_one() {
        assert!(matches!(
            parse_job("version = 2\n", "new.camcraft"),
            Err(SessionError::Job(JobError::UnsupportedVersion(2)))
        ));
    }
}

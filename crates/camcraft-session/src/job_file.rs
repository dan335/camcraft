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
    toml::from_str(text).map_err(parse)
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
#[allow(clippy::unwrap_used)]
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

    /// A file of another format is named as one, not refused over a field.
    #[test]
    fn another_format_is_named_as_one() {
        assert!(matches!(
            parse_job("version = 2\n", "new.camcraft"),
            Err(SessionError::Job(JobError::UnsupportedVersion(2)))
        ));
    }
}

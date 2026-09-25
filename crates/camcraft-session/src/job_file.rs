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
    // The version first, so an old or newer file is named for what it is rather
    // than refused over whichever field it lacks.
    #[derive(serde::Deserialize)]
    struct Probe {
        version: Option<u32>,
    }
    let parse = |source| SessionError::JobParse {
        path: name.to_string(),
        source,
    };
    match toml::from_str::<Probe>(text).map_err(parse)?.version {
        None => return Err(SessionError::OldJob(name.to_string())),
        Some(v) if !(1..=JOB_VERSION).contains(&v) => {
            return Err(JobError::UnsupportedVersion(v).into());
        }
        Some(_) => {}
    }
    // Read into this build's format. What a file is written as is decided when it
    // is written (`job_text`), from what the job holds, not from what it was read as.
    let mut job: Job = toml::from_str(text).map_err(parse)?;
    job.version = JOB_VERSION;
    Ok(job)
}

/// A job as the text of its file, at the oldest version that holds it
/// ([`Job::required_version`]): a job an older build can read in full is written
/// so it can, and one it would misread is written so it refuses it.
pub fn job_text(job: &Job) -> Result<String, SessionError> {
    let mut job = job.clone();
    job.version = job.required_version();
    let body = toml::to_string(&job)?;
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
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
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

    /// A job from before setups is named as that, not refused over a field.
    #[test]
    fn an_old_job_is_named_as_one() {
        let old = "tolerance = 0.05\n[setup]\nstock = { kind = \"box\" }\n";
        assert!(matches!(
            parse_job(old, "old.toml"),
            Err(SessionError::OldJob(_))
        ));
        assert!(matches!(
            parse_job("version = 9\n", "new.camcraft"),
            Err(SessionError::Job(JobError::UnsupportedVersion(9)))
        ));
    }

    /// A job is written at the oldest version that holds it: an older build reads
    /// a job without picks as before, and refuses one with picks by name rather
    /// than cutting the whole part where one face was asked for.
    #[test]
    fn a_job_is_written_at_the_version_it_needs() {
        let example =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/rail-jig.camcraft");
        let mut job = read_job(&example).unwrap();
        let text = job_text(&job).unwrap();
        assert!(
            text.contains(
                "version = 1
"
            ),
            "{text}"
        );
        assert_eq!(parse_job(&text, "one").unwrap(), job);

        let model = job.setups[0].models[0].clone();
        job.setups[0].operations[0]
            .geometry
            .faces
            .push(camcraft_core::job::FeatureRef {
                model,
                index: 0,
                at: [0.0, 0.0, 0.0],
            });
        let text = job_text(&job).unwrap();
        assert!(
            text.contains(
                "version = 2
"
            ),
            "{text}"
        );
        assert_eq!(parse_job(&text, "two").unwrap(), job);
    }
}

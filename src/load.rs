use std::path::{Path, PathBuf};

use crate::bean::Bean;

/// one bean that didn't make it. the rest of the dir still loads
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("{}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{}: {source}", path.display())]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{}: {problem}", path.display())]
    Invalid { path: PathBuf, problem: String },
}

/// the whole dir is unusable
#[derive(Debug, thiserror::Error)]
pub enum DirError {
    #[error("beans dir not found: {}", .0.display())]
    NotFound(PathBuf),
    #[error("{}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug, Default)]
pub struct Harvest {
    /// sorted by file name, so output is stable
    pub beans: Vec<Bean>,
    pub errors: Vec<LoadError>,
    /// old `.md` beans sitting in the dir, skipped
    pub markdown: usize,
}

/// read every `*.json` in the dir. a bad bean lands in `errors`, never fails the lot
#[tracing::instrument(name = "load", skip_all, fields(dir = %dir.display()))]
pub fn load(dir: &Path) -> Result<Harvest, DirError> {
    if !dir.is_dir() {
        return Err(DirError::NotFound(dir.to_owned()));
    }
    let read_err = |source| DirError::Read {
        path: dir.to_owned(),
        source,
    };
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(read_err)? {
        paths.push(entry.map_err(read_err)?.path());
    }
    paths.sort();

    let mut harvest = Harvest::default();
    for path in paths {
        match path.extension().and_then(|e| e.to_str()) {
            Some("json") => match parse(&path) {
                Ok(bean) => harvest.beans.push(bean),
                Err(errs) => harvest.errors.extend(errs),
            },
            Some("md") => harvest.markdown += 1,
            _ => {}
        }
    }
    tracing::info!(
        beans = harvest.beans.len(),
        errors = harvest.errors.len(),
        "loaded"
    );
    Ok(harvest)
}

// debug, so a normal run doesn't write a line per bean
#[tracing::instrument(name = "parse", level = "debug", skip_all, fields(file = %path.display()))]
fn parse(path: &Path) -> Result<Bean, Vec<LoadError>> {
    let raw = std::fs::read(path).map_err(|source| {
        vec![LoadError::Read {
            path: path.to_owned(),
            source,
        }]
    })?;
    let bean: Bean = serde_json::from_slice(&raw).map_err(|source| {
        vec![LoadError::Parse {
            path: path.to_owned(),
            source,
        }]
    })?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let problems = bean.problems(stem);
    if problems.is_empty() {
        Ok(bean)
    } else {
        Err(problems
            .into_iter()
            .map(|problem| LoadError::Invalid {
                path: path.to_owned(),
                problem,
            })
            .collect())
    }
}

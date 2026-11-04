//! reading beans off disk. the cli never writes them
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::bean::{self, Bean, SCHEMA};

/// one bean that didn't make it. the rest of the dir still loads
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// the file couldn't be read
    #[error("{}: {source}", path.display())]
    Read {
        /// path to the bean
        path: PathBuf,
        /// what the os said
        source: std::io::Error,
    },
    /// the json doesn't match the schema
    #[error("{}: {source}", path.display())]
    Parse {
        /// path to the bean
        path: PathBuf,
        /// what serde said, with line and column
        source: serde_json::Error,
    },
    /// written for a schema this senzu doesn't read
    #[error("{}: schema {found}, this senzu reads {SCHEMA}{}", path.display(), schema_hint(*found))]
    Schema {
        /// path to the bean
        path: PathBuf,
        /// the schema the bean says it's on
        found: u32,
    },
    /// a field this senzu doesn't know. the bean still loads, `check` fails
    #[error("{}: unknown field `{field}`, from a newer senzu? upgrade, or drop it", path.display())]
    UnknownField {
        /// path to the bean
        path: PathBuf,
        /// where in the bean, like `open.0.vibes`
        field: String,
    },
    /// it parsed, but breaks a rule serde can't check
    #[error("{}: {problem}", path.display())]
    Invalid {
        /// path to the bean
        path: PathBuf,
        /// which rule, in words
        problem: String,
    },
}

/// the whole dir is unusable
#[derive(Debug, thiserror::Error)]
pub enum DirError {
    /// it isn't there, or isn't a dir
    #[error("beans dir not found: {}", .0.display())]
    NotFound(PathBuf),
    /// it's there but couldn't be listed
    #[error("{}: {source}", path.display())]
    Read {
        /// the beans dir
        path: PathBuf,
        /// what the os said
        source: std::io::Error,
    },
}

/// why `one` came back empty-handed
#[derive(Debug, thiserror::Error)]
pub enum OneError {
    /// no `<slug>.json`, or a slug that could never name one. exit 3
    #[error("no bean named {0:?}")]
    Unknown(String),
    /// the whole dir is unusable
    #[error(transparent)]
    Dir(#[from] DirError),
    /// the file is there but bad: every problem, one per line
    #[error("{}", lines(.0))]
    Bad(Vec<LoadError>),
}

fn schema_hint(found: u32) -> &'static str {
    if found > SCHEMA {
        ", upgrade senzu"
    } else {
        ", the bean needs migrating"
    }
}

fn lines(errors: &[LoadError]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// what a dir held
#[derive(Debug, Default)]
pub struct Harvest {
    /// sorted by file name, so output is stable
    pub beans: Vec<Bean>,
    /// the beans that didn't make it, and why
    pub errors: Vec<LoadError>,
    /// fields the beans that did load carry and this senzu doesn't know
    pub unknown: Vec<LoadError>,
    /// old `.md` beans sitting in the dir, skipped
    pub markdown: usize,
}

/// read every `*.json` in the dir. a bad bean lands in `errors`, never fails the lot
///
/// # Errors
///
/// a [`DirError`] when the dir is missing or can't be listed.
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
                Ok(found) => {
                    for e in &found.unknown {
                        tracing::info!(file = %path.display(), outcome = "ignored", reason = %e, "unknown field");
                    }
                    harvest.beans.push(found.bean);
                    harvest.unknown.extend(found.unknown);
                }
                Err(errs) => {
                    for e in &errs {
                        tracing::info!(file = %path.display(), outcome = "skipped", reason = %e, "bad bean");
                    }
                    harvest.errors.extend(errs);
                }
            },
            Some("md") => harvest.markdown += 1,
            _ => {}
        }
    }
    tracing::info!(
        beans = harvest.beans.len(),
        errors = harvest.errors.len(),
        unknown = harvest.unknown.len(),
        markdown = harvest.markdown,
        "loaded"
    );
    Ok(harvest)
}

/// read just `<slug>.json`: the file name is the key, so the rest of the dir
/// never gets opened. the slug is checked first, which also keeps it from
/// walking out of the dir (`../x` isn't kebab-case)
///
/// # Errors
///
/// [`OneError::Unknown`] when there's no such bean, [`OneError::Bad`] when
/// it's there but broken, and [`OneError::Dir`] when the dir itself is.
#[tracing::instrument(name = "load", skip_all, fields(dir = %dir.display(), slug))]
pub fn one(dir: &Path, slug: &str) -> Result<Found, OneError> {
    if !dir.is_dir() {
        return Err(DirError::NotFound(dir.to_owned()).into());
    }
    let path = dir.join(format!("{slug}.json"));
    if !bean::is_slug(slug) || !path.is_file() {
        return Err(OneError::Unknown(slug.to_owned()));
    }
    parse(&path).map_err(OneError::Bad)
}

/// one bean, and the fields in it this senzu doesn't know
#[derive(Debug)]
pub struct Found {
    /// the parsed bean
    pub bean: Bean,
    /// each a [`LoadError::UnknownField`]
    pub unknown: Vec<LoadError>,
}

/// just enough to read the schema before trusting the rest of the shape
#[derive(Deserialize)]
struct Peek {
    schema: u32,
}

// debug, so a normal run doesn't write a line per bean
#[tracing::instrument(name = "parse", level = "debug", skip_all, fields(file = %path.display()))]
fn parse(path: &Path) -> Result<Found, Vec<LoadError>> {
    let raw = std::fs::read(path).map_err(|source| {
        vec![LoadError::Read {
            path: path.to_owned(),
            source,
        }]
    })?;
    let parse_err = |source| {
        vec![LoadError::Parse {
            path: path.to_owned(),
            source,
        }]
    };
    // the schema first: a bean from another schema may not fit the types at all
    let Peek { schema } = serde_json::from_slice(&raw).map_err(parse_err)?;
    if schema != SCHEMA {
        return Err(vec![LoadError::Schema {
            path: path.to_owned(),
            found: schema,
        }]);
    }
    let mut fields = Vec::new();
    let de = &mut serde_json::Deserializer::from_slice(&raw);
    let bean: Bean = serde_ignored::deserialize(de, |field| fields.push(field.to_string()))
        .map_err(parse_err)?;
    let unknown = fields
        .into_iter()
        .map(|field| LoadError::UnknownField {
            path: path.to_owned(),
            field,
        })
        .collect();
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let problems = bean.problems(stem);
    if problems.is_empty() {
        Ok(Found { bean, unknown })
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

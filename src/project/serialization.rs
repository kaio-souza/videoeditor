use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{EditorError, Result};

use super::Project;

pub const CURRENT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectFile {
    version: u32,
    #[serde(flatten)]
    project: Project,
}

pub fn save_project(project: &Project, path: &Path) -> Result<()> {
    let file = ProjectFile {
        version: CURRENT_VERSION,
        project: project.clone(),
    };
    let json = serde_json::to_string_pretty(&file)?;
    fs::write(path, json)?;
    Ok(())
}

pub fn load_project(path: &Path) -> Result<Project> {
    let json = fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&json)?;
    let version = value
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| EditorError::InvalidTimeline("missing project version".into()))?;

    if version != CURRENT_VERSION as u64 {
        return Err(EditorError::InvalidTimeline(format!(
            "unsupported project version: {version}"
        )));
    }

    let file: ProjectFile = serde_json::from_value(value)?;
    Ok(file.project)
}

/// Mídia cujo arquivo original não existe mais no caminho salvo (seção 59).
pub fn missing_media(project: &Project) -> Vec<&Path> {
    project
        .media
        .values()
        .filter(|media| !media.path.exists())
        .map(|media| media.path.as_path())
        .collect()
}

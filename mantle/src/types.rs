use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CowCloneResult {
    pub cloned_count: u32,
    pub fallback_count: u32,
    pub elapsed_ms: u64,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ArtifactType {
    Node,
    NodePnpm,
    Rust,
    Python,
    Swift,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum HydrationStrategy {
    CowClone,
    DelegateToPnpm,
    InjectCache,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ConfigSource {
    BuiltIn,
    Worktreeinclude,
    Gitignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum IncludeSource {
    Worktreeinclude,
    GitignoreFallback,
    None,
}

#[derive(Debug, Clone, Serialize)]
pub struct CloneCandidate {
    pub source_path: String,
    pub dest_path: String,
    pub artifact_type: ArtifactType,
    pub lockfile_matches: bool,
    pub size_bytes: u64,
    pub strategy: HydrationStrategy,
    pub skip_reason: Option<String>,
    pub config_source: ConfigSource,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileCandidate {
    pub relative_path: String,
    pub absolute_path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorktreeIncludeResult {
    pub clone_candidates: Vec<CloneCandidate>,
    pub file_candidates: Vec<FileCandidate>,
    pub source: IncludeSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EffectiveSource {
    BuiltIn,
    Worktreeinclude,
    Gitignore,
    GitignoreLifted,
    Suggestion,
}

#[derive(Debug, Clone, Serialize)]
pub struct EffectiveEntry {
    pub path: String,
    pub source: EffectiveSource,
    pub exists_on_disk: bool,
    pub size_bytes: u64,
    pub included: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EffectiveWorktreeinclude {
    pub entries: Vec<EffectiveEntry>,
    pub config_source: ConfigSource,
    pub has_worktreeinclude_file: bool,
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeneratedWorktreeinclude {
    pub content: String,
    pub builtin_dirs: Vec<String>,
    pub gitignore_dirs: Vec<String>,
    pub already_exists: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorktreeInfo {
    pub path: String,
    pub head: String,
    pub branch: Option<String>,
    pub is_main: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HydrationResult {
    pub cloned: Vec<String>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
    pub elapsed_ms: u64,
}

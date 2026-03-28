use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct CowCloneResult {
    pub cloned_count: u32,
    pub fallback_count: u32,
    pub elapsed_ms: u64,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum ArtifactType {
    Node,
    NodePnpm,
    Rust,
    Python,
    Swift,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum HydrationStrategy {
    CowClone,
    DelegateToPnpm,
    /// Reserved for future use. Originally intended for sccache injection into
    /// Rust `target/` directories, but CoW clone handles all artifact types
    /// (including Rust) effectively. Currently never returned by `resolve_strategy()`.
    InjectCache,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum ConfigSource {
    BuiltIn,
    Worktreeinclude,
    Gitignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum IncludeSource {
    Worktreeinclude,
    GitignoreFallback,
    None,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
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
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct FileCandidate {
    pub relative_path: String,
    pub absolute_path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct WorktreeIncludeResult {
    pub clone_candidates: Vec<CloneCandidate>,
    pub file_candidates: Vec<FileCandidate>,
    pub source: IncludeSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum EffectiveSource {
    BuiltIn,
    Worktreeinclude,
    Gitignore,
    GitignoreLifted,
    Suggestion,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct EffectiveEntry {
    pub path: String,
    pub source: EffectiveSource,
    pub exists_on_disk: bool,
    pub size_bytes: u64,
    pub included: bool,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct EffectiveWorktreeinclude {
    pub entries: Vec<EffectiveEntry>,
    pub config_source: ConfigSource,
    pub has_worktreeinclude_file: bool,
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct GeneratedWorktreeinclude {
    /// The generated file content, ready to write to `.worktreeinclude`.
    pub content: String,
    /// Directory names included from built-in artifact specs.
    pub builtin_dirs: Vec<String>,
    /// Directory names included from `.gitignore` parsing.
    pub gitignore_dirs: Vec<String>,
    /// Whether a `.worktreeinclude` file already existed (content is still generated but not written).
    pub already_exists: bool,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct WorktreeInfo {
    pub path: String,
    pub head: String,
    pub branch: Option<String>,
    pub is_main: bool,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct CommitInfo {
    pub hash: String,
    pub author_name: String,
    pub author_email: String,
    pub committer_name: String,
    pub committer_email: String,
    pub author_date: String,
    pub message: String,
    pub parent_hashes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct BranchInfo {
    pub name: String,
    pub date: String,
    pub author: String,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct StatusSummary {
    pub file_count: u32,
    pub output: String,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct WorktreeStatusInfo {
    pub is_dirty: bool,
    pub file_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RemoteInfo {
    pub name: String,
    pub push_url: Option<String>,
    pub fetch_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct FetchResult {
    pub updated_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct PushResult {
    pub updated_ref: Option<String>,
    pub up_to_date: bool,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct PullResult {
    pub fetch_updated_refs: Vec<String>,
    pub merge_type: String,
    pub new_head: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct AheadBehindResult {
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct CommitTreeRefsInfo {
    pub tree_hash: String,
    pub refs: String,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct BlameLineInfo {
    pub commit_hash: String,
    pub author_name: String,
    pub author_email: String,
    pub author_date: String,
    pub line_number: u32,
    pub num_lines: u32,
    pub original_line_number: u32,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RewriteResult {
    pub new_head: String,
    pub backup_ref: String,
    pub rewritten_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct CommitMetadataInfo {
    pub author_name: String,
    pub author_email: String,
    pub committer_name: String,
    pub committer_email: String,
    pub author_date: String,
    pub committer_date: String,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct StashEntry {
    pub index: u32,
    pub message: String,
    pub commit_hash: String,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct TagInfo {
    pub name: String,
    pub target_hash: String,
    pub is_annotated: bool,
    pub tagger_name: Option<String>,
    pub tagger_email: Option<String>,
    pub tagger_date: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ReflogEntry {
    pub id: String,
    pub previous_id: String,
    pub message: String,
    pub committer: String,
    pub date: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum MergeStateKind {
    None,
    Merge,
    Rebase,
    CherryPick,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct MergeStateInfo {
    pub kind: MergeStateKind,
    pub conflict_count: u32,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ConflictSides {
    pub path: String,
    pub base: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
}

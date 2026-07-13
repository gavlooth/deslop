use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use anyhow::{Context, Result, bail};

use crate::{DiscoveryPolicy, ProjectSnapshot, ProjectSnapshotBuilder, RepositoryId, ScopeSpec};

#[derive(Debug, Clone)]
pub enum RootSpec {
    Auto,
    Explicit(PathBuf),
}

#[derive(Debug, Clone)]
pub enum RepositorySpec {
    Auto,
    Explicit(RepositoryId),
}

#[derive(Debug, Clone)]
pub struct ProjectSnapshotRequest {
    pub invocation_base: PathBuf,
    pub root: RootSpec,
    pub repository: RepositorySpec,
    pub scope: ScopeSpec,
    pub discovery: DiscoveryPolicy,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnapshotPresentationMap {
    paths: BTreeMap<PathBuf, PathBuf>,
}

impl SnapshotPresentationMap {
    pub fn from_entries(entries: impl IntoIterator<Item = (PathBuf, PathBuf)>) -> Result<Self> {
        let mut paths = BTreeMap::new();
        for (logical, display) in entries {
            let logical = normalized_logical(&logical)?;
            if let Some(existing) = paths.insert(logical.clone(), display.clone())
                && existing != display
            {
                bail!(
                    "logical path {} has conflicting presentation paths",
                    logical.display()
                );
            }
        }
        Ok(Self { paths })
    }

    pub fn display_path<'a>(&'a self, logical: &'a Path) -> &'a Path {
        self.paths.get(logical).map_or(logical, PathBuf::as_path)
    }

    pub fn entries(&self) -> impl Iterator<Item = (&Path, &Path)> {
        self.paths
            .iter()
            .map(|(logical, display)| (logical.as_path(), display.as_path()))
    }
}

#[derive(Debug)]
pub struct SnapshotBuild {
    pub snapshot: Arc<ProjectSnapshot>,
    pub presentation: SnapshotPresentationMap,
}

pub struct ProjectSnapshotPlanner {
    request: ProjectSnapshotRequest,
    root: PathBuf,
    repository: RepositoryId,
    source_overlays: BTreeMap<PathBuf, Vec<u8>>,
    analysis_overlays: BTreeMap<PathBuf, Vec<u8>>,
    disk_analysis_inputs: BTreeSet<PathBuf>,
    presentation_candidates: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
    presentation_roots: Vec<(PathBuf, PathBuf)>,
}

impl ProjectSnapshotPlanner {
    pub fn resolve(mut request: ProjectSnapshotRequest) -> Result<Self> {
        request.invocation_base = request.invocation_base.canonicalize().with_context(|| {
            format!(
                "failed to resolve invocation base {}",
                request.invocation_base.display()
            )
        })?;
        if !request.invocation_base.is_dir() {
            bail!(
                "invocation base {} is not a directory",
                request.invocation_base.display()
            );
        }
        let root = match &request.root {
            RootSpec::Explicit(root) => root
                .canonicalize()
                .with_context(|| format!("failed to resolve explicit root {}", root.display()))?,
            RootSpec::Auto => resolve_auto_root(&request.invocation_base, &request.scope)?,
        };
        if !root.is_dir() {
            bail!("snapshot root {} is not a directory", root.display());
        }
        let repository = match &request.repository {
            RepositorySpec::Auto => auto_repository_id(&root)?,
            RepositorySpec::Explicit(repository) => repository.clone(),
        };
        let mut planner = Self {
            request,
            root,
            repository,
            source_overlays: BTreeMap::new(),
            analysis_overlays: BTreeMap::new(),
            disk_analysis_inputs: BTreeSet::new(),
            presentation_candidates: BTreeMap::new(),
            presentation_roots: Vec::new(),
        };
        planner.record_scope_presentations()?;
        Ok(planner)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn repository(&self) -> &RepositoryId {
        &self.repository
    }

    pub fn add_source_overlay(
        &mut self,
        logical_path: impl AsRef<Path>,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<()> {
        let logical = normalized_logical(logical_path.as_ref())?;
        insert_bytes(&mut self.source_overlays, &logical, bytes.into(), "source")?;
        self.presentation_candidates
            .entry(logical.clone())
            .or_default()
            .insert(logical);
        Ok(())
    }

    pub fn add_analysis_input_overlay(
        &mut self,
        logical_path: impl AsRef<Path>,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<()> {
        let logical = normalized_logical(logical_path.as_ref())?;
        insert_bytes(
            &mut self.analysis_overlays,
            &logical,
            bytes.into(),
            "analysis input",
        )?;
        Ok(())
    }

    pub fn add_disk_analysis_input(&mut self, path: impl AsRef<Path>) -> Result<PathBuf> {
        let logical = self.logical_for_disk_path(path.as_ref())?;
        self.disk_analysis_inputs.insert(logical.clone());
        Ok(logical)
    }

    pub fn build(self) -> Result<SnapshotBuild> {
        let mut builder = ProjectSnapshotBuilder::new(&self.root, self.repository)?
            .with_invocation_base(&self.request.invocation_base)?
            .with_scope_spec(self.request.scope)
            .with_discovery_policy(self.request.discovery);
        for (path, bytes) in self.source_overlays {
            builder = builder.with_overlay(path, bytes)?;
        }
        for path in self.disk_analysis_inputs {
            builder = builder.with_disk_analysis_input(path)?;
        }
        for (path, bytes) in self.analysis_overlays {
            builder = builder.with_analysis_input(path, bytes)?;
        }
        let snapshot = builder.build()?;
        let paths = snapshot
            .entries()
            .map(|entry| {
                let logical = entry.path().to_path_buf();
                let mut candidates = self
                    .presentation_candidates
                    .get(&logical)
                    .cloned()
                    .unwrap_or_default();
                for (logical_root, display_root) in &self.presentation_roots {
                    if let Ok(suffix) = logical.strip_prefix(logical_root) {
                        candidates.insert(display_root.join(suffix));
                    }
                }
                let display = candidates
                    .iter()
                    .min_by(|a, b| path_order(a, b))
                    .cloned()
                    .unwrap_or_else(|| logical.clone());
                (logical, display)
            })
            .collect();
        Ok(SnapshotBuild {
            snapshot,
            presentation: SnapshotPresentationMap { paths },
        })
    }

    fn record_scope_presentations(&mut self) -> Result<()> {
        let paths = match &self.request.scope {
            ScopeSpec::Requested(paths)
            | ScopeSpec::ExactFiles(paths)
            | ScopeSpec::ExactLogicalFiles(paths) => paths.clone(),
            ScopeSpec::DefaultAtInvocationBase => Vec::new(),
        };
        for display in paths {
            if display == Path::new(".") {
                self.presentation_roots
                    .push((PathBuf::new(), PathBuf::new()));
                continue;
            }
            if matches!(self.request.scope, ScopeSpec::ExactLogicalFiles(_)) {
                let logical = normalized_logical(&display)?;
                self.presentation_candidates
                    .entry(logical.clone())
                    .or_default()
                    .insert(logical);
                continue;
            }
            let physical = if display.is_absolute() {
                display.clone()
            } else {
                self.request.invocation_base.join(&display)
            }
            .canonicalize()
            .with_context(|| format!("failed to resolve input {}", display.display()))?;
            let relative = physical.strip_prefix(&self.root).with_context(|| {
                format!(
                    "input {} resolves outside snapshot root {}",
                    display.display(),
                    self.root.display()
                )
            })?;
            if physical.is_dir() {
                let logical = normalized_logical_prefix(relative)?;
                self.presentation_roots
                    .push((logical, normalized_display(&display)));
                continue;
            }
            let logical = if matches!(self.request.scope, ScopeSpec::ExactLogicalFiles(_)) {
                normalized_logical(&display)?
            } else {
                self.logical_for_disk_path(&display)?
            };
            self.presentation_candidates
                .entry(logical)
                .or_default()
                .insert(normalized_display(&display));
        }
        Ok(())
    }

    fn logical_for_disk_path(&self, path: &Path) -> Result<PathBuf> {
        let physical = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.request.invocation_base.join(path)
        };
        let physical = physical
            .canonicalize()
            .with_context(|| format!("failed to resolve input {}", path.display()))?;
        let relative = physical.strip_prefix(&self.root).with_context(|| {
            format!(
                "input {} resolves outside snapshot root {}",
                path.display(),
                self.root.display()
            )
        })?;
        normalized_logical(relative)
    }
}

fn insert_bytes(
    entries: &mut BTreeMap<PathBuf, Vec<u8>>,
    path: &Path,
    bytes: Vec<u8>,
    role: &str,
) -> Result<()> {
    if let Some(existing) = entries.get(path)
        && existing != &bytes
    {
        bail!("{role} overlay {} has conflicting bytes", path.display());
    }
    entries.insert(path.to_path_buf(), bytes);
    Ok(())
}

fn resolve_auto_root(invocation_base: &Path, scope: &ScopeSpec) -> Result<PathBuf> {
    let requested = match scope {
        ScopeSpec::DefaultAtInvocationBase | ScopeSpec::ExactLogicalFiles(_) => Vec::new(),
        ScopeSpec::Requested(paths) | ScopeSpec::ExactFiles(paths) => paths.clone(),
    };
    if requested.is_empty() {
        return Ok(nearest_repository_root(invocation_base)
            .unwrap_or_else(|| invocation_base.to_path_buf()));
    }
    let mut anchors = Vec::new();
    let mut repositories = BTreeSet::new();
    for path in requested {
        let physical = if path.is_absolute() {
            path
        } else {
            invocation_base.join(path)
        };
        let physical = physical
            .canonicalize()
            .with_context(|| format!("failed to resolve requested path {}", physical.display()))?;
        let anchor = if physical.is_file() {
            physical
                .parent()
                .expect("a canonical file has a parent")
                .to_path_buf()
        } else {
            physical
        };
        if let Some(repository) = nearest_repository_root(&anchor) {
            repositories.insert(repository);
        }
        anchors.push(anchor);
    }
    if repositories.len() > 1 {
        bail!("requested scope spans repositories; provide an explicit root");
    }
    if let Some(repository) = repositories.into_iter().next() {
        if anchors.iter().all(|anchor| anchor.starts_with(&repository)) {
            return Ok(repository);
        }
        bail!("requested scope crosses the discovered repository boundary");
    }
    Ok(common_ancestor(&anchors).expect("non-empty anchors have a common filesystem ancestor"))
}

fn nearest_repository_root(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|ancestor| ancestor.join(".jj").exists() || ancestor.join(".git").exists())
        .map(Path::to_path_buf)
}

fn auto_repository_id(root: &Path) -> Result<RepositoryId> {
    let identity = if root.join(".git").exists() {
        git_repository_identity(root)
    } else if root.join(".jj").exists() {
        jj_repository_identity(root)
    } else {
        None
    };
    identity.map_or_else(|| RepositoryId::local(root), Ok)
}

fn git_repository_identity(root: &Path) -> Option<RepositoryId> {
    let remote = command_output("git", ["-C", root.to_str()?, "remote", "get-url", "origin"])
        .or_else(|| {
            let name = command_output("git", ["-C", root.to_str()?, "remote"])?
                .lines()
                .next()?
                .to_string();
            command_output(
                "git",
                ["-C", root.to_str()?, "remote", "get-url", name.as_str()],
            )
        })
        .map(|value| normalize_remote(&value));
    let roots = command_output(
        "git",
        ["-C", root.to_str()?, "rev-list", "--max-parents=0", "--all"],
    )
    .map(|output| revision_lines(&output))
    .unwrap_or_default();
    RepositoryId::vcs(remote.as_deref(), &roots).ok()
}

fn jj_repository_identity(root: &Path) -> Option<RepositoryId> {
    let remotes = command_output(
        "jj",
        ["--repository", root.to_str()?, "git", "remote", "list"],
    )
    .unwrap_or_default();
    let remote = remotes
        .lines()
        .find(|line| line.split_whitespace().next() == Some("origin"))
        .or_else(|| remotes.lines().next())
        .and_then(|line| line.split_whitespace().nth(1))
        .map(normalize_remote);
    let roots = command_output(
        "jj",
        [
            "--repository",
            root.to_str()?,
            "log",
            "-r",
            "roots(::@) & ~root()",
            "--no-graph",
            "-T",
            "commit_id ++ \"\\n\"",
        ],
    )
    .map(|output| revision_lines(&output))
    .unwrap_or_default();
    RepositoryId::vcs(remote.as_deref(), &roots).ok()
}

fn command_output<const N: usize>(program: &str, args: [&str; N]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8(output.stdout).ok()?.trim().to_string())
}

fn revision_lines(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && line.chars().any(|ch| ch != '0')
                && line.chars().all(|ch| ch.is_ascii_hexdigit())
        })
        .map(str::to_string)
        .collect()
}

fn normalize_remote(remote: &str) -> String {
    let remote = remote.trim().trim_end_matches('/').trim_end_matches(".git");
    if let Some((scheme, rest)) = remote.split_once("://") {
        let authority_and_path = rest.rsplit_once('@').map_or(rest, |(_, value)| value);
        return format!("{}://{}", scheme.to_ascii_lowercase(), authority_and_path);
    }
    if let Some((user_host, path)) = remote.split_once(':') {
        let host = user_host
            .rsplit_once('@')
            .map_or(user_host, |(_, host)| host);
        return format!("{host}/{path}");
    }
    remote.to_string()
}

fn common_ancestor(paths: &[PathBuf]) -> Option<PathBuf> {
    let mut common = paths.first()?.clone();
    while !paths.iter().all(|path| path.starts_with(&common)) {
        common = common.parent()?.to_path_buf();
    }
    Some(common)
}

fn normalized_logical(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() || path.as_os_str().is_empty() {
        bail!(
            "logical path {} must be non-empty and relative",
            path.display()
        );
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => out.push(value),
            _ => bail!("logical path {} is not normalized", path.display()),
        }
    }
    if out.as_os_str().is_empty() {
        bail!("logical path {} must name an entry", path.display());
    }
    Ok(out)
}

fn normalized_logical_prefix(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        bail!("logical prefix {} must be relative", path.display());
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => out.push(value),
            _ => bail!("logical prefix {} is not normalized", path.display()),
        }
    }
    Ok(out)
}

fn normalized_display(path: &Path) -> PathBuf {
    path.components()
        .filter(|component| !matches!(component, Component::CurDir))
        .collect()
}

fn path_order(left: &PathBuf, right: &PathBuf) -> std::cmp::Ordering {
    left.is_absolute()
        .cmp(&right.is_absolute())
        .then_with(|| left.cmp(right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SnapshotEntryKind;

    fn request(
        root: &Path,
        scope: ScopeSpec,
        discovery: DiscoveryPolicy,
    ) -> ProjectSnapshotRequest {
        ProjectSnapshotRequest {
            invocation_base: root.to_path_buf(),
            root: RootSpec::Explicit(root.to_path_buf()),
            repository: RepositorySpec::Explicit(
                RepositoryId::explicit("planner-test-repository").unwrap(),
            ),
            scope,
            discovery,
        }
    }

    #[test]
    fn auto_root_uses_one_repository_and_rejects_spanning_repositories() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        std::fs::create_dir_all(first.join(".jj")).unwrap();
        std::fs::create_dir_all(second.join(".git")).unwrap();
        std::fs::write(first.join("a.rs"), "fn a() {}\n").unwrap();
        std::fs::write(second.join("b.rs"), "fn b() {}\n").unwrap();

        let single = ProjectSnapshotPlanner::resolve(ProjectSnapshotRequest {
            invocation_base: first.clone(),
            root: RootSpec::Auto,
            repository: RepositorySpec::Auto,
            scope: ScopeSpec::ExactFiles(vec![first.join("a.rs")]),
            discovery: DiscoveryPolicy::Canonical,
        })
        .unwrap();
        assert_eq!(single.root(), first.canonicalize().unwrap());

        let error = ProjectSnapshotPlanner::resolve(ProjectSnapshotRequest {
            invocation_base: temp.path().to_path_buf(),
            root: RootSpec::Auto,
            repository: RepositorySpec::Auto,
            scope: ScopeSpec::ExactFiles(vec![first.join("a.rs"), second.join("b.rs")]),
            discovery: DiscoveryPolicy::Canonical,
        })
        .err()
        .expect("spanning repositories must fail");
        assert!(error.to_string().contains("spans repositories"));
    }

    #[test]
    fn planner_pins_reads_overlays_discovery_and_presentation() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("sample.rs"), "fn sample() {}\n").unwrap();
        std::fs::write(root.path().join("config.toml"), "[tool]\nvalue = 1\n").unwrap();
        std::fs::write(root.path().join("ignored.rs"), "fn ignored() {}\n").unwrap();
        std::fs::write(root.path().join(".ignore"), "ignored.rs\n").unwrap();

        let absolute = root.path().join("sample.rs");
        let mut planner = ProjectSnapshotPlanner::resolve(request(
            root.path(),
            ScopeSpec::ExactFiles(vec![absolute, PathBuf::from("sample.rs")]),
            DiscoveryPolicy::Canonical,
        ))
        .unwrap();
        planner.add_disk_analysis_input("sample.rs").unwrap();
        planner.add_disk_analysis_input("config.toml").unwrap();
        planner
            .add_analysis_input_overlay("virtual.json", br#"{"value":1}"#.to_vec())
            .unwrap();
        let built = planner.build().unwrap();
        assert_eq!(
            built.presentation.display_path(Path::new("sample.rs")),
            Path::new("sample.rs")
        );
        assert_eq!(built.snapshot.read_counts()[Path::new("sample.rs")], 1);
        assert_eq!(built.snapshot.read_counts()[Path::new("config.toml")], 1);
        assert!(
            !built
                .snapshot
                .read_counts()
                .contains_key(Path::new("virtual.json"))
        );
        assert_eq!(
            built.snapshot.entry(Path::new("sample.rs")).unwrap().kind(),
            SnapshotEntryKind::Source
        );
        assert_eq!(
            built
                .snapshot
                .entry(Path::new("config.toml"))
                .unwrap()
                .kind(),
            SnapshotEntryKind::AnalysisInput
        );

        let canonical = ProjectSnapshotPlanner::resolve(request(
            root.path(),
            ScopeSpec::Requested(vec![PathBuf::from(".")]),
            DiscoveryPolicy::Canonical,
        ))
        .unwrap()
        .build()
        .unwrap();
        let legacy = ProjectSnapshotPlanner::resolve(request(
            root.path(),
            ScopeSpec::Requested(vec![PathBuf::from(".")]),
            DiscoveryPolicy::LegacyRespectIgnore,
        ))
        .unwrap()
        .build()
        .unwrap();
        assert!(canonical.snapshot.entry(Path::new("ignored.rs")).is_some());
        assert!(legacy.snapshot.entry(Path::new("ignored.rs")).is_none());
    }

    #[test]
    fn exact_logical_overlay_does_not_require_a_live_file() {
        let root = tempfile::tempdir().unwrap();
        let mut planner = ProjectSnapshotPlanner::resolve(request(
            root.path(),
            ScopeSpec::ExactLogicalFiles(vec![PathBuf::from("unsaved.tsx")]),
            DiscoveryPolicy::Canonical,
        ))
        .unwrap();
        planner
            .add_source_overlay("unsaved.tsx", b"const View = () => <div />;\n".to_vec())
            .unwrap();
        let built = planner.build().unwrap();
        assert!(built.snapshot.entry(Path::new("unsaved.tsx")).is_some());
        assert!(built.snapshot.read_counts().is_empty());
    }

    #[test]
    fn vcs_identity_normalizes_credentials_and_root_order() {
        assert_eq!(
            normalize_remote("https://token@example.com/org/repo.git/"),
            "https://example.com/org/repo"
        );
        assert_eq!(
            normalize_remote("git@example.com:org/repo.git"),
            "example.com/org/repo"
        );
        let first = RepositoryId::vcs(
            Some("https://example.com/org/repo"),
            &["bbbb".to_string(), "aaaa".to_string()],
        )
        .unwrap();
        let reordered = RepositoryId::vcs(
            Some("https://example.com/org/repo"),
            &["aaaa".to_string(), "bbbb".to_string()],
        )
        .unwrap();
        let different =
            RepositoryId::vcs(Some("https://example.com/org/repo"), &["cccc".to_string()]).unwrap();
        assert_eq!(first, reordered);
        assert_ne!(first, different);
    }
}

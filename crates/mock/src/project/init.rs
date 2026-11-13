//! Build a complete new project from explicit SDL inside private staging.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::filesystem::Directory;
use super::staging::Staging;
use super::{ProjectError, evaluate_config};
use crate::ir::MockIr;
use crate::luau::text::Text;

/// Project creation result.
pub struct InitReport {
    /// The schema is acceptable but cannot be satisfied with a finite Mock value.
    pub required_reference_cycles: Vec<crate::ir::RequiredReferenceCycle>,
}

/// Observable phase of project initialization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitProgress {
    /// Read and parse the SDL inputs.
    ReadingSchema,
    /// Generate shared types and module candidates.
    GeneratingTypes,
    /// Validate shared declarations plus each module's types and initialization.
    ValidatingModules,
    /// Stage validated candidates and publish them exclusively.
    WritingFiles,
}

/// Create a project directory with adopted public SDL and explicit JSONL ownership metadata.
pub fn init_project(paths: &[PathBuf], output: &Path) -> Result<InitReport, ProjectError> {
    init_with(paths, output, || {})
}

/// Run the same project creation process while reporting steps to a synchronous callback.
/// The callback is invoked synchronously and must return promptly.
pub fn init_project_with_progress(
    paths: &[PathBuf],
    output: &Path,
    progress: impl Fn(InitProgress) + Send + Sync + 'static,
) -> Result<InitReport, ProjectError> {
    init_observed(paths, output, || {}, Arc::new(progress))
}

pub(super) fn init_with(
    paths: &[PathBuf],
    output: &Path,
    before_publish: impl FnOnce(),
) -> Result<InitReport, ProjectError> {
    init_observed(paths, output, before_publish, Arc::new(|_| {}))
}

fn init_observed(
    paths: &[PathBuf],
    output: &Path,
    before_publish: impl FnOnce(),
    progress: Arc<dyn Fn(InitProgress) + Send + Sync>,
) -> Result<InitReport, ProjectError> {
    let name = output.file_name().ok_or_else(path_error)?;
    let parent_path = output.parent().ok_or_else(path_error)?;
    let parent = Directory::output_parent(parent_path)?;
    let lock = parent.lock(true)?;
    parent.absent(name)?;
    progress(InitProgress::ReadingSchema);
    let cwd = std::env::current_dir().map_err(|_| path_error())?;
    let directory = Directory::open(&cwd)?;
    let arguments: Vec<_> = paths
        .iter()
        .map(|path| path.to_str().map(str::to_owned).ok_or_else(path_error))
        .collect::<Result<_, _>>()?;
    let (sources, snapshots) =
        super::sources::read_sources(&directory, &arguments).map_err(|mut error| {
            if error.message == "required project input missing" {
                error.exit = 2;
            }
            error
        })?;
    let public_count = sources.len();
    super::sources::raw_budget(super::storage::SCAFFOLD.len(), &snapshots)?;
    let storage = vec![chigiri::core::Source::new(
        "storage/001.graphql",
        super::storage::SCAFFOLD,
    )];
    let ir = Arc::new(
        MockIr::parse_with_storage(sources, storage).map_err(|error| {
            let mut failure = ProjectError::new("schema", error.code);
            if let Some(span) = error.span {
                let source = span.source_id.index();
                failure.set_file(if source < public_count {
                    format!("schema/{:03}.graphql", source + 1)
                } else {
                    "storage/001.graphql".into()
                });
            }
            failure
        })?,
    );
    let required_reference_cycles = ir.required_reference_cycles();
    let refs: Vec<_> = paths
        .iter()
        .map(|path| parent.reference_from_child_to_path(name, &directory, path))
        .collect::<Result<_, _>>()?;
    let config = config(&refs)?;
    // Evaluate the generated configuration once through the same typed, closed VM boundary as user configuration.
    let settings = evaluate_config(&config).map_err(|mut error| {
        error.set_file("mock.luau");
        error
    })?;
    if settings.sources != refs {
        return Err(path_error());
    }
    if settings.storage_sources != [super::storage::PATH] {
        return Err(path_error());
    }
    progress(InitProgress::GeneratingTypes);
    let checking = progress.clone();
    let mut generated = super::generate::generate_observed(
        ir,
        crate::hooks::HookSettings::default(),
        crate::hooks::StateContract::default(),
        Default::default(),
        Default::default(),
        move || checking(InitProgress::ValidatingModules),
    )?;
    generated
        .files
        .insert(super::fixed_path("mock.luau"), config);
    generated.files.insert(
        super::fixed_path(super::storage::PATH),
        super::storage::SCAFFOLD.into(),
    );
    generated.files.insert(
        super::fixed_path(super::agent_guidance::PATH),
        super::agent_guidance::CONTENT.into(),
    );
    progress(InitProgress::WritingFiles);
    let mut staging = Staging::new(&parent)?;
    let result = (|| {
        staging.write(&generated.files)?;
        before_publish();
        for input in &snapshots {
            input.file.verify()?;
        }
        lock.verify()?;
        staging.publish(&parent, name)
    })();
    if let Err(mut error) = result {
        if !error.committed {
            staging.cleanup(&mut error);
        }
        return Err(error);
    }
    Ok(InitReport {
        required_reference_cycles,
    })
}

fn config(sources: &[String]) -> Result<String, ProjectError> {
    let render = || {
        let mut text = Text::new(crate::limits::CONFIG_BYTES);
        text.add("--!strict\n-- External SDL paths are adoption locators; do not overwrite those files.\nlocal config: ChigiriMockProjectConfig = {\n    format = \"chigiri-mock-project/v10\",\n    schema = {sources = {\n")?;
        for source in sources {
            text.add("        ")?;
            text.quoted(source)?;
            text.add(",\n")?;
        }
        text.add("    }},\n    storage = {sources = {\"storage.graphql\"}},\n    clock = {mode = \"system\"},\n    server = {listen = \"127.0.0.1:4000\", allowed_origins = {}},\n")?;
        text.add("    hooks = {\n")?;
        for phase in ["request", "operation", "field"] {
            text.format(format_args!(
                "        {phase} = {{before = false, after = false}},\n"
            ))?;
        }
        text.add("    },\n    state = {fields = {}, initial = {}},\n    events = {limits = {}, consumers = {}},\n}\nreturn config\n")?;
        crate::luau::format::source(&text.finish(), crate::limits::CONFIG_BYTES)
    };
    render().map_err(|error| {
        let mut failure = ProjectError::new(error.phase, error.message);
        if let Some(position) = error.source_position() {
            failure.set_position(position);
        }
        failure
    })
}

fn path_error() -> ProjectError {
    ProjectError::new(
        "init",
        "explicit UTF-8 paths and a new project directory required",
    )
}

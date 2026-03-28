use clap::{Parser, Subcommand};

mod cmd_hook;
mod cmd_hydrate;
mod cmd_init;
#[cfg(feature = "cuttlefish-app")]
mod cmd_open;
mod cmd_worktree;
#[cfg(feature = "cuttlefish-app")]
mod config;
mod permissions;
#[cfg(feature = "cuttlefish-app")]
mod socket;
mod util;

#[derive(Parser)]
#[command(
    name = "cuttlefish",
    version,
    about = "CoW-accelerated git worktree hydration"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Bootstrap .worktreeinclude for the current repository
    Init {
        /// Repository path (default: current directory)
        #[arg(default_value = ".")]
        path: String,

        /// Show what would be generated without writing
        #[arg(long)]
        dry_run: bool,

        /// Show effective hydration config
        #[arg(long)]
        show: bool,
    },

    /// Hydrate a worktree with CoW-cloned build artifacts
    Hydrate {
        /// Worktree path to hydrate
        #[arg(long)]
        worktree: Option<String>,

        /// Source repo path (default: current directory)
        #[arg(long)]
        source: Option<String>,

        /// Directories to exclude from hydration
        #[arg(long, value_delimiter = ',')]
        exclude: Vec<String>,
    },

    /// Manage git worktrees with `CoW` hydration
    Worktree {
        #[command(subcommand)]
        action: WorktreeAction,
    },

    /// Handle Claude Code / Codex hook events (reads JSON from stdin)
    Hook,

    /// Open a repository in Cuttlefish.app
    #[cfg(feature = "cuttlefish-app")]
    Open {
        /// Repository path (default: current directory)
        #[arg(default_value = ".")]
        path: String,
        /// Validate without launching
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum WorktreeAction {
    /// Create a new worktree with hydration
    Create {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
    },
    /// Remove a worktree
    Remove {
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        force: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init {
            path,
            dry_run,
            show,
        } => cmd_init::run(&path, dry_run, show),
        Commands::Hydrate {
            worktree,
            source,
            exclude,
        } => {
            cmd_hydrate::run(worktree, source, exclude);
        }
        Commands::Worktree { action } => match action {
            WorktreeAction::Create { name, cwd } => cmd_worktree::run_create(name, cwd),
            WorktreeAction::Remove { path, force } => cmd_worktree::run_remove(path, force),
        },
        Commands::Hook => cmd_hook::run(),
        #[cfg(feature = "cuttlefish-app")]
        Commands::Open { path, dry_run } => cmd_open::run(&path, dry_run),
    }
}

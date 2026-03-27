use clap::Parser;

#[derive(Parser)]
#[command(name = "cuttlefish", version, about = "CoW-accelerated git worktree hydration")]
enum Cli {
    /// Bootstrap .worktreeinclude for the current repository
    Init,
}

fn main() {
    let _cli = Cli::parse();
}

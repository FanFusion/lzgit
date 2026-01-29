//! Tab rendering modules

mod explorer;
mod git;
mod log;

pub use explorer::render_explorer_tab;
pub use git::render_git_tab;
pub use log::render_log_tab;

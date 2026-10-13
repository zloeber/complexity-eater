mod app;
mod mcp;
mod notify;

pub use app::{create_router, Access, AppState};
pub use mcp::handle_mcp;

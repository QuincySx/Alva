pub mod ask_human;
pub mod create_file;
pub mod execute_shell;
pub mod file_edit;
pub mod grep_search;
pub mod list_files;

use crate::ports::tool::ToolRegistry;

/// Register all built-in tools into a ToolRegistry
pub fn register_builtin_tools(registry: &mut ToolRegistry) {
    registry.register(Box::new(execute_shell::ExecuteShellTool));
    registry.register(Box::new(create_file::CreateFileTool));
    registry.register(Box::new(file_edit::FileEditTool));
    registry.register(Box::new(grep_search::GrepSearchTool));
    registry.register(Box::new(list_files::ListFilesTool));
    registry.register(Box::new(ask_human::AskHumanTool));
}
